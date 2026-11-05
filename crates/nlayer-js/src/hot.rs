// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
//! Hot module replacement (development bundles only): the module system
//! and component registry behind `__nlayer_hot`.
//!
//! A development bundle defines every module as a factory (one chunk per
//! module) and starts the entry with the namespaces of the native modules
//! it imports (`import "nlayer:hot"` makes `__nlayer_hot`). An update
//! script redefines the modules that changed; `update` runs them again,
//! together with the importers that cannot take the change on their own,
//! in place: the JS context, the native views and every other module keep
//! their state.
//!
//! `update` returns `false` both when a change cannot reach an importer or
//! the entry and when a module declines (or an accept/dispose callback
//! throws). Only the latter sets
//! [`Host::take_hot_reload`](crate::Host::take_hot_reload); a failed bubble
//! does not. Existing hosts treat `false` as "reload".
//!
//! Components are registered by module and name and rendered through a
//! stable proxy. Running a module again swaps the implementation behind
//! its proxies; the mounted instances whose code changed render again
//! inside one reactive batch. Contexts keep their identity across updates,
//! and so do the values [`keep`] returns while their signature matches. A
//! `mount` call made again with the same component (a proxy) and no props
//! keeps what it mounted. Other `mount` calls of a module that runs again
//! are undone, from the first one that differs, and made anew.

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet, HashMap},
    ffi::{CStr, CString, c_int},
};

use rquickjs::qjs;

use crate::{dom, reactive, state};

type JsContext = qjs::JSContext;
type JsValue = qjs::JSValue;

const UNDEFINED: JsValue = qjs::JS_UNDEFINED;

/// The module dev bundles import for `__nlayer_hot`.
pub(crate) const SPECIFIER: &str = "nlayer:hot";

/// Name of the global.
const GLOBAL: &CStr = c"__nlayer_hot";

/// The exports of [`SPECIFIER`]. Evaluating the module installs the global.
pub(crate) const EXPORTS: &[crate::modules::Item] =
    &[("hot", crate::modules::Export::Value(install))];

/// A context's hot state (empty until a development bundle starts).
#[derive(Default)]
pub(crate) struct Hot {
    inner: RefCell<Inner>,
    /// `decline()`, or a throw from an accept/dispose callback. Cleared by
    /// [`crate::Host::take_hot_reload`]. Not set when `update` returns
    /// false only because a bubble could not reach an importer or the entry.
    reload: Cell<bool>,
}

#[derive(Default)]
struct Inner {
    /// Module id -> factory(module, exports, require).
    factories: HashMap<String, Owned>,
    /// Modules that never run again after their first run (the bundler runtime).
    fixed: BTreeSet<String>,
    modules: HashMap<String, Module>,
    /// Module id -> the ids of the modules that required it.
    importers: BTreeMap<String, BTreeSet<String>>,
    /// "module:name" -> component.
    components: HashMap<String, Component>,
    /// "module:name" -> context.
    contexts: HashMap<String, Owned>,
    /// "module:name" -> the signature of a kept value, and the value.
    kept: HashMap<String, (String, Owned)>,
    /// Module id -> functions undoing what its top-level code did
    /// ([`undo_on_rerun`]), called before it runs again.
    undo: HashMap<String, Vec<Owned>>,
    /// Module id -> what its `mount` calls mounted.
    renders: HashMap<String, Vec<Render>>,
    /// Module id -> the renders of its previous run, while it runs again.
    pending: HashMap<String, Vec<Render>>,
    /// Specifier -> namespace of the native modules the app imports.
    natives: Option<Owned>,
    entry: Option<String>,
    /// The module running now.
    current: Option<String>,
    /// Running an update: components whose code did not change still
    /// render again (their module ran because a dependency changed).
    forced: bool,
    /// Inside `update`: signature changes wait for one batch at the end.
    updating: bool,
    /// Components to render again once the update ran.
    stale: BTreeSet<String>,
    /// Module id -> `import.meta.hot` state. The object is stable for the
    /// life of the context, including after the module leaves the graph.
    metas: HashMap<String, Meta>,
}

/// A `mount` call.
struct Render {
    /// The component it mounted.
    app: Owned,
    /// What `mount` returned: undoes it.
    dispose: Owned,
}

struct Module {
    /// The first exports object: importers keep it; later runs forward it.
    exports: Owned,
    components: u32,
    /// Takes its own updates: everything it exports is a component proxy.
    accepts: bool,
    /// The factory has completed once. Fixed modules stop after this.
    ran: bool,
}

struct Component {
    /// The implementation now.
    fun: Owned,
    /// Changes with the component's code (or its module's other code).
    sig: String,
    /// A signal the proxy reads: bumped to render again.
    version: Owned,
    proxy: Owned,
}

/// One `accept(dep, cb)` / `accept(deps, cb)` registration.
struct DepAccept {
    /// Resolved module ids.
    deps: Vec<String>,
    /// `accept(deps, cb)` passes an array; `accept(dep, cb)` one namespace.
    array: bool,
    cb: Owned,
}

struct Meta {
    /// The object `meta(id)` returns.
    object: Owned,
    /// `import.meta.hot.data`. The same object across updates.
    data: Owned,
    /// `accept()` / `accept(cb)` during the last run.
    self_accept: bool,
    /// `accept(cb)` from the last run, called with the new namespace.
    self_cb: Option<Owned>,
    dep_accepts: Vec<DepAccept>,
    dispose: Vec<Owned>,
    prune: Vec<Owned>,
    /// `invalidate()` cancelled self-accept for the update that consumes it.
    invalidated: bool,
    /// `decline()`: this module refuses in-place updates.
    declined: bool,
}

/// A JS value this state owns. Freed on drop; never dropped while `Inner`
/// is borrowed (a finalizer can re-enter).
struct Owned {
    cx: *mut JsContext,
    value: JsValue,
}

impl Owned {
    fn dup(cx: *mut JsContext, value: JsValue) -> Self {
        // SAFETY: a new reference of a live value.
        Self {
            cx,
            value: unsafe { qjs::JS_DupValue(cx, value) },
        }
    }

    fn from_raw(cx: *mut JsContext, value: JsValue) -> Self {
        Self { cx, value }
    }

    fn clone_ref(&self) -> Self {
        Self::dup(self.cx, self.value)
    }

    fn raw(&self) -> JsValue {
        self.value
    }

    /// Hands the reference to the caller. The wrapper no longer frees it.
    fn take(self) -> JsValue {
        let value = self.value;
        std::mem::forget(self);
        value
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the reference `from_raw` / `dup` took, freed once.
        unsafe { qjs::JS_FreeValue(self.cx, self.value) };
    }
}

enum Fail {
    /// A JS exception is pending.
    Exception,
    /// `decline()` or a throwing accept/dispose callback. The reload flag
    /// is set; `update` returns false.
    Reload,
}

impl Hot {
    /// Frees every JS value it holds (before the context goes).
    pub(crate) fn free_all(&mut self) {
        self.inner.take();
        self.reload.set(false);
    }

    /// Whether a hot update asked for a full reload. True once, then clear.
    pub(crate) fn take_reload(&self) -> bool {
        self.reload.replace(false)
    }
}

/// Undoes what a development bundle mounted and drops kept signals, so
/// the next full script starts clean in this context. A module re-run
/// would keep those signals; a full reload must not.
pub(crate) fn abandon(cx: *mut JsContext) {
    let (disposes, undos, renders) = with(cx, |h| {
        let disposes: Vec<(Vec<Owned>, Owned)> = h
            .metas
            .values_mut()
            .map(|meta| (std::mem::take(&mut meta.dispose), meta.data.clone_ref()))
            .collect();
        let undos: Vec<Owned> = std::mem::take(&mut h.undo)
            .into_values()
            .flatten()
            .collect();
        let mut renders: Vec<Render> = std::mem::take(&mut h.renders)
            .into_values()
            .flatten()
            .collect();
        renders.extend(std::mem::take(&mut h.pending).into_values().flatten());
        (disposes, undos, renders)
    });
    for (callbacks, data) in &disposes {
        for cb in callbacks {
            let ran = call(cx, cb.raw(), &mut [data.raw()]);
            if is_exception(ran) {
                report_exception(cx);
            } else {
                free(cx, ran);
            }
        }
    }
    drop(disposes);
    for cb in &undos {
        let ran = call(cx, cb.raw(), &mut []);
        if is_exception(ran) {
            report_exception(cx);
        } else {
            free(cx, ran);
        }
    }
    drop(undos);
    undo_renders(cx, renders);
    let old = with(cx, std::mem::take);
    drop(old);
    hot(cx).reload.set(false);
}

fn hot(cx: *mut JsContext) -> &'static Hot {
    // SAFETY: the state `install` made, alive for every native call.
    unsafe { &(*state::state(cx)).hot }
}

fn with<T>(cx: *mut JsContext, f: impl FnOnce(&mut Inner) -> T) -> T {
    f(&mut hot(cx).inner.borrow_mut())
}

fn request_reload(cx: *mut JsContext) {
    hot(cx).reload.set(true);
}

/// Clears `updating` / `forced` however `update` returns.
struct EndUpdate {
    cx: *mut JsContext,
}

impl Drop for EndUpdate {
    fn drop(&mut self) {
        with(self.cx, |h| {
            h.updating = false;
            h.forced = false;
        });
    }
}

/// Makes `__nlayer_hot` (a global, which update scripts and the bundler's
/// component wrappers call) and returns it.
fn install(cx: *mut JsContext) -> JsValue {
    let made = (|| -> Result<Owned, Fail> {
        let obj = new_object(cx)?;
        let methods: &[(&CStr, Native, c_int)] = &[
            (c"define", define, 3),
            (c"start", start, 2),
            (c"update", update, 1),
            (c"component", component, 4),
            (c"context", context, 3),
            (c"keep", keep, 4),
            (c"meta", meta, 1),
        ];
        for &(name, f, length) in methods {
            let func = reactive::new_function(cx, name, f, length);
            if is_exception(func) {
                return Err(Fail::Exception);
            }
            define_value(cx, obj.raw(), name, func, qjs::JS_PROP_C_W_E)?;
        }
        let global = unsafe { qjs::JS_GetGlobalObject(cx) };
        // SAFETY: a new reference for the global's property; `obj` stays.
        let r = unsafe {
            qjs::JS_DefinePropertyValueStr(
                cx,
                global,
                GLOBAL.as_ptr(),
                qjs::JS_DupValue(cx, obj.raw()),
                qjs::JS_PROP_CONFIGURABLE as i32,
            )
        };
        unsafe { qjs::JS_FreeValue(cx, global) };
        if r < 0 {
            return Err(Fail::Exception);
        }
        Ok(obj)
    })();
    match made {
        Ok(v) => v.take(),
        Err(Fail::Exception) => qjs::JS_EXCEPTION,
        Err(Fail::Reload) => qjs::JS_EXCEPTION,
    }
}

/// `mount(app)` ran: what it returned is undone when the module running
/// now runs again (unless that run mounts `app` again: [`reuse`]).
pub(crate) fn rendered(cx: *mut JsContext, app: JsValue, dispose: JsValue) {
    with(cx, |h| {
        if let Some(id) = h.current.clone() {
            h.renders.entry(id).or_default().push(Render {
                app: Owned::dup(cx, app),
                dispose: Owned::dup(cx, dispose),
            });
        }
    });
}

/// `f` undoes what the module running now did at its top level: it runs
/// before the module runs again, after its dispose callbacks. Nothing
/// happens outside a development bundle's module.
#[allow(dead_code)]
pub(crate) fn undo_on_rerun(cx: *mut JsContext, f: JsValue) {
    with(cx, |h| {
        if let Some(id) = h.current.clone() {
            h.undo.entry(id).or_default().push(Owned::dup(cx, f));
        }
    });
}

/// Takes `f` back from what undoes a module's run ([`undo_on_rerun`]):
/// what it undoes ended by itself. The value returned is owned.
#[allow(dead_code)]
pub(crate) fn forget_undo(cx: *mut JsContext, f: JsValue) -> Option<JsValue> {
    let found = with(cx, |h| {
        h.undo.values_mut().find_map(|list| {
            let at = list.iter().position(|v| same(v.raw(), f))?;
            Some(list.remove(at))
        })
    });
    found.map(Owned::take)
}

/// `mount(app, props)` is called while a module runs again: when its
/// previous run's next render mounted the same `app` without props, that
/// render stays and its disposer (a new reference) is returned. Otherwise
/// the previous run's remaining renders are undone, so this one mounts
/// after what stays.
pub(crate) fn reuse(cx: *mut JsContext, app: JsValue, props: JsValue) -> Option<JsValue> {
    // SAFETY: an inline tag check.
    let has_props = unsafe { qjs::JS_IsObject(props) };
    let (kept, undo) = with(cx, |h| {
        let id = h.current.clone()?;
        let pending = h.pending.get_mut(&id)?;
        if !has_props && pending.first().is_some_and(|r| same(r.app.raw(), app)) {
            let render = pending.remove(0);
            let dispose = render.dispose.clone_ref();
            h.renders.entry(id).or_default().push(render);
            Some((Some(dispose), Vec::new()))
        } else {
            Some((None, std::mem::take(pending)))
        }
    })?;
    undo_renders(cx, undo);
    kept.map(Owned::take)
}

fn undo_renders(cx: *mut JsContext, renders: Vec<Render>) {
    for render in renders {
        let ran = call(cx, render.dispose.raw(), &mut []);
        if is_exception(ran) {
            report_exception(cx);
        } else {
            free(cx, ran);
        }
    }
}

type Native = unsafe extern "C" fn(*mut JsContext, JsValue, c_int, *mut JsValue) -> JsValue;

/// `define(id, factory, fixed)`: registers a module factory; fixed
/// modules never run again after the first run.
unsafe extern "C" fn define(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let Ok(id) = to_string(cx, arg(argc, argv, 0)) else {
        return qjs::JS_EXCEPTION;
    };
    let fixed = to_bool(cx, arg(argc, argv, 2));
    let factory = Owned::dup(cx, arg(argc, argv, 1));
    let old = with(cx, |h| {
        if fixed {
            h.fixed.insert(id.clone());
        }
        h.factories.insert(id, factory)
    });
    drop(old);
    UNDEFINED
}

/// `start(entry, natives)`: runs the entry; `natives` maps the native
/// modules' specifiers to their namespaces.
unsafe extern "C" fn start(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let Ok(entry) = to_string(cx, arg(argc, argv, 0)) else {
        return qjs::JS_EXCEPTION;
    };
    let natives_arg = arg(argc, argv, 1);
    let natives = if unsafe { qjs::JS_IsObject(natives_arg) } {
        Owned::dup(cx, natives_arg)
    } else {
        match new_object(cx) {
            Ok(obj) => obj,
            Err(Fail::Exception) => return qjs::JS_EXCEPTION,
            Err(Fail::Reload) => return qjs::JS_EXCEPTION,
        }
    };
    let old = with(cx, |h| {
        h.entry = Some(entry.clone());
        h.natives.replace(natives)
    });
    drop(old);
    match load(cx, &entry) {
        Ok(exports) => {
            drop(exports);
            UNDEFINED
        }
        Err(Fail::Exception) => qjs::JS_EXCEPTION,
        Err(Fail::Reload) => {
            request_reload(cx);
            qjs::JS_FALSE
        }
    }
}

/// `update(defs)`: replaces the factories of `defs` (id -> factory) and
/// runs the modules affected. `false` when the update cannot be applied
/// in place (the host reloads). A decline or a throwing accept/dispose
/// callback also sets the reload flag; a bubble that stops short does not.
unsafe extern "C" fn update(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    match update_inner(cx, arg(argc, argv, 0)) {
        Ok(true) => qjs::JS_TRUE,
        Ok(false) | Err(Fail::Reload) => qjs::JS_FALSE,
        Err(Fail::Exception) => qjs::JS_EXCEPTION,
    }
}

fn update_inner(cx: *mut JsContext, defs: JsValue) -> Result<bool, Fail> {
    if hot(cx).reload.get() {
        // A module already declined (often during its first run).
        return Ok(false);
    }
    let record = own_values(cx, defs)?;
    let mut changed = Vec::new();
    let mut defined = BTreeSet::new();
    for (id, factory) in record {
        let old = with(cx, |h| {
            if h.modules.contains_key(&id) && !h.fixed.contains(&id) {
                changed.push(id.clone());
            }
            h.factories.insert(id.clone(), factory)
        });
        drop(old);
        defined.insert(id);
    }
    with(cx, |h| {
        h.updating = true;
        h.stale.clear();
    });
    let _end = EndUpdate { cx };
    let Some((order, consumed)) = with(cx, |h| plan(h, changed, &BTreeSet::new())) else {
        return Ok(false);
    };
    clear_invalidated(cx, &consumed);
    let mut ran = BTreeSet::new();
    for id in &order {
        if declined(cx, id) {
            request_reload(cx);
            return Err(Fail::Reload);
        }
        with(cx, |h| h.forced = !defined.contains(id));
        run(cx, id)?;
        ran.insert(id.clone());
        if hot(cx).reload.get() {
            return Err(Fail::Reload);
        }
    }
    // `invalidate()` during a run cancels that module's self-accept and
    // bubbles. `accept(dep)` callbacks run for importers that did not rerun.
    loop {
        let invalidated = with(cx, |h| take_invalidated(h, &ran));
        if !invalidated.is_empty() {
            let seeds: Vec<String> = invalidated.into_iter().collect();
            let Some((extra, consumed)) = with(cx, |h| plan(h, seeds, &ran)) else {
                return Ok(false);
            };
            clear_invalidated(cx, &consumed);
            for id in &extra {
                if declined(cx, id) {
                    request_reload(cx);
                    return Err(Fail::Reload);
                }
                with(cx, |h| h.forced = !defined.contains(id));
                run(cx, id)?;
                ran.insert(id.clone());
                if hot(cx).reload.get() {
                    return Err(Fail::Reload);
                }
            }
            continue;
        }
        let callbacks = with(cx, |h| dep_callbacks(h, &ran));
        if callbacks.is_empty() {
            break;
        }
        // Mark them ran so a callback that does not invalidate is not
        // fired again; an invalidate still bubbles on the next pass.
        with(cx, |h| h.forced = false);
        for (id, _) in &callbacks {
            ran.insert(id.clone());
        }
        fire_dep_callbacks(cx, callbacks)?;
        if hot(cx).reload.get() {
            return Err(Fail::Reload);
        }
    }
    with(cx, |h| h.forced = false);
    prune_unreachable(cx)?;
    if hot(cx).reload.get() {
        return Err(Fail::Reload);
    }
    let stale = with(cx, |h| {
        h.forced = false;
        std::mem::take(&mut h.stale)
            .into_iter()
            .filter_map(|k| h.components.get(&k).map(|c| c.version.clone_ref()))
            .collect::<Vec<_>>()
    });
    reactive::batch_with(cx, || {
        for version in &stale {
            let n = peek_number(cx, version.raw());
            let next = qjs::JS_NewFloat64(n + 1.0);
            reactive::set_signal(cx, version.raw(), next);
        }
    });
    Ok(true)
}

fn declined(cx: *mut JsContext, id: &str) -> bool {
    with(cx, |h| h.metas.get(id).is_some_and(|m| m.declined))
}

fn clear_invalidated(cx: *mut JsContext, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    with(cx, |h| {
        for id in ids {
            if let Some(meta) = h.metas.get_mut(id) {
                meta.invalidated = false;
            }
        }
    });
}

fn take_invalidated(h: &mut Inner, ran: &BTreeSet<String>) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for (id, meta) in &mut h.metas {
        if meta.invalidated && ran.contains(id) {
            meta.invalidated = false;
            ids.insert(id.clone());
        }
    }
    ids
}

/// The modules to run again for `seeds`, dependencies before their
/// importers. `already` propagates (an `invalidate`) but is not run again.
/// `None` when a change reaches a module nothing imports and that is not
/// the entry. The second vector is the ids whose `invalidated` flag this
/// walk consumed.
fn plan(
    h: &Inner,
    seeds: Vec<String>,
    already: &BTreeSet<String>,
) -> Option<(Vec<String>, Vec<String>)> {
    let mut rerun = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut consumed = Vec::new();
    let mut queue = seeds;
    while let Some(id) = queue.pop() {
        if h.fixed.contains(&id) {
            continue;
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        if h.metas.get(&id).is_some_and(|m| m.invalidated) {
            consumed.push(id.clone());
        }
        if already.contains(&id) {
            push_importers(h, &id, &mut queue)?;
            continue;
        }
        rerun.insert(id.clone());
        if stops(h, &id) {
            continue;
        }
        push_importers(h, &id, &mut queue)?;
    }
    Some((topo(h, &rerun), consumed))
}

fn stops(h: &Inner, id: &str) -> bool {
    if h.entry.as_deref() == Some(id) {
        return true;
    }
    if h.metas.get(id).is_some_and(|m| m.invalidated) {
        return false;
    }
    let heuristic = h.modules.get(id).is_some_and(|m| m.accepts);
    let explicit = h.metas.get(id).is_some_and(|m| m.self_accept);
    heuristic || explicit
}

fn push_importers(h: &Inner, id: &str, queue: &mut Vec<String>) -> Option<()> {
    let from = h.importers.get(id).filter(|f| !f.is_empty())?;
    for importer in from {
        if h.fixed.contains(importer) {
            continue;
        }
        if accepts_dep(h, importer, id) {
            continue;
        }
        queue.push(importer.clone());
    }
    Some(())
}

fn accepts_dep(h: &Inner, importer: &str, dep: &str) -> bool {
    h.metas.get(importer).is_some_and(|m| {
        m.dep_accepts
            .iter()
            .any(|a| a.deps.iter().any(|d| d == dep))
    })
}

fn topo(h: &Inner, rerun: &BTreeSet<String>) -> Vec<String> {
    fn visit(
        h: &Inner,
        rerun: &BTreeSet<String>,
        id: &str,
        seen: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) {
        if !seen.insert(id.to_owned()) {
            return;
        }
        for (dep, from) in &h.importers {
            if rerun.contains(dep) && from.contains(id) {
                visit(h, rerun, dep, seen, order);
            }
        }
        order.push(id.to_owned());
    }
    let (mut seen, mut order) = (BTreeSet::new(), Vec::new());
    for id in rerun {
        visit(h, rerun, id, &mut seen, &mut order);
    }
    order
}

/// `(accepter, registration index)` whose accepted dep reran and who did not.
fn dep_callbacks(h: &Inner, ran: &BTreeSet<String>) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (id, meta) in &h.metas {
        if ran.contains(id) {
            continue;
        }
        for (i, accept) in meta.dep_accepts.iter().enumerate() {
            if accept.deps.iter().any(|dep| ran.contains(dep)) {
                out.push((id.clone(), i));
            }
        }
    }
    out
}

fn fire_dep_callbacks(cx: *mut JsContext, callbacks: Vec<(String, usize)>) -> Result<(), Fail> {
    for (id, index) in callbacks {
        let Some((deps, array, cb)) = with(cx, |h| {
            let meta = h.metas.get(&id)?;
            let accept = meta.dep_accepts.get(index)?;
            Some((accept.deps.clone(), accept.array, accept.cb.clone_ref()))
        }) else {
            continue;
        };
        let arg = if array {
            let mut values = Vec::with_capacity(deps.len());
            for dep in &deps {
                values.push(namespace_of(cx, dep));
            }
            new_array(cx, values)
        } else {
            namespace_of(cx, deps.first().map(String::as_str).unwrap_or(""))
        };
        let ran = call(cx, cb.raw(), &mut [arg]);
        free(cx, arg);
        if is_exception(ran) {
            report_exception(cx);
            request_reload(cx);
            return Err(Fail::Reload);
        }
        free(cx, ran);
    }
    Ok(())
}

fn namespace_of(cx: *mut JsContext, id: &str) -> JsValue {
    with(cx, |h| {
        h.modules
            .get(id)
            .map(|m| m.exports.clone_ref())
            .map(Owned::take)
            .unwrap_or(UNDEFINED)
    })
}

fn prune_unreachable(cx: *mut JsContext) -> Result<(), Fail> {
    loop {
        let (ids, entry) = with(cx, |h| (reachable(h), h.entry.clone()));
        let leaving = with(cx, |h| {
            h.modules
                .keys()
                .find(|id| entry.as_deref() != Some(id.as_str()) && !ids.contains(*id))
                .filter(|id| !h.fixed.contains(*id))
                .cloned()
        });
        let Some(id) = leaving else {
            return Ok(());
        };
        let (module, meta, renders, undo) = with(cx, |h| {
            h.importers.remove(&id);
            for set in h.importers.values_mut() {
                set.remove(&id);
            }
            let meta = h.metas.get_mut(&id).map(|meta| {
                meta.self_accept = false;
                let dep = std::mem::take(&mut meta.dep_accepts);
                let dispose = std::mem::take(&mut meta.dispose);
                let prune = std::mem::take(&mut meta.prune);
                let data = Some(meta.data.clone_ref());
                let self_cb = meta.self_cb.take();
                (dep, dispose, prune, data, self_cb)
            });
            (
                h.modules.remove(&id),
                meta,
                h.renders.remove(&id).unwrap_or_default(),
                h.undo.remove(&id).unwrap_or_default(),
            )
        });
        drop(module);
        undo_renders(cx, renders);
        for cb in undo {
            let ran = call(cx, cb.raw(), &mut []);
            if is_exception(ran) {
                report_exception(cx);
            } else {
                free(cx, ran);
            }
        }
        let Some((_dep, _dispose, prune, data, _self_cb)) = meta else {
            continue;
        };
        let data = data.as_ref().map_or(UNDEFINED, Owned::raw);
        for cb in prune {
            let ran = call(cx, cb.raw(), &mut [data]);
            if is_exception(ran) {
                report_exception(cx);
                request_reload(cx);
                return Err(Fail::Reload);
            }
            free(cx, ran);
        }
    }
}

fn reachable(h: &Inner) -> BTreeSet<String> {
    let Some(entry) = h.entry.clone() else {
        return BTreeSet::new();
    };
    let mut seen = BTreeSet::new();
    let mut stack = vec![entry];
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for (dep, importers) in &h.importers {
            if importers.contains(&id) {
                stack.push(dep.clone());
            }
        }
    }
    seen
}

/// The exports of module `id`, running it the first time.
fn load(cx: *mut JsContext, id: &str) -> Result<Owned, Fail> {
    if let Some(exports) = with(cx, |h| h.modules.get(id).map(|m| m.exports.clone_ref())) {
        return Ok(exports);
    }
    let exports = new_object(cx)?;
    with(cx, |h| {
        h.modules.insert(
            id.to_owned(),
            Module {
                exports: exports.clone_ref(),
                components: 0,
                accepts: false,
                ran: false,
            },
        );
    });
    run(cx, id)?;
    Ok(exports)
}

/// Runs module `id` (again).
fn run(cx: *mut JsContext, id: &str) -> Result<(), Fail> {
    let Some((factory, kept, fixed, ran)) = with(cx, |h| {
        let factory = h.factories.get(id)?.clone_ref();
        let module = h.modules.get(id)?;
        Some((
            factory,
            module.exports.clone_ref(),
            h.fixed.contains(id),
            module.ran,
        ))
    }) else {
        return Err(throw_type(cx, &format!("nlayer hot: no module {id}")).into());
    };
    if fixed && ran {
        return Ok(());
    }
    let (dispose, self_cb, undo, dep_old, prune_old, stale_pending) = with(cx, |h| {
        if let Some(module) = h.modules.get_mut(id) {
            module.components = 0;
        }
        let previous = h.renders.remove(id).unwrap_or_default();
        let stale_pending = h.pending.insert(id.to_owned(), previous);
        forget_imports(h, id);
        let meta = h.metas.get_mut(id);
        match meta {
            Some(meta) => (
                std::mem::take(&mut meta.dispose),
                meta.self_cb.take(),
                h.undo.remove(id).unwrap_or_default(),
                std::mem::take(&mut meta.dep_accepts),
                std::mem::take(&mut meta.prune),
                stale_pending,
            ),
            None => (
                Vec::new(),
                None,
                h.undo.remove(id).unwrap_or_default(),
                Vec::new(),
                Vec::new(),
                stale_pending,
            ),
        }
    });
    drop(stale_pending);
    // The new run registers its own accept/dispose/prune. Drop the previous
    // dep/prune callbacks once the borrow is gone; dispose and the self
    // callback still run.
    drop(dep_old);
    drop(prune_old);
    with(cx, |h| {
        if let Some(meta) = h.metas.get_mut(id) {
            meta.self_accept = false;
        }
    });
    let data = with(cx, |h| h.metas.get(id).map(|m| m.data.raw())).unwrap_or(UNDEFINED);
    for cb in &dispose {
        let ran = call(cx, cb.raw(), &mut [data]);
        if is_exception(ran) {
            report_exception(cx);
            request_reload(cx);
            return Err(Fail::Reload);
        }
        free(cx, ran);
    }
    for cb in &undo {
        let ran = call(cx, cb.raw(), &mut []);
        if is_exception(ran) {
            report_exception(cx);
        } else {
            free(cx, ran);
        }
    }
    let exports = if fixed {
        kept.clone_ref()
    } else {
        new_object(cx)?
    };
    let module = new_object(cx)?;
    define_value(
        cx,
        module.raw(),
        c"exports",
        exports.clone_ref().take(),
        qjs::JS_PROP_C_W_E,
    )?;
    let from = new_string(cx, id)?;
    let mut require_data = [from.raw()];
    // SAFETY: the function keeps a dup of `from`.
    let require = unsafe {
        qjs::JS_NewCFunctionData(cx, Some(require_call), 1, 0, 1, require_data.as_mut_ptr())
    };
    drop(from);
    if is_exception(require) {
        drop(module);
        return Err(Fail::Exception);
    }
    let previous = with(cx, |h| h.current.replace(id.to_owned()));
    let mut args = [module.raw(), exports.raw(), require];
    let ran_value = call(cx, factory.raw(), &mut args);
    free(cx, require);
    drop(module);
    with(cx, |h| h.current = previous);
    let left = with(cx, |h| h.pending.remove(id).unwrap_or_default());
    undo_renders(cx, left);
    if is_exception(ran_value) {
        return Err(Fail::Exception);
    }
    free(cx, ran_value);
    if !same(exports.raw(), kept.raw()) {
        forward(cx, &kept, &exports)?;
    }
    let accepts = accepts(cx, id, &kept)?;
    with(cx, |h| {
        if let Some(module) = h.modules.get_mut(id) {
            module.accepts = accepts;
            module.ran = true;
        }
    });
    let invalidated = with(cx, |h| h.metas.get(id).is_some_and(|m| m.invalidated));
    if let Some(cb) = self_cb
        && !invalidated
    {
        let ran = call(cx, cb.raw(), &mut [kept.raw()]);
        if is_exception(ran) {
            report_exception(cx);
            request_reload(cx);
            return Err(Fail::Reload);
        }
        free(cx, ran);
    }
    if hot(cx).reload.get() {
        return Err(Fail::Reload);
    }
    Ok(())
}

fn forget_imports(h: &mut Inner, id: &str) {
    for set in h.importers.values_mut() {
        set.remove(id);
    }
}

/// A factory's `require(spec)`: a native module's namespace, or the
/// exports of the app's module `spec` names (relative to `from`).
unsafe extern "C" fn require_call(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: one data value, the module id.
    let from = match to_string(cx, unsafe { *data }) {
        Ok(from) => from,
        Err(e) => return e,
    };
    let spec = match to_string(cx, arg(argc, argv, 0)) {
        Ok(spec) => spec,
        Err(e) => return e,
    };
    if !spec.starts_with('.') {
        let natives = with(cx, |h| h.natives.as_ref().map(Owned::clone_ref));
        let Some(natives) = natives else {
            return throw_type(cx, "nlayer hot: no native modules").take_exception();
        };
        let namespace = match get(cx, natives.raw(), &spec) {
            Ok(v) => v,
            Err(Fail::Exception) => return qjs::JS_EXCEPTION,
            Err(Fail::Reload) => return qjs::JS_EXCEPTION,
        };
        if is_undefined(namespace.raw()) {
            drop(namespace);
            return throw_type(cx, &format!("nlayer hot: no module {spec}")).take_exception();
        }
        return namespace.take();
    }
    let id = resolve(&from, &spec);
    with(cx, |h| {
        h.importers.entry(id.clone()).or_default().insert(from);
    });
    match load(cx, &id) {
        Ok(exports) => exports.take(),
        Err(Fail::Exception) => qjs::JS_EXCEPTION,
        Err(Fail::Reload) => {
            request_reload(cx);
            throw_type(cx, "nlayer hot: the module declined to run").take_exception()
        }
    }
}

/// `spec` relative to module `from` (`a/b.js` + `../c.js` -> `c.js`).
fn resolve(from: &str, spec: &str) -> String {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for part in spec.split('/') {
        match part {
            ".." => {
                parts.pop();
            }
            "." => {}
            part => parts.push(part),
        }
    }
    parts.join("/")
}

fn resolve_dep(from: &str, spec: &str) -> String {
    if spec.starts_with('.') {
        resolve(from, spec)
    } else {
        spec.to_owned()
    }
}

/// Makes `kept` (what importers hold) forward to `exports` (the run's):
/// its keys follow, read through getters.
fn forward(cx: *mut JsContext, kept: &Owned, exports: &Owned) -> Result<(), Fail> {
    let new_keys = own_keys(cx, exports.raw())?;
    let old_keys = own_keys(cx, kept.raw())?;
    for (key, _) in &old_keys {
        if !new_keys.iter().any(|(k, _)| k == key) {
            delete_prop(cx, kept.raw(), key)?;
        }
    }
    for (key, enumerable) in new_keys {
        let key_js = new_string(cx, &key)?;
        let mut data = [exports.clone_ref().take(), key_js.take()];
        // SAFETY: the getter keeps dups of the exports object and the key.
        let getter =
            unsafe { qjs::JS_NewCFunctionData(cx, Some(forward_get), 0, 0, 2, data.as_mut_ptr()) };
        free(cx, data[0]);
        free(cx, data[1]);
        if is_exception(getter) {
            return Err(Fail::Exception);
        }
        let mut flags = qjs::JS_PROP_CONFIGURABLE
            | qjs::JS_PROP_HAS_GET
            | qjs::JS_PROP_HAS_CONFIGURABLE
            | qjs::JS_PROP_HAS_ENUMERABLE;
        if enumerable {
            flags |= qjs::JS_PROP_ENUMERABLE;
        }
        let name = CString::new(key).unwrap_or_default();
        // SAFETY: a new atom, freed below; the getter is borrowed and the
        // property keeps its own reference.
        let r = unsafe {
            let atom = qjs::JS_NewAtom(cx, name.as_ptr());
            let r = qjs::JS_DefineProperty(
                cx,
                kept.raw(),
                atom,
                UNDEFINED,
                getter,
                UNDEFINED,
                flags as i32,
            );
            qjs::JS_FreeAtom(cx, atom);
            r
        };
        free(cx, getter);
        if r < 0 {
            return Err(Fail::Exception);
        }
    }
    Ok(())
}

unsafe extern "C" fn forward_get(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values, the exports object and the key.
    let (exports, key) = unsafe { (*data, *data.add(1)) };
    let Ok(key) = to_string(cx, key) else {
        return qjs::JS_EXCEPTION;
    };
    match get(cx, exports, &key) {
        Ok(v) => v.take(),
        Err(_) => qjs::JS_EXCEPTION,
    }
}

/// The own string keys of `obj`, and whether each is enumerable.
fn own_keys(cx: *mut JsContext, obj: JsValue) -> Result<Vec<(String, bool)>, Fail> {
    let mut tab: *mut qjs::JSPropertyEnum = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: an enumeration of a live object, freed below.
    unsafe {
        if qjs::JS_GetOwnPropertyNames(cx, &mut tab, &mut len, obj, qjs::JS_GPN_STRING_MASK as i32)
            < 0
        {
            return Err(Fail::Exception);
        }
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len as usize {
            let e = &*tab.add(i);
            let name = qjs::JS_AtomToCStringLen(cx, std::ptr::null_mut(), e.atom);
            if !name.is_null() {
                let key = CStr::from_ptr(name).to_string_lossy().into_owned();
                qjs::JS_FreeCString(cx, name);
                out.push((key, e.is_enumerable));
            }
        }
        qjs::JS_FreePropertyEnum(cx, tab, len);
        Ok(out)
    }
}

/// Enumerable own string keys of `obj` and their values (owned).
fn own_values(cx: *mut JsContext, obj: JsValue) -> Result<Vec<(String, Owned)>, Fail> {
    if unsafe { !qjs::JS_IsObject(obj) } {
        return Err(throw_type(cx, "nlayer hot: update expected an object").into());
    }
    let mut tab: *mut qjs::JSPropertyEnum = std::ptr::null_mut();
    let mut len = 0u32;
    let flags = (qjs::JS_GPN_STRING_MASK | qjs::JS_GPN_ENUM_ONLY) as i32;
    // SAFETY: an enumeration of a live object, freed below.
    unsafe {
        if qjs::JS_GetOwnPropertyNames(cx, &mut tab, &mut len, obj, flags) < 0 {
            return Err(Fail::Exception);
        }
    }
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len as usize {
        // SAFETY: `len` entries the enumeration returned.
        let atom = unsafe { (*tab.add(i)).atom };
        let name = unsafe { qjs::JS_AtomToCStringLen(cx, std::ptr::null_mut(), atom) };
        if name.is_null() {
            unsafe { qjs::JS_FreePropertyEnum(cx, tab, len) };
            return Err(Fail::Exception);
        }
        let key = unsafe { CStr::from_ptr(name).to_string_lossy().into_owned() };
        unsafe { qjs::JS_FreeCString(cx, name) };
        let value = match get(cx, obj, &key) {
            Ok(value) => value,
            Err(_) => {
                unsafe { qjs::JS_FreePropertyEnum(cx, tab, len) };
                return Err(Fail::Exception);
            }
        };
        out.push((key, value));
    }
    unsafe { qjs::JS_FreePropertyEnum(cx, tab, len) };
    Ok(out)
}

/// Whether module `id` takes its own updates: it made components and
/// everything it exports (enumerably) is one of their proxies.
fn accepts(cx: *mut JsContext, id: &str, exports: &Owned) -> Result<bool, Fail> {
    let made = with(cx, |h| h.modules.get(id).map_or(0, |m| m.components));
    if made == 0 {
        return Ok(false);
    }
    for (key, enumerable) in own_keys(cx, exports.raw())? {
        if !enumerable {
            continue;
        }
        let value = get(cx, exports.raw(), &key)?;
        let proxy = with(cx, |h| {
            h.components
                .values()
                .any(|c| same(c.proxy.raw(), value.raw()))
        });
        drop(value);
        if !proxy {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `component(module, name, fn, signature)`: the stable proxy of a
/// component; a new signature renders its instances again.
unsafe extern "C" fn component(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let Ok(module) = to_string(cx, arg(argc, argv, 0)) else {
        return qjs::JS_EXCEPTION;
    };
    let Ok(name) = to_string(cx, arg(argc, argv, 1)) else {
        return qjs::JS_EXCEPTION;
    };
    let Ok(sig) = to_string(cx, arg(argc, argv, 3)) else {
        return qjs::JS_EXCEPTION;
    };
    let key = format!("{module}:{name}");
    let fun_js = arg(argc, argv, 2);
    if let Some(proxy) = update_component(cx, &key, sig.clone(), fun_js) {
        return proxy;
    }
    let fun = Owned::dup(cx, fun_js);
    let initial = qjs::JS_NewFloat64(0.0);
    let version_js = reactive::new_signal(cx, initial);
    free(cx, initial);
    if is_exception(version_js) {
        return version_js;
    }
    let version = Owned::from_raw(cx, version_js);
    let key_js = match new_string(cx, &key) {
        Ok(v) => v,
        Err(Fail::Exception) => return qjs::JS_EXCEPTION,
        Err(Fail::Reload) => return qjs::JS_EXCEPTION,
    };
    let mut data = [version.raw(), key_js.raw()];
    // SAFETY: the proxy keeps dups of the version signal and the key.
    let proxy_js =
        unsafe { qjs::JS_NewCFunctionData(cx, Some(proxy_call), 1, 0, 2, data.as_mut_ptr()) };
    drop(key_js);
    if is_exception(proxy_js) {
        return proxy_js;
    }
    let proxy = Owned::from_raw(cx, proxy_js);
    if let Ok(name_js) = new_string(cx, &name)
        && define_value(
            cx,
            proxy.raw(),
            c"name",
            name_js.take(),
            qjs::JS_PROP_CONFIGURABLE,
        )
        .is_err()
    {
        // A fresh function takes a name; drop the error rather than
        // returning the proxy with an exception pending.
        report_exception(cx);
    }
    let returned = proxy.clone_ref();
    let old = with(cx, |h| {
        h.components.insert(
            key,
            Component {
                fun,
                sig,
                version,
                proxy,
            },
        )
    });
    drop(old);
    returned.take()
}

fn count_component(h: &mut Inner) {
    if let Some(id) = h.current.clone()
        && let Some(module) = h.modules.get_mut(&id)
    {
        module.components += 1;
    }
}

/// The proxy of a component that already exists: swaps its implementation,
/// marks it stale, and returns the proxy. `None` for a new component.
fn update_component(cx: *mut JsContext, key: &str, sig: String, fun: JsValue) -> Option<JsValue> {
    let (proxy, old, bump) = with(cx, |h| {
        count_component(h);
        let forced = h.forced;
        let updating = h.updating;
        let c = h.components.get_mut(key)?;
        let stale = c.sig != sig || forced;
        let old = std::mem::replace(&mut c.fun, Owned::dup(cx, fun));
        c.sig = sig;
        let proxy = c.proxy.clone_ref();
        let bump = if stale {
            if updating {
                h.stale.insert(key.to_owned());
                None
            } else {
                Some(c.version.clone_ref())
            }
        } else {
            None
        };
        Some((proxy, Some(old), bump))
    })?;
    drop(old);
    if let Some(version) = bump {
        bump_version(cx, &version);
    }
    Some(proxy.take())
}

fn bump_version(cx: *mut JsContext, version: &Owned) {
    reactive::batch_with(cx, || {
        let n = peek_number(cx, version.raw());
        let next = qjs::JS_NewFloat64(n + 1.0);
        reactive::set_signal(cx, version.raw(), next);
    });
}

unsafe extern "C" fn proxy_call(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values, the version signal and the component key.
    let (version, key) = unsafe { (*data, *data.add(1)) };
    let props = arg(argc, argv, 0);
    let mut fdata = [version, key, props];
    // SAFETY: the memo's function keeps dups.
    let run =
        unsafe { qjs::JS_NewCFunctionData(cx, Some(proxy_render), 0, 0, 3, fdata.as_mut_ptr()) };
    if is_exception(run) {
        return run;
    }
    let memo = reactive::memo(cx, run, false);
    reactive::finish_value(cx, memo)
}

/// A proxy's render: a memo of the component's implementation now, run
/// again when its version changes.
unsafe extern "C" fn proxy_render(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: three data values (`proxy_call`).
    let (version, key_js, props) = unsafe { (*data, *data.add(1), *data.add(2)) };
    let read = call(cx, version, &mut []);
    if is_exception(read) {
        return read;
    }
    free(cx, read);
    let Ok(key) = to_string(cx, key_js) else {
        return qjs::JS_EXCEPTION;
    };
    let Some(fun) = with(cx, |h| h.components.get(&key).map(|c| c.fun.clone_ref())) else {
        return UNDEFINED;
    };
    let mut args = [props];
    let out = reactive::untracked_with(cx, || call(cx, fun.raw(), &mut args));
    if is_exception(out) {
        // A component that throws reports the error and renders nothing,
        // ready for the fix.
        report_exception(cx);
        return UNDEFINED;
    }
    out
}

/// `context(module, name, context)`: the first context made under that
/// name keeps its identity (taking the new default).
unsafe extern "C" fn context(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let Ok(module) = to_string(cx, arg(argc, argv, 0)) else {
        return qjs::JS_EXCEPTION;
    };
    let Ok(name) = to_string(cx, arg(argc, argv, 1)) else {
        return qjs::JS_EXCEPTION;
    };
    let key = format!("{module}:{name}");
    let made = arg(argc, argv, 2);
    let kept = with(cx, |h| h.contexts.get(&key).map(Owned::clone_ref));
    match kept {
        Some(kept) => {
            dom::set_context_default(cx, kept.raw(), made);
            kept.take()
        }
        None => {
            let stored = Owned::dup(cx, made);
            let returned = stored.clone_ref();
            with(cx, |h| h.contexts.insert(key, stored));
            returned.take()
        }
    }
}

/// `keep(module, name, signature, value)`: the value first made under that
/// name while its signature stays the same; otherwise `value`.
unsafe extern "C" fn keep(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let Ok(module) = to_string(cx, arg(argc, argv, 0)) else {
        return qjs::JS_EXCEPTION;
    };
    let Ok(name) = to_string(cx, arg(argc, argv, 1)) else {
        return qjs::JS_EXCEPTION;
    };
    let Ok(sig) = to_string(cx, arg(argc, argv, 2)) else {
        return qjs::JS_EXCEPTION;
    };
    let key = format!("{module}:{name}");
    let made = arg(argc, argv, 3);
    let (returned, old) = with(cx, |h| match h.kept.get(&key) {
        Some((kept_sig, kept)) if *kept_sig == sig => (kept.clone_ref(), None),
        _ => {
            let stored = Owned::dup(cx, made);
            let returned = stored.clone_ref();
            let old = h.kept.insert(key, (sig, stored));
            (returned, old)
        }
    });
    drop(old);
    returned.take()
}

/// `meta(id)`: the stable `import.meta.hot` object of module `id`.
unsafe extern "C" fn meta(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let Ok(id) = to_string(cx, arg(argc, argv, 0)) else {
        return qjs::JS_EXCEPTION;
    };
    if let Some(existing) = with(cx, |h| h.metas.get(&id).map(|m| m.object.clone_ref())) {
        return existing.take();
    }
    match make_meta(cx, &id) {
        Ok(v) => v.take(),
        Err(Fail::Exception) => qjs::JS_EXCEPTION,
        Err(Fail::Reload) => qjs::JS_EXCEPTION,
    }
}

fn make_meta(cx: *mut JsContext, id: &str) -> Result<Owned, Fail> {
    let object = new_object(cx)?;
    let data = new_object(cx)?;
    define_value(
        cx,
        object.raw(),
        c"data",
        data.clone_ref().take(),
        qjs::JS_PROP_C_W_E,
    )?;
    let id_js = new_string(cx, id)?;
    let methods: &[(&CStr, DataNative, c_int)] = &[
        (c"accept", meta_accept, 2),
        (c"dispose", meta_dispose, 1),
        (c"prune", meta_prune, 1),
        (c"invalidate", meta_invalidate, 0),
        (c"decline", meta_decline, 0),
    ];
    for &(name, f, length) in methods {
        let mut fdata = [id_js.raw()];
        // SAFETY: the method keeps a dup of the module id.
        let func =
            unsafe { qjs::JS_NewCFunctionData(cx, Some(f), length, 0, 1, fdata.as_mut_ptr()) };
        if is_exception(func) {
            return Err(Fail::Exception);
        }
        define_value(cx, object.raw(), name, func, qjs::JS_PROP_C_W_E)?;
    }
    drop(id_js);
    let returned = object.clone_ref();
    let old = with(cx, |h| {
        h.metas.insert(
            id.to_owned(),
            Meta {
                object,
                data,
                self_accept: false,
                self_cb: None,
                dep_accepts: Vec::new(),
                dispose: Vec::new(),
                prune: Vec::new(),
                invalidated: false,
                declined: false,
            },
        )
    });
    drop(old);
    Ok(returned)
}

type DataNative = unsafe extern "C" fn(
    *mut JsContext,
    JsValue,
    c_int,
    *mut JsValue,
    c_int,
    *mut JsValue,
) -> JsValue;

unsafe extern "C" fn meta_accept(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    let Ok(id) = (unsafe { to_string(cx, *data) }) else {
        return qjs::JS_EXCEPTION;
    };
    let first = arg(argc, argv, 0);
    let second = arg(argc, argv, 1);
    if argc <= 0 || is_undefined(first) {
        let old = with(cx, |h| match h.metas.get_mut(&id) {
            Some(meta) => {
                meta.self_accept = true;
                meta.self_cb.take()
            }
            None => None,
        });
        drop(old);
        return UNDEFINED;
    }
    if argc == 1 && is_function(cx, first) {
        let cb = Owned::dup(cx, first);
        let old = with(cx, |h| match h.metas.get_mut(&id) {
            Some(meta) => {
                meta.self_accept = true;
                meta.self_cb.replace(cb)
            }
            None => Some(cb),
        });
        drop(old);
        return UNDEFINED;
    }
    if !is_function(cx, second) {
        return throw_type(cx, "nlayer hot: accept expected a callback").take_exception();
    }
    let deps = if is_array(cx, first) {
        match array_strings(cx, first) {
            Ok(deps) => deps,
            Err(e) => return e,
        }
    } else {
        match to_string(cx, first) {
            Ok(dep) => vec![dep],
            Err(e) => return e,
        }
    };
    let array = is_array(cx, first);
    let deps: Vec<String> = deps.iter().map(|dep| resolve_dep(&id, dep)).collect();
    let accept = DepAccept {
        deps,
        array,
        cb: Owned::dup(cx, second),
    };
    let unused = with(cx, |h| match h.metas.get_mut(&id) {
        Some(meta) => {
            meta.dep_accepts.push(accept);
            None
        }
        None => Some(accept),
    });
    drop(unused);
    UNDEFINED
}

unsafe extern "C" fn meta_dispose(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    push_callback(cx, argc, argv, data, true)
}

unsafe extern "C" fn meta_prune(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    push_callback(cx, argc, argv, data, false)
}

fn push_callback(
    cx: *mut JsContext,
    argc: c_int,
    argv: *mut JsValue,
    data: *mut JsValue,
    dispose: bool,
) -> JsValue {
    let Ok(id) = (unsafe { to_string(cx, *data) }) else {
        return qjs::JS_EXCEPTION;
    };
    let cb = arg(argc, argv, 0);
    if !is_function(cx, cb) {
        return throw_type(cx, "nlayer hot: expected a function").take_exception();
    }
    let cb = Owned::dup(cx, cb);
    let unused = with(cx, |h| match h.metas.get_mut(&id) {
        Some(meta) => {
            if dispose {
                meta.dispose.push(cb);
            } else {
                meta.prune.push(cb);
            }
            None
        }
        None => Some(cb),
    });
    drop(unused);
    UNDEFINED
}

unsafe extern "C" fn meta_invalidate(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    let Ok(id) = (unsafe { to_string(cx, *data) }) else {
        return qjs::JS_EXCEPTION;
    };
    with(cx, |h| {
        if let Some(meta) = h.metas.get_mut(&id) {
            // Cancels self-accept for this update only. The flag is
            // consumed by the walk that bubbles, not kept for the next one.
            meta.invalidated = true;
        }
    });
    UNDEFINED
}

unsafe extern "C" fn meta_decline(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    let Ok(id) = (unsafe { to_string(cx, *data) }) else {
        return qjs::JS_EXCEPTION;
    };
    with(cx, |h| {
        if let Some(meta) = h.metas.get_mut(&id) {
            meta.declined = true;
        }
    });
    request_reload(cx);
    UNDEFINED
}

// --- values -------------------------------------------------------------------

fn arg(argc: c_int, argv: *mut JsValue, i: usize) -> JsValue {
    if (i as c_int) < argc {
        // SAFETY: QuickJS passes `argc` values.
        unsafe { *argv.add(i) }
    } else {
        UNDEFINED
    }
}

fn is_exception(v: JsValue) -> bool {
    unsafe { qjs::JS_IsException(v) }
}

fn is_undefined(v: JsValue) -> bool {
    unsafe { qjs::JS_IsUndefined(v) }
}

fn is_function(cx: *mut JsContext, v: JsValue) -> bool {
    unsafe { qjs::JS_IsFunction(cx, v) }
}

fn is_array(_cx: *mut JsContext, v: JsValue) -> bool {
    // SAFETY: an inline check.
    unsafe { qjs::JS_IsArray(v) }
}

fn same(a: JsValue, b: JsValue) -> bool {
    // SAFETY: inline tag check, no JS runs.
    unsafe {
        qjs::JS_VALUE_GET_PTR(a) == qjs::JS_VALUE_GET_PTR(b)
            && qjs::JS_VALUE_GET_TAG(a) == qjs::JS_VALUE_GET_TAG(b)
    }
}

fn free(cx: *mut JsContext, v: JsValue) {
    unsafe { qjs::JS_FreeValue(cx, v) };
}

fn to_bool(cx: *mut JsContext, v: JsValue) -> bool {
    // SAFETY: converting a live value (never throws).
    unsafe { qjs::JS_ToBool(cx, v) == 1 }
}

fn to_string(cx: *mut JsContext, v: JsValue) -> Result<String, JsValue> {
    // SAFETY: converts a live value; the result is freed.
    unsafe {
        let s = qjs::JS_ToString(cx, v);
        if qjs::JS_IsException(s) {
            return Err(qjs::JS_EXCEPTION);
        }
        let mut len = 0usize;
        let c = qjs::JS_ToCStringLen(cx, &mut len, s);
        qjs::JS_FreeValue(cx, s);
        if c.is_null() {
            return Err(qjs::JS_EXCEPTION);
        }
        let out = std::str::from_utf8(std::slice::from_raw_parts(c.cast::<u8>(), len))
            .unwrap_or_default()
            .to_owned();
        qjs::JS_FreeCString(cx, c);
        Ok(out)
    }
}

fn new_object(cx: *mut JsContext) -> Result<Owned, Fail> {
    let obj = unsafe { qjs::JS_NewObject(cx) };
    if is_exception(obj) {
        return Err(Fail::Exception);
    }
    Ok(Owned::from_raw(cx, obj))
}

fn new_string(cx: *mut JsContext, text: &str) -> Result<Owned, Fail> {
    let value = unsafe { qjs::JS_NewStringLen(cx, text.as_ptr().cast(), text.len() as _) };
    if is_exception(value) {
        return Err(Fail::Exception);
    }
    Ok(Owned::from_raw(cx, value))
}

fn new_array(cx: *mut JsContext, mut values: Vec<JsValue>) -> JsValue {
    // SAFETY: the array takes the values.
    unsafe { qjs::JS_NewArrayFrom(cx, values.len() as c_int, values.as_mut_ptr()) }
}

fn array_strings(cx: *mut JsContext, array: JsValue) -> Result<Vec<String>, JsValue> {
    let mut len: i64 = 0;
    // SAFETY: reads of a live array.
    if unsafe { qjs::JS_GetLength(cx, array, &mut len) } != 0 {
        return Err(qjs::JS_EXCEPTION);
    }
    let mut out = Vec::with_capacity(len.max(0) as usize);
    for i in 0..len.max(0) as u32 {
        let value = unsafe { qjs::JS_GetPropertyUint32(cx, array, i) };
        if is_exception(value) {
            return Err(qjs::JS_EXCEPTION);
        }
        let text = match to_string(cx, value) {
            Ok(text) => text,
            Err(e) => {
                free(cx, value);
                return Err(e);
            }
        };
        free(cx, value);
        out.push(text);
    }
    Ok(out)
}

fn get(cx: *mut JsContext, obj: JsValue, key: &str) -> Result<Owned, Fail> {
    let name = CString::new(key).unwrap_or_default();
    let value = unsafe { qjs::JS_GetPropertyStr(cx, obj, name.as_ptr()) };
    if is_exception(value) {
        return Err(Fail::Exception);
    }
    Ok(Owned::from_raw(cx, value))
}

fn delete_prop(cx: *mut JsContext, obj: JsValue, key: &str) -> Result<(), Fail> {
    let name = CString::new(key).unwrap_or_default();
    // SAFETY: a new atom, freed below.
    let r = unsafe {
        let atom = qjs::JS_NewAtom(cx, name.as_ptr());
        let r = qjs::JS_DeleteProperty(cx, obj, atom, 0);
        qjs::JS_FreeAtom(cx, atom);
        r
    };
    if r < 0 {
        return Err(Fail::Exception);
    }
    Ok(())
}

fn define_value(
    cx: *mut JsContext,
    obj: JsValue,
    name: &CStr,
    value: JsValue,
    flags: u32,
) -> Result<(), Fail> {
    // SAFETY: the property takes `value`.
    let r = unsafe { qjs::JS_DefinePropertyValueStr(cx, obj, name.as_ptr(), value, flags as i32) };
    if r < 0 {
        return Err(Fail::Exception);
    }
    Ok(())
}

fn call(cx: *mut JsContext, f: JsValue, args: &mut [JsValue]) -> JsValue {
    unsafe { qjs::JS_Call(cx, f, UNDEFINED, args.len() as c_int, args.as_mut_ptr()) }
}

fn throw_type(cx: *mut JsContext, message: &str) -> Thrown {
    let message = CString::new(message).unwrap_or_default();
    // SAFETY: throws on a live context.
    unsafe { qjs::JS_ThrowTypeError(cx, c"%s".as_ptr(), message.as_ptr()) };
    Thrown
}

/// A type error is pending on the context.
struct Thrown;

impl Thrown {
    fn take_exception(self) -> JsValue {
        qjs::JS_EXCEPTION
    }
}

impl From<Thrown> for Fail {
    fn from(_thrown: Thrown) -> Self {
        Fail::Exception
    }
}

fn peek_number(cx: *mut JsContext, obj: JsValue) -> f64 {
    let v = reactive::peek(cx, obj);
    // SAFETY: an inline check and a conversion of a borrowed value.
    unsafe {
        if !qjs::JS_IsNumber(v) {
            return 0.0;
        }
        let mut n = 0.0;
        qjs::JS_ToFloat64(cx, &mut n, v);
        n
    }
}

fn report_exception(cx: *mut JsContext) {
    // SAFETY: an exception is pending; the text is freed.
    unsafe {
        let error = qjs::JS_GetException(cx);
        let text = qjs::JS_ToCStringLen(cx, std::ptr::null_mut(), error);
        if !text.is_null() {
            log::error!("hot: {}", CStr::from_ptr(text).to_string_lossy());
            qjs::JS_FreeCString(cx, text);
        }
        qjs::JS_FreeValue(cx, error);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn relative_ids() {
        assert_eq!(super::resolve("App.js", "./Counter.js"), "Counter.js");
        assert_eq!(super::resolve("a/b.js", "../c.js"), "c.js");
        assert_eq!(super::resolve("a/b.js", "./d/e.js"), "a/d/e.js");
    }
}
