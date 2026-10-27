//! The state of a context's natives: the reactive graph, reached through
//! the context's opaque pointer (natives get the context) and, for class
//! finalizers and GC marks (which get the runtime), through a per-thread
//! map from runtime to state.
//!
//! Natives take short references to one part at a time and never hold one
//! across a call into JS or a free of a JS value: either may re-enter
//! (another native, a finalizer).

use std::{cell::RefCell, ffi::c_void, ptr};

use rquickjs::{Ctx, qjs};
use rustc_hash::FxHashMap;

use crate::{dom::Dom, reactive::Graph};

pub(crate) struct State {
    pub(crate) graph: Graph,
    pub(crate) dom: Dom,
    /// When the VM started, for `performance.now`.
    pub(crate) started: std::time::Instant,
    /// The timers the engine's loop waits on.
    pub(crate) timers: Timers,
    /// The requests `fetch` queued for the engine.
    pub(crate) net: crate::net::Queue,
    /// What `platform.os` reports. Set before the app imports `@nlayer/ui`.
    pub(crate) os: std::cell::Cell<crate::Os>,
}

/// Timers, by the id JS gave them and the instant they are due.
#[derive(Default)]
pub(crate) struct Timers {
    due: std::collections::BTreeMap<(std::time::Instant, u32), ()>,
}

impl Timers {
    /// Makes timer `id` due after `ms` milliseconds (again).
    pub(crate) fn set(&mut self, id: u32, ms: f64) {
        self.clear(id);
        let at = std::time::Instant::now() + std::time::Duration::from_secs_f64(ms / 1000.0);
        self.due.insert((at, id), ());
    }

    /// Forgets timer `id`.
    pub(crate) fn clear(&mut self, id: u32) {
        self.due.retain(|&(_, at), ()| at != id);
    }

    /// How long until the first timer is due.
    pub(crate) fn next(&self) -> Option<std::time::Duration> {
        let (&(at, _), ()) = self.due.first_key_value()?;
        Some(at.saturating_duration_since(std::time::Instant::now()))
    }

    /// The ids due now, removed from the queue (they reschedule themselves
    /// when they repeat).
    pub(crate) fn run_due(&mut self) -> Vec<u32> {
        let now = std::time::Instant::now();
        let mut ids = Vec::new();
        while let Some((&(at, id), ())) = self.due.first_key_value() {
            if at > now {
                break;
            }
            self.due.remove(&(at, id));
            ids.push(id);
        }
        ids
    }
}

thread_local! {
    /// The state of each runtime (one context per runtime), for finalizers.
    static STATES: RefCell<FxHashMap<usize, usize>> = RefCell::default();
}

/// The state of `cx`, set by [`install`] (callers project the part they
/// use, never a reference to the whole).
#[inline]
pub(crate) fn state(cx: *mut qjs::JSContext) -> *mut State {
    // SAFETY: reading the opaque pointer of a live context.
    unsafe { qjs::JS_GetContextOpaque(cx) as *mut State }
}

/// The state of runtime `rt`, if it has one (finalizers may run after
/// [`uninstall`], while the context is freed).
pub(crate) fn of_runtime(rt: *mut qjs::JSRuntime) -> Option<*mut State> {
    STATES.with(|m| m.borrow().get(&(rt as usize)).map(|&p| p as *mut State))
}

/// Makes the state of `cx`: the reactive graph and its classes.
pub(crate) fn install(cx: &Ctx<'_>) {
    let raw = cx.as_raw().as_ptr();
    // SAFETY: registering classes on the context's runtime; the state is
    // freed by `uninstall`.
    unsafe {
        let rt = qjs::JS_GetRuntime(raw);
        let graph = crate::reactive::new_graph(raw);
        let dom = crate::dom::new();
        let state = Box::into_raw(Box::new(State {
            graph,
            dom,
            started: crate::standard::started(),
            timers: crate::state::Timers::default(),
            net: crate::net::Queue::default(),
            os: std::cell::Cell::new(crate::Os::Web),
        }));
        qjs::JS_SetContextOpaque(raw, state as *mut c_void);
        STATES.with(|m| m.borrow_mut().insert(rt as usize, state as usize));
    }
}

/// Frees the state of `cx` and every JS value it holds: before the
/// context goes (QuickJS checks that nothing is left).
pub(crate) fn uninstall(cx: &Ctx<'_>) {
    let raw = cx.as_raw().as_ptr();
    // SAFETY: the state `install` made; no JS runs from here on.
    unsafe {
        let p = qjs::JS_GetContextOpaque(raw) as *mut State;
        if p.is_null() {
            return;
        }
        // Unregistered first: the frees below finalize graph objects, whose
        // finalizers then only free their own data.
        let rt = qjs::JS_GetRuntime(raw);
        STATES.with(|m| m.borrow_mut().remove(&(rt as usize)));
        qjs::JS_SetContextOpaque(raw, ptr::null_mut());
        let mut state = Box::from_raw(p);
        state.dom.free_all(raw);
        crate::reactive::free_all(&mut state.graph, raw);
    }
}
