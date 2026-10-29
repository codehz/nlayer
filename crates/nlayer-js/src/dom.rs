//! The renderer compiled JSX runs on: the natives of `nlayer:rt` (internal:
//! only the bundler's JSX output imports it), `@nlayer/runtime` (`mount`,
//! `context`, `ErrorBoundary`) and `@nlayer/ui` (the host components).
//!
//! A node is a JS object of a native class holding its id and what keeps
//! the tree alive: a parent holds its children's objects and a node holds
//! its event handlers, and the GC mark reports both, so the tree, the
//! handlers and whatever they capture are one heap the QuickJS collector
//! sees. The root view (id 1) is held by the renderer. A node nothing
//! refers to any more is collected and its native view released (a removed
//! node may be inserted again while JS holds it).
//!
//! Every change becomes an [`Op`] pushed straight into the host's op
//! queue. Dynamic children are kept by a render effect whose value is the
//! array of nodes it inserted, reconciled with Solid's `reconcileArrays`
//! against the parent's children, which the renderer mirrors.
//!
//! Re-entrancy: freeing a JS value can finalize a node, whose finalizer
//! updates the renderer (`nodes`, the op queue). The renderer is only ever
//! reached by shared reference with short `RefCell` borrows, none held
//! across a call into JS, an allocation of a JS value or a free of one.

use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, c_int, c_void},
    ptr,
};

use nlayer_core::{Command, EventKind, NodeId, Op, ROOT_ID, Tag};
use rquickjs::{Ctx, Value, qjs};
use rustc_hash::FxHashMap;

use crate::{protocol, reactive, state};

type JsValue = qjs::JSValue;
type JsContext = qjs::JSContext;

const UNDEFINED: JsValue = qjs::JS_UNDEFINED;
/// No parent.
const DETACHED: NodeId = 0;
/// How deep nested arrays and accessors are unwrapped.
const MAX_DEPTH: u32 = 64;

/// The renderer's part of a context's state.
pub(crate) struct Dom {
    /// Changes not yet taken by the engine.
    ops: RefCell<Vec<Op>>,
    /// Native nodes collected since the last take (their finalizers queue
    /// here: the op queue may be borrowed meanwhile).
    releases: RefCell<Vec<NodeId>>,
    /// Problems reported once (styles, props, text that is not shown).
    warnings: RefCell<protocol::Warnings>,
    classes: Classes,
    /// The next id JS gives a node (even: the core gives odd ones).
    next_id: Cell<NodeId>,
    /// Live nodes by `id >> 1` (the root is 0); weak: a node's object owns
    /// its data.
    nodes: RefCell<Vec<*mut NodeData>>,
    /// The root view's object (owned).
    root: Cell<JsValue>,
}

#[derive(Clone, Copy)]
struct Classes {
    node: qjs::JSClassID,
    each: qjs::JSClassID,
    context: qjs::JSClassID,
}

struct NodeData {
    id: NodeId,
    /// A text node.
    text: bool,
    /// An element's tag (`None`: a text node or the root).
    tag: Option<Tag>,
    /// The parent in the mirror ([`DETACHED`]: none).
    parent: NodeId,
    /// The children in order (owned objects).
    children: Vec<JsValue>,
    /// Event handlers (owned).
    handlers: Vec<(EventKind, JsValue)>,
    /// The owner the node was made under: a listener's failure goes to the
    /// error boundary above it.
    owner: (u32, u32),
    /// The node's object (not owned: the data goes with it).
    object: JsValue,
}

/// Registers the renderer's classes and makes the root node; runs once per
/// context, after the state exists.
fn init(cx: &Ctx<'_>) {
    let raw = cx.as_raw().as_ptr();
    // SAFETY: registering classes on the context's runtime; prototypes hold
    // no values of ours.
    let classes = unsafe {
        let rt = qjs::JS_GetRuntime(raw);
        let function_proto = qjs::JS_GetFunctionProto(raw);
        let class = |name: &'static CStr,
                     finalize: qjs::JSClassFinalizer,
                     mark: qjs::JSClassGCMark,
                     call: qjs::JSClassCall| {
            let mut id: qjs::JSClassID = 0;
            qjs::JS_NewClassID(rt, &mut id);
            let def = qjs::JSClassDef {
                class_name: name.as_ptr(),
                finalizer: finalize,
                gc_mark: mark,
                call,
                exotic: ptr::null_mut(),
            };
            qjs::JS_NewClass(rt, id, &def);
            id
        };
        let classes = Classes {
            node: class(c"Node", Some(node_finalize), Some(node_mark), None),
            each: class(c"EachRows", Some(each_finalize), Some(each_mark), None),
            context: class(
                c"Context",
                Some(context_finalize),
                Some(context_mark),
                Some(context_call),
            ),
        };
        // Calls of a context are function calls.
        qjs::JS_SetClassProto(raw, classes.context, qjs::JS_GetFunctionProto(raw));
        qjs::JS_FreeValue(raw, function_proto);
        classes
    };
    // SAFETY: the live state; the borrow ends before `new_node` reads it.
    unsafe { dom_mut(raw).classes = classes };
    let root = new_node(raw, ROOT_ID, false, None);
    // SAFETY: as above.
    unsafe { dom_mut(raw).root.set(root) };
    install_globals(raw);
}

/// Makes the renderer of a fresh context.
pub(crate) fn new() -> Dom {
    Dom {
        ops: RefCell::default(),
        releases: RefCell::default(),
        warnings: RefCell::default(),
        classes: Classes {
            node: 0,
            each: 0,
            context: 0,
        },
        next_id: Cell::new(2),
        nodes: RefCell::default(),
        root: Cell::new(UNDEFINED),
    }
}

impl Dom {
    /// Queues `op`.
    fn push(&self, op: Op) {
        self.ops.borrow_mut().push(op);
    }

    /// Moves the ops queued since the last call into `out`.
    pub(crate) fn take_ops(&self, out: &mut Vec<Op>) {
        out.append(&mut self.ops.borrow_mut());
    }

    fn by_id(&self, id: NodeId) -> *mut NodeData {
        self.nodes
            .borrow()
            .get((id >> 1) as usize)
            .copied()
            .unwrap_or(ptr::null_mut())
    }

    fn warn(&self, message: String) {
        self.warnings.borrow_mut().warn(message);
    }

    /// Frees the values the renderer holds (the state is unregistered).
    pub(crate) fn free_all(&mut self, raw: *mut JsContext) {
        // SAFETY: the renderer's own value; no JS runs from here on.
        unsafe { qjs::JS_FreeValue(raw, self.root.replace(UNDEFINED)) };
    }
}

/// The renderer of `cx`.
#[inline]
fn dom(cx: *mut JsContext) -> &'static Dom {
    // SAFETY: the state `install` made; every borrow is short.
    unsafe { &(*state::state(cx)).dom }
}

/// The renderer of `cx`, mutably (only during installation).
#[allow(clippy::mut_from_ref)]
unsafe fn dom_mut<'a>(cx: *mut JsContext) -> &'a mut Dom {
    // SAFETY: the caller holds the context.
    unsafe { &mut (*state::state(cx)).dom }
}

fn throw_type(cx: *mut JsContext, message: &str) -> JsValue {
    let message = std::ffi::CString::new(message).unwrap_or_default();
    // SAFETY: throws on a live context.
    unsafe { qjs::JS_ThrowTypeError(cx, c"%s".as_ptr(), message.as_ptr()) }
}

fn arg(argc: c_int, argv: *mut JsValue, i: usize) -> JsValue {
    if (i as c_int) < argc {
        // SAFETY: QuickJS passes `argc` values.
        unsafe { *argv.add(i) }
    } else {
        UNDEFINED
    }
}

fn to_string(cx: *mut JsContext, v: JsValue) -> Option<String> {
    // SAFETY: converts a live value; the result is freed.
    unsafe {
        let s = qjs::JS_ToString(cx, v);
        if qjs::JS_IsException(s) {
            return None;
        }
        let mut len = 0usize;
        let c = qjs::JS_ToCStringLen(cx, &mut len, s);
        if c.is_null() {
            return None;
        }
        // By length: a string may hold NUL.
        let out =
            String::from_utf8_lossy(std::slice::from_raw_parts(c.cast::<u8>(), len)).into_owned();
        qjs::JS_FreeCString(cx, c);
        qjs::JS_FreeValue(cx, s);
        Some(out)
    }
}

fn string_arg(cx: *mut JsContext, v: JsValue) -> Option<String> {
    // SAFETY: inline check; a borrowed value read.
    unsafe { qjs::JS_IsString(v) }.then(|| to_string(cx, v))?
}

fn same(a: JsValue, b: JsValue) -> bool {
    // SAFETY: inline tag check, no JS runs.
    unsafe {
        qjs::JS_VALUE_GET_PTR(a) == qjs::JS_VALUE_GET_PTR(b)
            && qjs::JS_VALUE_GET_TAG(a) == qjs::JS_VALUE_GET_TAG(b)
    }
}

fn ptr_of(v: JsValue) -> usize {
    // SAFETY: inline.
    unsafe { qjs::JS_VALUE_GET_PTR(v) as usize }
}

fn is_node(cx: *mut JsContext, v: JsValue) -> bool {
    !node_ptr(cx, v).is_null()
}

/// Node `v`'s data, or null when it is not a node object.
fn node_ptr(cx: *mut JsContext, v: JsValue) -> *mut NodeData {
    let class = dom(cx).classes.node;
    // SAFETY: the opaque of a node object is its boxed data.
    unsafe { qjs::JS_GetOpaque(v, class) as *mut NodeData }
}

/// A new node object holding `data` (boxed) for id `id`.
fn new_node(cx: *mut JsContext, id: NodeId, text: bool, tag: Option<Tag>) -> JsValue {
    let d = dom(cx);
    let p = Box::into_raw(Box::new(NodeData {
        id,
        text,
        tag,
        parent: DETACHED,
        children: Vec::new(),
        handlers: Vec::new(),
        owner: reactive::owner_handle(cx),
        object: UNDEFINED,
    }));
    // SAFETY: a registered class; the opaque is the boxed data, freed by
    // the finalizer.
    let object = unsafe {
        let object = qjs::JS_NewObjectClass(cx, d.classes.node);
        qjs::JS_SetOpaque(object, p as *mut c_void);
        (*p).object = object;
        object
    };
    let mut nodes = d.nodes.borrow_mut();
    let index = (id >> 1) as usize;
    if nodes.len() <= index {
        nodes.resize(index + 1, ptr::null_mut());
    }
    nodes[index] = p;
    drop(nodes);
    object
}

fn next_id(cx: *mut JsContext) -> NodeId {
    let d = dom(cx);
    let id = d.next_id.get();
    d.next_id.set(id + 2);
    id
}

unsafe extern "C" fn node_finalize(rt: *mut qjs::JSRuntime, obj: JsValue) {
    // SAFETY: the opaque of a node object is its boxed data.
    let p = unsafe { qjs::JS_GetOpaque(obj, qjs::JS_GetClassID(obj)) } as *mut NodeData;
    if p.is_null() {
        return;
    }
    let mut data = unsafe { Box::from_raw(p) };
    if let Some(state) = state::of_runtime(rt) {
        // SAFETY: a projection of the live state's renderer.
        let d = unsafe { &(*state).dom };
        let index = (data.id >> 1) as usize;
        if let Some(slot) = d.nodes.borrow_mut().get_mut(index)
            && *slot == p
        {
            *slot = ptr::null_mut();
        }
        if data.id != ROOT_ID {
            // Not the op queue: a finalizer may run while it is borrowed.
            d.releases.borrow_mut().push(data.id);
        }
    }
    for v in data.children.drain(..) {
        // SAFETY: values the node owned.
        unsafe { qjs::JS_FreeValueRT(rt, v) };
    }
    for (_, v) in data.handlers.drain(..) {
        unsafe { qjs::JS_FreeValueRT(rt, v) };
    }
}

unsafe extern "C" fn node_mark(rt: *mut qjs::JSRuntime, obj: JsValue, mark: qjs::JS_MarkFunc) {
    // SAFETY: as in the finalizer; marking reads only.
    let p = unsafe { qjs::JS_GetOpaque(obj, qjs::JS_GetClassID(obj)) } as *const NodeData;
    if p.is_null() {
        return;
    }
    let data = unsafe { &*p };
    for &v in &data.children {
        unsafe { qjs::JS_MarkValue(rt, v, mark) };
    }
    for &(_, v) in &data.handlers {
        unsafe { qjs::JS_MarkValue(rt, v, mark) };
    }
}

// --- The tree mirror ----------------------------------------------------------

/// The node after `child` in its parent (`None`: the last one, or not a
/// child).
fn next_sibling(cx: *mut JsContext, child: JsValue) -> Option<JsValue> {
    let c = node_ptr(cx, child);
    if c.is_null() {
        return None;
    }
    // SAFETY: live node data, read without other references to it.
    let parent = unsafe { (*c).parent };
    let p = dom(cx).by_id(parent);
    if p.is_null() {
        return None;
    }
    let children = unsafe { &(*p).children };
    let i = children.iter().position(|&v| same(v, child))?;
    children.get(i + 1).copied()
}

/// Whether `node` is `of` or one of its ancestors.
fn is_ancestor(cx: *mut JsContext, node: NodeId, of: NodeId) -> bool {
    let d = dom(cx);
    let mut at = of;
    while at != DETACHED {
        if at == node {
            return true;
        }
        let p = d.by_id(at);
        if p.is_null() {
            return false;
        }
        // SAFETY: live node data.
        at = unsafe { (*p).parent };
    }
    false
}

/// Moves node `child` into `parent` before `before` (`None`: at the end).
fn insert_before(
    cx: *mut JsContext,
    parent: JsValue,
    child: JsValue,
    before: Option<JsValue>,
) -> Result<(), JsValue> {
    let (p, c) = (node_ptr(cx, parent), node_ptr(cx, child));
    if p.is_null() || c.is_null() {
        return Err(throw_type(cx, "nlayer: not a node"));
    }
    if before.is_some_and(|b| same(b, child)) {
        return Ok(());
    }
    // Already there: no move.
    // SAFETY: live node data.
    if unsafe { (*c).parent == (*p).id } {
        let next = next_sibling(cx, child);
        let in_place = match (next, before) {
            (None, None) => true,
            (Some(n), Some(b)) => same(n, b),
            _ => false,
        };
        if in_place {
            return Ok(());
        }
    }
    // SAFETY: live node data; `p` and `c` differ (checked below), each
    // reached briefly.
    let (parent_id, child_id) = unsafe { ((*p).id, (*c).id) };
    if is_ancestor(cx, child_id, parent_id) {
        return Err(throw_type(
            cx,
            "nlayer: a node cannot be inserted into itself",
        ));
    }
    // SAFETY: live node data, read briefly.
    if unsafe { (*c).text && (*p).tag != Some(Tag::Text) } {
        let tag = unsafe { (*p).tag };
        dom(cx).warn(format!(
            "a string in {} is not shown: text shows in a <Text>",
            match tag {
                None => "the root".to_owned(),
                Some(tag) => format!("<{tag:?}>"),
            }
        ));
    }
    let d = dom(cx);
    // Out of its current parent (the core moves the view itself).
    let mut held = None;
    let old = unsafe { (*c).parent };
    if old != DETACHED {
        let op = d.by_id(old);
        if !op.is_null() {
            let children = unsafe { &mut (*op).children };
            if let Some(i) = children.iter().position(|&v| same(v, child)) {
                held = Some(children.remove(i));
            }
        }
    }
    let children = unsafe { &mut (*p).children };
    let (pos, before_id) = match before {
        Some(b) => match children.iter().position(|&v| same(v, b)) {
            Some(i) => (i, Some(unsafe { (*node_ptr(cx, b)).id })),
            None => (children.len(), None),
        },
        None => (children.len(), None),
    };
    // SAFETY: a dup the parent owns from now on.
    let owned = held.unwrap_or_else(|| unsafe { qjs::JS_DupValue(cx, child) });
    children.insert(pos, owned);
    unsafe { (*c).parent = parent_id };
    d.push(Op::Insert {
        parent: parent_id,
        child: child_id,
        before: before_id,
    });
    Ok(())
}

/// Removes node `child` from `parent` (nothing when it is not a child).
fn remove_child(cx: *mut JsContext, parent: JsValue, child: JsValue) -> Result<(), JsValue> {
    let (p, c) = (node_ptr(cx, parent), node_ptr(cx, child));
    if p.is_null() || c.is_null() {
        return Ok(());
    }
    // SAFETY: live node data, reached briefly.
    let (parent_id, child_id) = unsafe { ((*p).id, (*c).id) };
    if unsafe { (*c).parent } != parent_id {
        return Ok(());
    }
    let children = unsafe { &mut (*p).children };
    let Some(i) = children.iter().position(|&v| same(v, child)) else {
        return Ok(());
    };
    let owned = children.remove(i);
    unsafe { (*c).parent = DETACHED };
    dom(cx).push(Op::Remove {
        parent: parent_id,
        child: child_id,
    });
    // Last: this may finalize the child.
    unsafe { qjs::JS_FreeValue(cx, owned) };
    Ok(())
}

// --- Elements, text, props -----------------------------------------------------

/// `element(tag)`: a new host element.
unsafe extern "C" fn element(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let Some(name) = to_string(cx, arg(argc, argv, 0)) else {
        return qjs::JS_EXCEPTION;
    };
    let tag = Tag::from_name(&name).unwrap_or_else(|| {
        dom(cx).warn(format!("unknown element <{name}>, rendering it as <view>"));
        Tag::View
    });
    let id = next_id(cx);
    dom(cx).push(Op::Create { id, tag });
    new_node(cx, id, false, Some(tag))
}

/// A new text node showing `text`.
fn text_node(cx: *mut JsContext, text: String) -> JsValue {
    let id = next_id(cx);
    dom(cx).push(Op::CreateText { id, text });
    new_node(cx, id, true, None)
}

/// `text(value)`: a new text node.
unsafe extern "C" fn text(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let Some(s) = to_string(cx, arg(argc, argv, 0)) else {
        return qjs::JS_EXCEPTION;
    };
    text_node(cx, s)
}

/// `append(parent, child)`: a static child at the end.
unsafe extern "C" fn append(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let (parent, child) = (arg(argc, argv, 0), arg(argc, argv, 1));
    let result = if is_node(cx, child) {
        insert_before(cx, parent, child, None)
    } else {
        insert_value(cx, parent, child, None)
    };
    result.err().unwrap_or(UNDEFINED)
}

/// `prop(node, name, value)`.
unsafe extern "C" fn prop(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let Some(name) = to_string(cx, arg(argc, argv, 1)) else {
        return qjs::JS_EXCEPTION;
    };
    set_prop(cx, arg(argc, argv, 0), &name, arg(argc, argv, 2), UNDEFINED)
        .err()
        .unwrap_or(UNDEFINED)
}

/// Sets prop `name` of node `node` to `value` (all borrowed): an event
/// handler, or a host prop the core decodes.
fn set_prop(
    cx: *mut JsContext,
    node: JsValue,
    name: &str,
    value: JsValue,
    prev: JsValue,
) -> Result<(), JsValue> {
    let p = node_ptr(cx, node);
    if p.is_null() {
        return Err(throw_type(cx, "nlayer: not a node"));
    }
    // SAFETY: live node data.
    let id = unsafe { (*p).id };
    if name == "children" {
        return Ok(());
    }
    if let Some(event) = EventKind::from_prop(name) {
        return set_event(cx, p, event, value);
    }
    // SAFETY: a context reference for the decoder, which reads a dup.
    let js_cx = unsafe { Ctx::from_raw(std::ptr::NonNull::new_unchecked(cx)) };
    let v = unsafe { Value::from_raw(js_cx, qjs::JS_DupValue(cx, value)) };
    // Decoding may run JS (getters): no borrow is held meanwhile.
    let mut warnings = protocol::Warnings::deferred();
    // SAFETY: live node data.
    let tag = unsafe { (*p).tag };
    let _ = prev;
    let decoded = protocol::decode_prop(id, tag, name, v, &mut warnings);
    dom(cx).warnings.borrow_mut().merge(warnings);
    match decoded {
        Ok(Some(op)) => {
            dom(cx).push(op);
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(rquickjs::Error::Exception) => Err(qjs::JS_EXCEPTION),
        Err(e) => Err(throw_type(cx, &format!("nlayer: prop `{name}`: {e}"))),
    }
}

/// Sets the handler of `event` on node `p` to `value` (borrowed; not a
/// function: none) and tells the core whether the node listens.
fn set_event(
    cx: *mut JsContext,
    p: *mut NodeData,
    event: EventKind,
    value: JsValue,
) -> Result<(), JsValue> {
    // SAFETY: live node data; an inline check, then a dup the node owns.
    let id = unsafe { (*p).id };
    let handler =
        unsafe { qjs::JS_IsFunction(cx, value) }.then(|| unsafe { qjs::JS_DupValue(cx, value) });
    let handlers = unsafe { &mut (*p).handlers };
    let at = handlers.iter().position(|&(e, _)| e == event);
    let (was, old) = match (at, handler) {
        (Some(i), Some(h)) => (true, Some(std::mem::replace(&mut handlers[i].1, h))),
        (Some(i), None) => (true, Some(handlers.remove(i).1)),
        (None, Some(h)) => {
            handlers.push((event, h));
            (false, None)
        }
        (None, None) => (false, None),
    };
    let now = handler.is_some();
    if was != now {
        dom(cx).push(Op::Listen {
            id,
            event,
            enabled: now,
        });
    }
    if let Some(old) = old {
        unsafe { qjs::JS_FreeValue(cx, old) };
    }
    Ok(())
}

/// `bindProp(node, name, get)`: a prop following `get()`.
unsafe extern "C" fn bind_prop(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let mut data = [arg(argc, argv, 0), arg(argc, argv, 1), arg(argc, argv, 2)];
    // SAFETY: the function keeps dups of `data`.
    let func =
        unsafe { qjs::JS_NewCFunctionData(cx, Some(bind_prop_run), 1, 0, 3, data.as_mut_ptr()) };
    let r = reactive::render_effect(cx, func);
    unsafe { qjs::JS_FreeValue(cx, r) };
    reactive::finish_value(cx, UNDEFINED)
}

/// A run of a bound prop: `prev` in, the new value out (kept by the graph).
unsafe extern "C" fn bind_prop_run(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: three data values (`bind_prop`).
    let (node, name, get) = unsafe { (*data, *data.add(1), *data.add(2)) };
    let prev = arg(argc, argv, 0);
    let next = unsafe { qjs::JS_Call(cx, get, UNDEFINED, 0, ptr::null_mut()) };
    if unsafe { qjs::JS_IsException(next) } {
        return next;
    }
    if unsafe { qjs::JS_IsStrictEqual(cx, prev, next) } {
        return next;
    }
    let Some(name) = to_string(cx, name) else {
        unsafe { qjs::JS_FreeValue(cx, next) };
        return qjs::JS_EXCEPTION;
    };
    match set_prop(cx, node, &name, next, prev) {
        Ok(()) => next,
        Err(e) => {
            unsafe { qjs::JS_FreeValue(cx, next) };
            e
        }
    }
}

/// `spread(node, props)`: every prop of `props` (an object, possibly with
/// getters), followed.
unsafe extern "C" fn spread(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let mut data = [arg(argc, argv, 0), arg(argc, argv, 1)];
    // SAFETY: the function keeps dups of `data`.
    let func =
        unsafe { qjs::JS_NewCFunctionData(cx, Some(spread_run), 1, 0, 2, data.as_mut_ptr()) };
    let r = reactive::render_effect(cx, func);
    unsafe { qjs::JS_FreeValue(cx, r) };
    reactive::finish_value(cx, UNDEFINED)
}

/// A run of a spread: the previous values (an object) in, the new ones
/// out.
unsafe extern "C" fn spread_run(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values (`spread`); every value got is freed.
    unsafe {
        let (node, props) = (*data, *data.add(1));
        let prev = arg(argc, argv, 0);
        let next = qjs::JS_NewObject(cx);
        let mut tab: *mut qjs::JSPropertyEnum = ptr::null_mut();
        let mut len: u32 = 0;
        if !qjs::JS_IsObject(props)
            || qjs::JS_GetOwnPropertyNames(
                cx,
                &mut tab,
                &mut len,
                props,
                (qjs::JS_GPN_STRING_MASK | qjs::JS_GPN_ENUM_ONLY) as c_int,
            ) != 0
        {
            return next;
        }
        let mut failed = None;
        for i in 0..len as usize {
            let atom = (*tab.add(i)).atom;
            let name = qjs::JS_AtomToCStringLen(cx, std::ptr::null_mut(), atom);
            if name.is_null() {
                failed = Some(qjs::JS_EXCEPTION);
                break;
            }
            let name_str = CStr::from_ptr(name).to_string_lossy().into_owned();
            qjs::JS_FreeCString(cx, name);
            if name_str == "children" {
                continue;
            }
            let v = qjs::JS_GetProperty(cx, props, atom);
            if qjs::JS_IsException(v) {
                failed = Some(v);
                break;
            }
            let old = if qjs::JS_IsObject(prev) {
                qjs::JS_GetProperty(cx, prev, atom)
            } else {
                UNDEFINED
            };
            let r = if qjs::JS_IsStrictEqual(cx, old, v) {
                Ok(())
            } else {
                set_prop(cx, node, &name_str, v, old)
            };
            qjs::JS_FreeValue(cx, old);
            qjs::JS_SetProperty(cx, next, atom, v);
            if let Err(e) = r {
                failed = Some(e);
                break;
            }
        }
        qjs::JS_FreePropertyEnum(cx, tab, len);
        match failed {
            Some(e) => {
                qjs::JS_FreeValue(cx, next);
                e
            }
            None => next,
        }
    }
}

// --- Children -----------------------------------------------------------------

/// What a child value renders as.
enum Item {
    Node(JsValue),
    Text(String),
}

/// Collects what `value` (borrowed) renders as into `out` (node values
/// borrowed from `keep`, which owns what calls returned). Functions are
/// called (tracked by the running computation).
fn resolve(
    cx: *mut JsContext,
    value: JsValue,
    out: &mut Vec<Item>,
    keep: &mut Vec<JsValue>,
    depth: u32,
) -> Result<(), JsValue> {
    if depth > MAX_DEPTH {
        return Err(throw_type(cx, "nlayer: children nested too deeply"));
    }
    // SAFETY: inline tag checks and reads of live values; returned values
    // are kept in `keep` (freed by the caller).
    unsafe {
        let tag = qjs::JS_VALUE_GET_TAG(value);
        if tag == qjs::JS_TAG_UNDEFINED || tag == qjs::JS_TAG_NULL || tag == qjs::JS_TAG_BOOL {
            return Ok(());
        }
        if qjs::JS_IsString(value) || qjs::JS_IsNumber(value) || tag == qjs::JS_TAG_BIG_INT {
            let Some(s) = to_string(cx, value) else {
                return Err(qjs::JS_EXCEPTION);
            };
            out.push(Item::Text(s));
            return Ok(());
        }
        if !node_ptr(cx, value).is_null() {
            out.push(Item::Node(value));
            return Ok(());
        }
        if qjs::JS_IsFunction(cx, value) {
            let r = qjs::JS_Call(cx, value, UNDEFINED, 0, ptr::null_mut());
            if qjs::JS_IsException(r) {
                return Err(r);
            }
            keep.push(r);
            return resolve(cx, r, out, keep, depth + 1);
        }
        if qjs::JS_IsArray(value) {
            let mut len: i64 = 0;
            if qjs::JS_GetLength(cx, value, &mut len) != 0 {
                return Err(qjs::JS_EXCEPTION);
            }
            for i in 0..len.max(0) as u32 {
                let item = qjs::JS_GetPropertyUint32(cx, value, i);
                if qjs::JS_IsException(item) {
                    return Err(item);
                }
                keep.push(item);
                resolve(cx, item, out, keep, depth + 1)?;
            }
            return Ok(());
        }
    }
    dom(cx).warn("a child that is not an element, text, array or function is not rendered".into());
    Ok(())
}

/// Turns items into node values (owned): text becomes new text nodes,
/// except a lone text over a lone text node `reuse`, which is updated.
fn materialize(cx: *mut JsContext, items: Vec<Item>, reuse: Option<JsValue>) -> Vec<JsValue> {
    let reuse = reuse.filter(|&n| {
        let p = node_ptr(cx, n);
        // SAFETY: live node data.
        !p.is_null() && unsafe { (*p).text }
    });
    if let (Some(node), [Item::Text(_)]) = (reuse, items.as_slice()) {
        let Some(Item::Text(text)) = items.into_iter().next() else {
            unreachable!()
        };
        // SAFETY: a live text node's data.
        let id = unsafe { (*node_ptr(cx, node)).id };
        dom(cx).push(Op::SetText { id, text });
        return vec![unsafe { qjs::JS_DupValue(cx, node) }];
    }
    let mut nodes = Vec::with_capacity(items.len());
    for item in items {
        match item {
            // SAFETY: a dup of a live node.
            Item::Node(v) => nodes.push(unsafe { qjs::JS_DupValue(cx, v) }),
            Item::Text(text) => nodes.push(text_node(cx, text)),
        }
    }
    nodes
}

fn free_values(cx: *mut JsContext, values: Vec<JsValue>) {
    for v in values {
        // SAFETY: owned values.
        unsafe { qjs::JS_FreeValue(cx, v) };
    }
}

/// Whether `value` holds a function (directly or in nested arrays): its
/// children change over time.
fn is_dynamic(cx: *mut JsContext, value: JsValue, depth: u32) -> bool {
    // SAFETY: inline checks and reads of live values, each freed.
    unsafe {
        if qjs::JS_IsFunction(cx, value) {
            return true;
        }
        if depth > MAX_DEPTH || !qjs::JS_IsArray(value) {
            return false;
        }
        let mut len: i64 = 0;
        if qjs::JS_GetLength(cx, value, &mut len) != 0 {
            qjs::JS_FreeValue(cx, qjs::JS_GetException(cx));
            return false;
        }
        for i in 0..len.max(0) as u32 {
            let item = qjs::JS_GetPropertyUint32(cx, value, i);
            let dynamic = is_dynamic(cx, item, depth + 1);
            qjs::JS_FreeValue(cx, item);
            if dynamic {
                return true;
            }
        }
    }
    false
}

/// Inserts `value` (borrowed) into `parent` before `marker`: once for a
/// static value, kept up to date by a render effect for a dynamic one.
fn insert_value(
    cx: *mut JsContext,
    parent: JsValue,
    value: JsValue,
    marker: Option<JsValue>,
) -> Result<(), JsValue> {
    if is_dynamic(cx, value, 0) {
        // What the insert shows now, which its effects update.
        let shown = new_array(cx, Vec::new());
        let r = if unsafe { qjs::JS_IsFunction(cx, value) } {
            insert_effect(
                cx,
                insert_run,
                [parent, marker.unwrap_or(UNDEFINED), value, shown],
            )
        } else {
            insert_items(cx, parent, marker.unwrap_or(UNDEFINED), value, shown)
        };
        unsafe { qjs::JS_FreeValue(cx, shown) };
        r?;
        // A throw in the effect is the round's error.
        let r = reactive::finish_value(cx, UNDEFINED);
        return if unsafe { qjs::JS_IsException(r) } {
            Err(r)
        } else {
            Ok(())
        };
    }
    let mut items = Vec::new();
    let mut keep = Vec::new();
    let resolved = resolve(cx, value, &mut items, &mut keep, 0);
    let result = resolved.and_then(|()| {
        let nodes = materialize(cx, items, None);
        let mut result = Ok(());
        for &n in &nodes {
            result = insert_before(cx, parent, n, marker);
            if result.is_err() {
                break;
            }
        }
        free_values(cx, nodes);
        result
    });
    free_values(cx, keep);
    result
}

/// A render effect of an insert running `run` with `data` (borrowed).
fn insert_effect(
    cx: *mut JsContext,
    run: unsafe extern "C" fn(
        *mut JsContext,
        JsValue,
        c_int,
        *mut JsValue,
        c_int,
        *mut JsValue,
    ) -> JsValue,
    mut data: [JsValue; 4],
) -> Result<(), JsValue> {
    // SAFETY: the function keeps dups of `data`.
    let func = unsafe { qjs::JS_NewCFunctionData(cx, Some(run), 0, 0, 4, data.as_mut_ptr()) };
    if unsafe { qjs::JS_IsException(func) } {
        return Err(func);
    }
    let r = reactive::render_effect(cx, func);
    unsafe { qjs::JS_FreeValue(cx, r) };
    Ok(())
}

/// The insert of an array holding functions (`items`, borrowed): each
/// function gets a memo of its own, so a change in one runs only that one
/// and the others' nodes stay (not made again), and one render effect
/// shows them all.
fn insert_items(
    cx: *mut JsContext,
    parent: JsValue,
    marker: JsValue,
    items: JsValue,
    shown: JsValue,
) -> Result<(), JsValue> {
    let memos = memoize_run(cx, items);
    let r = insert_effect(
        cx,
        insert_items_run,
        [parent, marker, memos.unwrap_or(items), shown],
    );
    if let Some(memos) = memos {
        unsafe { qjs::JS_FreeValue(cx, memos) };
    }
    r
}

/// For an array of children holding functions, a copy with a memo for each
/// function (owned), made under the running owner; `None` otherwise.
fn memoize_run(cx: *mut JsContext, value: JsValue) -> Option<JsValue> {
    // SAFETY: reads of a live value.
    if unsafe { !qjs::JS_IsArray(value) } {
        return None;
    }
    let items = array_items(cx, value);
    // SAFETY: borrowed values checked.
    let functions = items
        .iter()
        .filter(|&&v| unsafe { qjs::JS_IsFunction(cx, v) } && node_ptr(cx, v).is_null())
        .count();
    if items.len() < 2 || functions == 0 {
        free_values(cx, items);
        return None;
    }
    let items = items
        .into_iter()
        .map(|v| {
            // SAFETY: a borrowed check of an owned value.
            if unsafe { qjs::JS_IsFunction(cx, v) } && node_ptr(cx, v).is_null() {
                item_memo(cx, v)
            } else {
                v
            }
        })
        .collect();
    Some(new_array(cx, items))
}

/// A memo of `func` (owned), one function of an array of children, made
/// under the running owner (owned; an exception value when it throws).
fn item_memo(cx: *mut JsContext, func: JsValue) -> JsValue {
    let mut data = [func];
    // SAFETY: the function keeps a dup of `func`, which is then freed.
    let run = unsafe { qjs::JS_NewCFunctionData(cx, Some(item_run), 0, 0, 1, data.as_mut_ptr()) };
    unsafe { qjs::JS_FreeValue(cx, func) };
    if unsafe { qjs::JS_IsException(run) } {
        return run;
    }
    // The memo takes the function.
    reactive::memo(cx, run, false)
}

/// A run of an [`item_memo`]: calls its function, and gives a function it
/// returns (an `<If>` whose branch is an expression returns one) a memo of
/// its own, under this one.
unsafe extern "C" fn item_run(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: one data value (`item_memo`).
    let r = unsafe { qjs::JS_Call(cx, *data, UNDEFINED, 0, ptr::null_mut()) };
    // SAFETY: inline checks of a live value.
    if unsafe { !qjs::JS_IsException(r) && qjs::JS_IsFunction(cx, r) } && node_ptr(cx, r).is_null()
    {
        return item_memo(cx, r);
    }
    r
}

/// `insert(parent, value, marker)`: children before `marker` (or at the
/// end).
unsafe extern "C" fn insert(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let marker = arg(argc, argv, 2);
    let marker = is_node(cx, marker).then_some(marker);
    insert_value(cx, arg(argc, argv, 0), arg(argc, argv, 1), marker)
        .err()
        .unwrap_or(UNDEFINED)
}

/// The values of array `array` (owned), empty for other values.
fn array_items(cx: *mut JsContext, array: JsValue) -> Vec<JsValue> {
    let mut out = Vec::new();
    // SAFETY: reads of a live array.
    unsafe {
        if !qjs::JS_IsArray(array) {
            return out;
        }
        let mut len: i64 = 0;
        if qjs::JS_GetLength(cx, array, &mut len) != 0 {
            qjs::JS_FreeValue(cx, qjs::JS_GetException(cx));
            return out;
        }
        for i in 0..len.max(0) as u32 {
            out.push(qjs::JS_GetPropertyUint32(cx, array, i));
        }
    }
    out
}

/// A new array of `values` (taken).
fn new_array(cx: *mut JsContext, mut values: Vec<JsValue>) -> JsValue {
    // SAFETY: the array takes the values.
    unsafe { qjs::JS_NewArrayFrom(cx, values.len() as c_int, values.as_mut_ptr()) }
}

/// A run of a dynamic insert of a function: calls it, tracked by this
/// effect. A function it returns (what a component whose root is an
/// `<If>` returns) gets a render effect of its own under this one.
unsafe extern "C" fn insert_run(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: four data values (`insert_effect`).
    let (parent, marker, value, shown) =
        unsafe { (*data, *data.add(1), *data.add(2), *data.add(3)) };
    // SAFETY: a dup of a live value; the call's result replaces it.
    let mut value = unsafe { qjs::JS_DupValue(cx, value) };
    if unsafe { qjs::JS_IsFunction(cx, value) } && node_ptr(cx, value).is_null() {
        let r = unsafe { qjs::JS_Call(cx, value, UNDEFINED, 0, ptr::null_mut()) };
        unsafe { qjs::JS_FreeValue(cx, value) };
        if unsafe { qjs::JS_IsException(r) } {
            return r;
        }
        value = r;
        if unsafe { qjs::JS_IsFunction(cx, value) } && node_ptr(cx, value).is_null() {
            // The nested effect shows what the function gives, in this
            // insert's place; it goes when this effect runs again.
            let nested = insert_effect(cx, insert_run, [parent, marker, value, shown]);
            unsafe { qjs::JS_FreeValue(cx, value) };
            return nested.err().unwrap_or(UNDEFINED);
        }
    }
    let r = if unsafe { qjs::JS_IsArray(value) } && is_dynamic(cx, value, 0) {
        insert_items(cx, parent, marker, value, shown)
    } else {
        show(cx, parent, marker, value, shown)
    };
    unsafe { qjs::JS_FreeValue(cx, value) };
    r.err().unwrap_or(UNDEFINED)
}

/// A run of the effect of [`insert_items`]: shows the array, its functions
/// called (tracked by this effect).
unsafe extern "C" fn insert_items_run(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: four data values (`insert_effect`).
    let (parent, marker, items, shown) =
        unsafe { (*data, *data.add(1), *data.add(2), *data.add(3)) };
    show(cx, parent, marker, items, shown)
        .err()
        .unwrap_or(UNDEFINED)
}

/// Shows `value` (borrowed; its functions are called) in `parent` before
/// `marker` in place of the nodes `shown` holds, which then holds the new
/// ones.
fn show(
    cx: *mut JsContext,
    parent: JsValue,
    marker: JsValue,
    value: JsValue,
    shown: JsValue,
) -> Result<(), JsValue> {
    let marker = is_node(cx, marker).then_some(marker);
    let mut items = Vec::new();
    let mut keep = Vec::new();
    if let Err(e) = resolve(cx, value, &mut items, &mut keep, 0) {
        free_values(cx, keep);
        return Err(e);
    }
    let old = array_items(cx, shown);
    // A lone text node shown again as text is updated in place.
    let reuse = match old.as_slice() {
        [only] => Some(*only),
        _ => None,
    };
    let lone_text = matches!(items.as_slice(), [Item::Text(_)]);
    let new = materialize(cx, items, reuse.filter(|_| lone_text));
    free_values(cx, keep);
    let result = reconcile(cx, parent, &old, &new, marker);
    // The old nodes are freed after the reconcile: a node it removes stays
    // alive until then.
    free_values(cx, old);
    match result {
        Ok(()) => {
            set_shown(cx, shown, new);
            Ok(())
        }
        Err(e) => {
            free_values(cx, new);
            Err(e)
        }
    }
}

/// Makes array `shown` hold `nodes` (taken) alone.
fn set_shown(cx: *mut JsContext, shown: JsValue, nodes: Vec<JsValue>) {
    // SAFETY: writes to a live array, which takes the values.
    unsafe {
        let len = qjs::JS_NewFloat64(0.0);
        qjs::JS_SetPropertyStr(cx, shown, c"length".as_ptr(), len);
        for (i, node) in nodes.into_iter().enumerate() {
            qjs::JS_SetPropertyUint32(cx, shown, i as u32, node);
        }
    }
}

/// Solid's `reconcileArrays`: changes `parent`'s children from `old` to
/// `new` (the nodes of one insert, before `marker`) with few moves.
fn reconcile(
    cx: *mut JsContext,
    parent: JsValue,
    old: &[JsValue],
    new: &[JsValue],
    marker: Option<JsValue>,
) -> Result<(), JsValue> {
    let mut a: Vec<JsValue> = old.to_vec();
    let b = new;
    let b_len = b.len();
    let (mut a_start, mut b_start) = (0usize, 0usize);
    let (mut a_end, mut b_end) = (a.len(), b_len);
    let after = match a.last() {
        Some(&last) => next_sibling(cx, last).or(marker),
        None => marker,
    };
    let mut map: Option<FxHashMap<usize, usize>> = None;
    while a_start < a_end || b_start < b_end {
        if a_start < a_end && b_start < b_end && same(a[a_start], b[b_start]) {
            a_start += 1;
            b_start += 1;
            continue;
        }
        while a_end > a_start && b_end > b_start && same(a[a_end - 1], b[b_end - 1]) {
            a_end -= 1;
            b_end -= 1;
        }
        if a_end == a_start {
            let node = if b_end < b_len { Some(b[b_end]) } else { after };
            while b_start < b_end {
                insert_before(cx, parent, b[b_start], node)?;
                b_start += 1;
            }
        } else if b_end == b_start {
            while a_start < a_end {
                if map
                    .as_ref()
                    .is_none_or(|m| !m.contains_key(&ptr_of(a[a_start])))
                {
                    remove_child(cx, parent, a[a_start])?;
                }
                a_start += 1;
            }
        } else if same(a[a_start], b[b_end - 1]) && same(b[b_start], a[a_end - 1]) {
            a_end -= 1;
            let node = next_sibling(cx, a[a_end]);
            let first_next = next_sibling(cx, a[a_start]);
            insert_before(cx, parent, b[b_start], first_next)?;
            b_start += 1;
            a_start += 1;
            b_end -= 1;
            insert_before(cx, parent, b[b_end], node)?;
            a[a_end] = b[b_end];
        } else {
            let map =
                map.get_or_insert_with(|| (b_start..b_end).map(|i| (ptr_of(b[i]), i)).collect());
            match map.get(&ptr_of(a[a_start])).copied() {
                Some(index) if b_start < index && index < b_end => {
                    let mut i = a_start;
                    let mut sequence = 1;
                    loop {
                        i += 1;
                        if !(i < a_end && i < b_end) {
                            break;
                        }
                        match map.get(&ptr_of(a[i])) {
                            Some(&t) if t == index + sequence => sequence += 1,
                            _ => break,
                        }
                    }
                    if sequence > index - b_start {
                        let node = a[a_start];
                        while b_start < index {
                            insert_before(cx, parent, b[b_start], Some(node))?;
                            b_start += 1;
                        }
                    } else {
                        insert_before(cx, parent, b[b_start], Some(a[a_start]))?;
                        if !map.contains_key(&ptr_of(a[a_start])) {
                            remove_child(cx, parent, a[a_start])?;
                        }
                        b_start += 1;
                        a_start += 1;
                    }
                }
                Some(_) => a_start += 1,
                None => {
                    remove_child(cx, parent, a[a_start])?;
                    a_start += 1;
                }
            }
        }
    }
    Ok(())
}

// --- Components and control flow ------------------------------------------------

/// `merge(...sources)`: a new object with the own enumerable properties of
/// each source, later ones winning; getters stay getters.
unsafe extern "C" fn merge(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    // SAFETY: property reads and definitions on live objects; descriptor
    // values are freed after use.
    unsafe {
        let target = qjs::JS_NewObject(cx);
        for i in 0..argc.max(0) as usize {
            let source = *argv.add(i);
            if !qjs::JS_IsObject(source) {
                continue;
            }
            let mut tab: *mut qjs::JSPropertyEnum = ptr::null_mut();
            let mut len: u32 = 0;
            if qjs::JS_GetOwnPropertyNames(
                cx,
                &mut tab,
                &mut len,
                source,
                (qjs::JS_GPN_STRING_MASK | qjs::JS_GPN_SYMBOL_MASK | qjs::JS_GPN_ENUM_ONLY)
                    as c_int,
            ) != 0
            {
                qjs::JS_FreeValue(cx, target);
                return qjs::JS_EXCEPTION;
            }
            for j in 0..len as usize {
                copy_property(cx, target, source, (*tab.add(j)).atom);
            }
            qjs::JS_FreePropertyEnum(cx, tab, len);
        }
        target
    }
}

/// Defines `source`'s own property `atom` on `target` as it is: a getter
/// stays a getter (props keep following what they read).
///
/// # Safety
///
/// Live objects and atom.
unsafe fn copy_property(cx: *mut JsContext, target: JsValue, source: JsValue, atom: qjs::JSAtom) {
    // SAFETY: per the contract; descriptor values are freed after use.
    unsafe {
        let mut desc = qjs::JSPropertyDescriptor {
            flags: 0,
            value: UNDEFINED,
            getter: UNDEFINED,
            setter: UNDEFINED,
        };
        if qjs::JS_GetOwnProperty(cx, &mut desc, source, atom) <= 0 {
            return;
        }
        let flags = if desc.flags & qjs::JS_PROP_GETSET as c_int != 0 {
            qjs::JS_PROP_HAS_GET | qjs::JS_PROP_HAS_SET
        } else {
            qjs::JS_PROP_HAS_VALUE | qjs::JS_PROP_HAS_WRITABLE | qjs::JS_PROP_WRITABLE
        } | qjs::JS_PROP_HAS_ENUMERABLE
            | qjs::JS_PROP_ENUMERABLE
            | qjs::JS_PROP_HAS_CONFIGURABLE
            | qjs::JS_PROP_CONFIGURABLE;
        qjs::JS_DefineProperty(
            cx,
            target,
            atom,
            desc.value,
            desc.getter,
            desc.setter,
            flags as c_int,
        );
        qjs::JS_FreeValue(cx, desc.value);
        qjs::JS_FreeValue(cx, desc.getter);
        qjs::JS_FreeValue(cx, desc.setter);
    }
}

/// `component(Comp, props)`: renders a component (untracked).
unsafe extern "C" fn component(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let (comp, props) = (arg(argc, argv, 0), arg(argc, argv, 1));
    let mut args = [props];
    // SAFETY: a borrowed function and argument.
    reactive::untracked_with(cx, || unsafe {
        qjs::JS_Call(cx, comp, UNDEFINED, 1, args.as_mut_ptr())
    })
}

/// `cond(when, then, otherwise)`: what `then()` renders while `when()` is
/// truthy, else what `otherwise()` renders.
unsafe extern "C" fn cond(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    // SAFETY: dups the memos and functions own.
    let when = unsafe { qjs::JS_DupValue(cx, arg(argc, argv, 0)) };
    let condition = reactive::memo(cx, when, true);
    let mut data = [condition, arg(argc, argv, 1), arg(argc, argv, 2)];
    let func = unsafe { qjs::JS_NewCFunctionData(cx, Some(cond_run), 0, 0, 3, data.as_mut_ptr()) };
    unsafe { qjs::JS_FreeValue(cx, condition) };
    let branch = reactive::memo(cx, func, false);
    reactive::finish_value(cx, branch)
}

unsafe extern "C" fn cond_run(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: three data values (`cond`).
    let (condition, then, otherwise) = unsafe { (*data, *data.add(1), *data.add(2)) };
    let c = unsafe { qjs::JS_Call(cx, condition, UNDEFINED, 0, ptr::null_mut()) };
    if unsafe { qjs::JS_IsException(c) } {
        return c;
    }
    let truthy = unsafe { qjs::JS_ToBool(cx, c) } == 1;
    unsafe { qjs::JS_FreeValue(cx, c) };
    let branch = if truthy { then } else { otherwise };
    if !unsafe { qjs::JS_IsFunction(cx, branch) } {
        return UNDEFINED;
    }
    reactive::untracked_with(cx, || unsafe {
        qjs::JS_Call(cx, branch, UNDEFINED, 0, ptr::null_mut())
    })
}

/// A key rows are matched by: objects by identity, primitives by value.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Object(usize),
    Text(Box<str>),
    Number(u64),
    Other(i32, i64),
}

impl Key {
    fn of(cx: *mut JsContext, v: JsValue) -> Key {
        // SAFETY: inline reads of a live value.
        unsafe {
            let tag = qjs::JS_VALUE_GET_TAG(v);
            if tag == qjs::JS_TAG_OBJECT {
                Key::Object(ptr_of(v))
            } else if qjs::JS_IsString(v) {
                Key::Text(to_string(cx, v).unwrap_or_default().into())
            } else if qjs::JS_IsNumber(v) {
                let mut n = 0f64;
                qjs::JS_ToFloat64(cx, &mut n, v);
                // SameValueZero: -0 is 0, NaN is NaN.
                let n = if n == 0.0 {
                    0.0
                } else if n.is_nan() {
                    f64::NAN
                } else {
                    n
                };
                Key::Number(n.to_bits())
            } else {
                Key::Other(tag, qjs::JS_VALUE_GET_INT(v) as i64)
            }
        }
    }
}

/// A row of `each`.
struct Row {
    key: Key,
    /// The key value (owned: an object key stays alive while it names the
    /// row).
    key_value: JsValue,
    /// Signals of the row's item and index (owned).
    item: JsValue,
    index: JsValue,
    /// The row's owner.
    owner: u32,
    /// What the row rendered (owned).
    output: JsValue,
}

#[derive(Default)]
struct EachData {
    rows: Vec<Row>,
}

unsafe extern "C" fn each_finalize(rt: *mut qjs::JSRuntime, obj: JsValue) {
    // SAFETY: the opaque of an each object is its boxed data.
    let p = unsafe { qjs::JS_GetOpaque(obj, qjs::JS_GetClassID(obj)) } as *mut EachData;
    if p.is_null() {
        return;
    }
    let data = unsafe { Box::from_raw(p) };
    for row in data.rows {
        for v in [row.key_value, row.item, row.index, row.output] {
            unsafe { qjs::JS_FreeValueRT(rt, v) };
        }
    }
}

unsafe extern "C" fn each_mark(rt: *mut qjs::JSRuntime, obj: JsValue, mark: qjs::JS_MarkFunc) {
    // SAFETY: as in the finalizer; marking reads only.
    let p = unsafe { qjs::JS_GetOpaque(obj, qjs::JS_GetClassID(obj)) } as *const EachData;
    if p.is_null() {
        return;
    }
    for row in unsafe { &(*p).rows } {
        for v in [row.key_value, row.item, row.index, row.output] {
            unsafe { qjs::JS_MarkValue(rt, v, mark) };
        }
    }
}

/// `each(list, render, key)`: `render(item, index)` for each item of
/// `list()` (both accessors), one row per key (`key(item, index)`, the
/// index for `key` `"index"`, or the item itself) kept while the key
/// stays.
unsafe extern "C" fn each(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    // SAFETY: a new object of the each class owning its boxed data.
    let rows = unsafe {
        let obj = qjs::JS_NewObjectClass(cx, dom(cx).classes.each as _);
        qjs::JS_SetOpaque(
            obj,
            Box::into_raw(Box::<EachData>::default()) as *mut c_void,
        );
        obj
    };
    // The rows go with the owner `each` is called under.
    let mut cleanup_data = [rows];
    let cleanup = unsafe {
        qjs::JS_NewCFunctionData(cx, Some(each_dispose), 0, 0, 1, cleanup_data.as_mut_ptr())
    };
    let mut args = [cleanup];
    let r = unsafe { reactive::on_cleanup(cx, UNDEFINED, 1, args.as_mut_ptr()) };
    unsafe {
        qjs::JS_FreeValue(cx, r);
        qjs::JS_FreeValue(cx, cleanup);
    }
    let mut data = [
        arg(argc, argv, 0),
        arg(argc, argv, 1),
        arg(argc, argv, 2),
        rows,
    ];
    let func = unsafe { qjs::JS_NewCFunctionData(cx, Some(each_run), 0, 0, 4, data.as_mut_ptr()) };
    unsafe { qjs::JS_FreeValue(cx, rows) };
    let memo = reactive::memo(cx, func, false);
    reactive::finish_value(cx, memo)
}

fn each_data(cx: *mut JsContext, rows: JsValue) -> *mut EachData {
    // SAFETY: the opaque of an each object.
    unsafe { qjs::JS_GetOpaque(rows, dom(cx).classes.each) as *mut EachData }
}

/// Disposes a row (taken).
fn dispose_row(cx: *mut JsContext, row: Row) {
    reactive::dispose_owner(cx, row.owner);
    for v in [row.key_value, row.item, row.index, row.output] {
        // SAFETY: values the row owned.
        unsafe { qjs::JS_FreeValue(cx, v) };
    }
}

unsafe extern "C" fn each_dispose(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: one data value (`each`).
    let p = each_data(cx, unsafe { *data });
    if !p.is_null() {
        let rows = std::mem::take(unsafe { &mut (*p).rows });
        for row in rows {
            dispose_row(cx, row);
        }
    }
    UNDEFINED
}

unsafe extern "C" fn each_run(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: four data values (`each`).
    let (list, render, key_fn, rows) = unsafe { (*data, *data.add(1), *data.add(2), *data.add(3)) };
    let items = unsafe { qjs::JS_Call(cx, list, UNDEFINED, 0, ptr::null_mut()) };
    if unsafe { qjs::JS_IsException(items) } {
        return items;
    }
    let items_v = array_items(cx, items);
    unsafe { qjs::JS_FreeValue(cx, items) };
    let keyed = unsafe { qjs::JS_IsFunction(cx, key_fn) };
    // SAFETY: a borrowed value, read when it is a string.
    let by_index = !keyed
        && unsafe { qjs::JS_IsString(key_fn) }
        && to_string(cx, key_fn).as_deref() == Some("index");
    let result = reactive::untracked_with(cx, || {
        let p = each_data(cx, rows);
        let old = std::mem::take(unsafe { &mut (*p).rows });
        let mut old: Vec<Option<Row>> = old.into_iter().map(Some).collect();
        let mut by_key: FxHashMap<Key, Vec<usize>> = FxHashMap::default();
        for (i, row) in old.iter().enumerate().rev() {
            let row = row.as_ref().expect("all present");
            by_key.entry(row.key.clone()).or_default().push(i);
        }
        let mut new_rows = Vec::with_capacity(items_v.len());
        let mut failed = None;
        for (i, &item) in items_v.iter().enumerate() {
            // SAFETY: calls of borrowed functions with live values; the
            // values got are owned by the row or freed.
            let key_value = if by_index {
                qjs::JS_NewFloat64(i as f64)
            } else if keyed {
                let mut args = [item, qjs::JS_NewFloat64(i as f64)];
                let k = unsafe { qjs::JS_Call(cx, key_fn, UNDEFINED, 2, args.as_mut_ptr()) };
                if unsafe { qjs::JS_IsException(k) } {
                    failed = Some(k);
                    break;
                }
                k
            } else {
                unsafe { qjs::JS_DupValue(cx, item) }
            };
            let key = Key::of(cx, key_value);
            let reused = by_key
                .get_mut(&key)
                .and_then(|v| v.pop())
                .and_then(|at| old[at].take());
            match reused {
                Some(row) => {
                    unsafe { qjs::JS_FreeValue(cx, key_value) };
                    if !unsafe { qjs::JS_IsStrictEqual(cx, reactive::peek(cx, row.item), item) } {
                        reactive::set_signal(cx, row.item, unsafe { qjs::JS_DupValue(cx, item) });
                    }
                    let index = qjs::JS_NewFloat64(i as f64);
                    if !unsafe { qjs::JS_IsStrictEqual(cx, reactive::peek(cx, row.index), index) } {
                        reactive::set_signal(cx, row.index, index);
                    }
                    new_rows.push(row);
                }
                None => {
                    let item_signal = reactive::new_signal(cx, item);
                    let index_signal = reactive::new_signal(cx, qjs::JS_NewFloat64(i as f64));
                    let (owner, output) = reactive::with_root(cx, || {
                        let mut args = [item_signal, index_signal];
                        unsafe { qjs::JS_Call(cx, render, UNDEFINED, 2, args.as_mut_ptr()) }
                    });
                    let row = Row {
                        key,
                        key_value,
                        item: item_signal,
                        index: index_signal,
                        owner,
                        output: UNDEFINED,
                    };
                    if unsafe { qjs::JS_IsException(output) } {
                        dispose_row(cx, row);
                        failed = Some(output);
                        break;
                    }
                    new_rows.push(Row { output, ..row });
                }
            }
        }
        for row in old.into_iter().flatten() {
            dispose_row(cx, row);
        }
        let outputs: Vec<JsValue> = new_rows
            .iter()
            // SAFETY: dups for the returned array.
            .map(|r| unsafe { qjs::JS_DupValue(cx, r.output) })
            .collect();
        let p = each_data(cx, rows);
        unsafe { (*p).rows = new_rows };
        match failed {
            Some(e) => {
                free_values(cx, outputs);
                e
            }
            None => new_array(cx, outputs),
        }
    });
    free_values(cx, items_v);
    result
}

// --- Context ------------------------------------------------------------------

/// A context's default value (owned).
struct ContextData {
    default: JsValue,
}

unsafe extern "C" fn context_finalize(rt: *mut qjs::JSRuntime, obj: JsValue) {
    // SAFETY: the opaque of a context object is its boxed data.
    let p = unsafe { qjs::JS_GetOpaque(obj, qjs::JS_GetClassID(obj)) } as *mut ContextData;
    if !p.is_null() {
        let data = unsafe { Box::from_raw(p) };
        unsafe { qjs::JS_FreeValueRT(rt, data.default) };
    }
}

unsafe extern "C" fn context_mark(rt: *mut qjs::JSRuntime, obj: JsValue, mark: qjs::JS_MarkFunc) {
    // SAFETY: as in the finalizer.
    let p = unsafe { qjs::JS_GetOpaque(obj, qjs::JS_GetClassID(obj)) } as *const ContextData;
    if !p.is_null() {
        unsafe { qjs::JS_MarkValue(rt, (*p).default, mark) };
    }
}

/// Calling a context: its nearest provided value, else the default.
unsafe extern "C" fn context_call(
    cx: *mut JsContext,
    func: JsValue,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _flags: c_int,
) -> JsValue {
    if let Some(v) = reactive::lookup(cx, func) {
        return v;
    }
    // SAFETY: the opaque of a context object.
    let p = unsafe { qjs::JS_GetOpaque(func, dom(cx).classes.context) } as *const ContextData;
    if p.is_null() {
        return UNDEFINED;
    }
    unsafe { qjs::JS_DupValue(cx, (*p).default) }
}

/// `context(default)`: a context, read by calling it; `<Ctx.Provider
/// value={…}>` provides a value to what it renders.
pub(crate) unsafe extern "C" fn context(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    // SAFETY: a new object of the context class owning its boxed data; the
    // provider function keeps a dup of it.
    unsafe {
        let obj = qjs::JS_NewObjectClass(cx, dom(cx).classes.context as _);
        let data = Box::new(ContextData {
            default: qjs::JS_DupValue(cx, arg(argc, argv, 0)),
        });
        qjs::JS_SetOpaque(obj, Box::into_raw(data) as *mut c_void);
        let mut fdata = [obj];
        let provider = qjs::JS_NewCFunctionData(cx, Some(provider), 1, 0, 1, fdata.as_mut_ptr());
        qjs::JS_SetPropertyStr(cx, obj, c"Provider".as_ptr(), provider);
        obj
    }
}

/// A provider component: `{ value, children }`.
unsafe extern "C" fn provider(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: one data value (`context`); the function keeps dups.
    let mut fdata = [unsafe { *data }, arg(argc, argv, 0)];
    let func =
        unsafe { qjs::JS_NewCFunctionData(cx, Some(provider_run), 0, 0, 2, fdata.as_mut_ptr()) };
    let rendered = reactive::render_effect(cx, func);
    reactive::finish_value(cx, rendered)
}

/// The provider's owner: sets the value, then renders the children in a
/// memo under it (so functions among them run with the value in reach).
unsafe extern "C" fn provider_run(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values (`provider`); values got are freed.
    let (key, props) = unsafe { (*data, *data.add(1)) };
    reactive::untracked_with(cx, || unsafe {
        let value = qjs::JS_GetPropertyStr(cx, props, c"value".as_ptr());
        if qjs::JS_IsException(value) {
            return value;
        }
        reactive::provide(cx, key, value);
        qjs::JS_FreeValue(cx, value);
        let children = qjs::JS_GetPropertyStr(cx, props, c"children".as_ptr());
        if qjs::JS_IsException(children) {
            return children;
        }
        let mut fdata = [children];
        let func = qjs::JS_NewCFunctionData(cx, Some(children_run), 0, 0, 1, fdata.as_mut_ptr());
        qjs::JS_FreeValue(cx, children);
        reactive::memo(cx, func, false)
    })
}

/// Unwraps children: functions called (tracked), arrays flattened.
unsafe extern "C" fn children_run(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: one data value.
    let children = unsafe { *data };
    let mut items = Vec::new();
    let mut keep = Vec::new();
    let resolved = resolve(cx, children, &mut items, &mut keep, 0);
    // Text stays text (the insert makes its nodes); nodes are kept.
    let out = resolved.map(|()| {
        items
            .into_iter()
            .map(|item| match item {
                // SAFETY: a dup for the array.
                Item::Node(v) => unsafe { qjs::JS_DupValue(cx, v) },
                Item::Text(t) => unsafe {
                    qjs::JS_NewStringLen(cx, t.as_ptr().cast(), t.len() as _)
                },
            })
            .collect::<Vec<_>>()
    });
    free_values(cx, keep);
    match out {
        Ok(values) => new_array(cx, values),
        Err(e) => e,
    }
}

// --- Mounting -----------------------------------------------------------------

/// `mount(App, props)`: renders `App` into the surface; returns a function
/// that removes it.
pub(crate) unsafe extern "C" fn mount(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let (app, props) = (arg(argc, argv, 0), arg(argc, argv, 1));
    let root = dom(cx).root.get();
    // SAFETY: reads of the root's children; dups for the dispose function.
    let before: Vec<JsValue> = unsafe { &(*node_ptr(cx, root)).children }
        .iter()
        .map(|&v| unsafe { qjs::JS_DupValue(cx, v) })
        .collect();
    let (owner, result) = reactive::with_root(cx, || {
        let props = if unsafe { qjs::JS_IsObject(props) } {
            unsafe { qjs::JS_DupValue(cx, props) }
        } else {
            unsafe { qjs::JS_NewObject(cx) }
        };
        let mut args = [props];
        let out = reactive::untracked_with(cx, || unsafe {
            qjs::JS_Call(cx, app, UNDEFINED, 1, args.as_mut_ptr())
        });
        unsafe { qjs::JS_FreeValue(cx, props) };
        if unsafe { qjs::JS_IsException(out) } {
            return Err(out);
        }
        let r = insert_value(cx, root, out, None);
        unsafe { qjs::JS_FreeValue(cx, out) };
        r
    });
    if let Err(e) = result {
        free_values(cx, before);
        reactive::dispose_owner(cx, owner);
        return e;
    }
    let mut data = [qjs::JS_NewFloat64(owner as f64), new_array(cx, before)];
    // SAFETY: the function keeps dups of `data`.
    let dispose =
        unsafe { qjs::JS_NewCFunctionData(cx, Some(unmount), 0, 0, 2, data.as_mut_ptr()) };
    unsafe { qjs::JS_FreeValue(cx, data[1]) };
    reactive::finish_value(cx, dispose)
}

/// `Keyboard.dismiss()`: resign the focused field. The platform hides the
/// software keyboard. It does not depend on the keyboard inset.
pub(crate) unsafe extern "C" fn dismiss_keyboard(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
) -> JsValue {
    dom(cx).push(Op::Command {
        id: ROOT_ID,
        command: Command::Blur,
    });
    UNDEFINED
}

unsafe extern "C" fn unmount(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values (`mount`).
    let (owner, before) = unsafe { (*data, *data.add(1)) };
    let mut id = 0f64;
    unsafe { qjs::JS_ToFloat64(cx, &mut id, owner) };
    reactive::dispose_owner(cx, id as u32);
    let before = array_items(cx, before);
    let root = dom(cx).root.get();
    let current: Vec<JsValue> = unsafe { &(*node_ptr(cx, root)).children }
        .iter()
        .map(|&v| unsafe { qjs::JS_DupValue(cx, v) })
        .collect();
    let mut result = UNDEFINED;
    for &c in &current {
        if !before.iter().any(|&b| same(b, c))
            && let Err(e) = remove_child(cx, root, c)
        {
            result = e;
            break;
        }
    }
    free_values(cx, current);
    free_values(cx, before);
    result
}

// --- Events -------------------------------------------------------------------

fn scroll_payload(
    cx: *mut JsContext,
    x: f32,
    y: f32,
    content_w: f32,
    content_h: f32,
    width: f32,
    height: f32,
) -> qjs::JSValue {
    let point = |a: f32, b: f32, first: &CStr, second: &CStr| unsafe {
        let object = qjs::JS_NewObject(cx);
        qjs::JS_SetPropertyStr(cx, object, first.as_ptr(), qjs::JS_NewFloat64(f64::from(a)));
        qjs::JS_SetPropertyStr(
            cx,
            object,
            second.as_ptr(),
            qjs::JS_NewFloat64(f64::from(b)),
        );
        object
    };
    unsafe {
        let object = qjs::JS_NewObject(cx);
        qjs::JS_SetPropertyStr(cx, object, c"offset".as_ptr(), point(x, y, c"x", c"y"));
        qjs::JS_SetPropertyStr(
            cx,
            object,
            c"contentSize".as_ptr(),
            point(content_w, content_h, c"width", c"height"),
        );
        qjs::JS_SetPropertyStr(
            cx,
            object,
            c"size".as_ptr(),
            point(width, height, c"width", c"height"),
        );
        object
    }
}

/// Calls node `id`'s handler of `event` with `payload`.
pub(crate) fn dispatch(
    cx: *mut JsContext,
    id: NodeId,
    event: EventKind,
    payload: &crate::EventPayload,
) -> Result<(), JsValue> {
    let Some(handler) = handler(cx, id, event) else {
        return Ok(());
    };
    let value = match payload {
        crate::EventPayload::None => unsafe { qjs::JS_NewObject(cx) },
        crate::EventPayload::Text(text) => unsafe {
            qjs::JS_NewStringLen(cx, text.as_ptr().cast(), text.len() as _)
        },
        crate::EventPayload::Scroll {
            x,
            y,
            content_w,
            content_h,
            width,
            height,
        } => scroll_payload(cx, *x, *y, *content_w, *content_h, *width, *height),
    };
    let mut args = [value];
    let r = unsafe { qjs::JS_Call(cx, handler, UNDEFINED, 1, args.as_mut_ptr()) };
    unsafe {
        qjs::JS_FreeValue(cx, handler);
        qjs::JS_FreeValue(cx, value);
    }
    if unsafe { qjs::JS_IsException(r) } {
        if catch_listener_failure(cx, id) {
            Ok(())
        } else {
            Err(r)
        }
    } else {
        unsafe { qjs::JS_FreeValue(cx, r) };
        Ok(())
    }
}

/// The handler node `id` has for `event` (owned), if any.
pub(crate) fn handler(cx: *mut JsContext, id: NodeId, event: EventKind) -> Option<JsValue> {
    let p = dom(cx).by_id(id);
    if p.is_null() {
        return None;
    }
    // SAFETY: live node data; a dup of the handler.
    unsafe { &(*p).handlers }
        .iter()
        .find(|&&(e, _)| e == event)
        .map(|&(_, h)| unsafe { qjs::JS_DupValue(cx, h) })
}

/// Hands the pending exception of a listener of node `id` to the error
/// boundary the node was made under: whether one caught it.
pub(crate) fn catch_listener_failure(cx: *mut JsContext, id: NodeId) -> bool {
    let p = dom(cx).by_id(id);
    if p.is_null() {
        return false;
    }
    // SAFETY: live node data.
    let owner = unsafe { (*p).owner };
    reactive::catch_pending(cx, owner)
}

// --- The primitives as components ---------------------------------------------

/// A component rendering host element `tag` with its props (children too
/// when `children`): what `@nlayer/ui`'s `View`, `Text`, `Image`, `Scroll`
/// and `Input` are as values.
pub(crate) fn host_component(cx: *mut JsContext, tag: &str, children: bool) -> JsValue {
    let name = new_string(cx, tag);
    let mut data = [
        name,
        if children {
            qjs::JS_TRUE
        } else {
            qjs::JS_FALSE
        },
    ];
    // SAFETY: the function keeps dups of `data`; ours is freed.
    unsafe {
        let f = qjs::JS_NewCFunctionData(cx, Some(render_host), 1, 0, 2, data.as_mut_ptr());
        qjs::JS_FreeValue(cx, name);
        f
    }
}

fn new_string(cx: *mut JsContext, s: &str) -> JsValue {
    // SAFETY: a new string.
    unsafe { qjs::JS_NewStringLen(cx, s.as_ptr().cast(), s.len() as _) }
}

unsafe extern "C" fn render_host(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    let props = arg(argc, argv, 0);
    // SAFETY: two data values (`host_component`).
    let (tag, children) = unsafe { (*data, qjs::JS_ToBool(cx, *data.add(1)) == 1) };
    let Some(tag) = string_arg(cx, tag).and_then(|t| Tag::from_name(&t)) else {
        return UNDEFINED;
    };
    let mut args = [new_string(cx, tag_name(tag))];
    // SAFETY: a live context, as in every native here.
    let node = unsafe { element(cx, UNDEFINED, 1, args.as_mut_ptr()) };
    if unsafe { qjs::JS_IsException(node) } {
        return node;
    }
    let r =
        spread_except(cx, node, props, if children { &[] } else { &["children"] }).and_then(|()| {
            if !children {
                return Ok(());
            }
            let reader = prop_reader(cx, props, "children");
            let r = insert_value(cx, node, reader, None);
            // SAFETY: ours.
            unsafe { qjs::JS_FreeValue(cx, reader) };
            r
        });
    match r {
        Ok(()) => reactive::finish_value(cx, node),
        Err(e) => {
            // SAFETY: ours.
            unsafe { qjs::JS_FreeValue(cx, node) };
            e
        }
    }
}

fn tag_name(tag: Tag) -> &'static str {
    match tag {
        Tag::View => "view",
        Tag::Text => "text",
        Tag::Image => "image",
        Tag::Scroll => "scroll",
        Tag::Input => "input",
    }
}

/// A function returning `props[key]` when called (owned).
fn prop_reader(cx: *mut JsContext, props: JsValue, key: &str) -> JsValue {
    let k = new_string(cx, key);
    let mut data = [props, k];
    // SAFETY: the function keeps dups of `data`; ours is freed.
    unsafe {
        let f = qjs::JS_NewCFunctionData(cx, Some(read_prop), 0, 0, 2, data.as_mut_ptr());
        qjs::JS_FreeValue(cx, k);
        f
    }
}

unsafe extern "C" fn read_prop(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values (props, key); the atom is freed.
    unsafe {
        if !qjs::JS_IsObject(*data) {
            return UNDEFINED;
        }
        let key = qjs::JS_ValueToAtom(cx, *data.add(1));
        let v = qjs::JS_GetProperty(cx, *data, key);
        qjs::JS_FreeAtom(cx, key);
        v
    }
}

/// [`spread`] of `props` onto `node` without the keys `skip`.
fn spread_except(
    cx: *mut JsContext,
    node: JsValue,
    props: JsValue,
    skip: &[&str],
) -> Result<(), JsValue> {
    let mut keys: Vec<JsValue> = skip
        .iter()
        // SAFETY: new strings the array takes.
        .map(|k| unsafe { qjs::JS_NewStringLen(cx, k.as_ptr().cast(), k.len() as _) })
        .collect();
    let skip = unsafe { qjs::JS_NewArrayFrom(cx, keys.len() as c_int, keys.as_mut_ptr()) };
    let mut data = [node, props, skip];
    // SAFETY: the function keeps dups of `data`; ours is freed.
    let func = unsafe {
        qjs::JS_NewCFunctionData(cx, Some(spread_except_run), 1, 0, 3, data.as_mut_ptr())
    };
    unsafe { qjs::JS_FreeValue(cx, skip) };
    let r = reactive::render_effect(cx, func);
    unsafe { qjs::JS_FreeValue(cx, r) };
    let r = reactive::finish_value(cx, UNDEFINED);
    if unsafe { qjs::JS_IsException(r) } {
        Err(r)
    } else {
        Ok(())
    }
}

unsafe extern "C" fn spread_except_run(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: three data values (`spread_except`); every value got is
    // freed.
    unsafe {
        let (node, props, skip) = (*data, *data.add(1), *data.add(2));
        let skip: Vec<String> = array_items(cx, skip)
            .into_iter()
            .map(|k| {
                let s = to_string(cx, k).unwrap_or_default();
                qjs::JS_FreeValue(cx, k);
                s
            })
            .collect();
        let prev = arg(argc, argv, 0);
        let next = qjs::JS_NewObject(cx);
        let mut tab: *mut qjs::JSPropertyEnum = ptr::null_mut();
        let mut len: u32 = 0;
        if !qjs::JS_IsObject(props)
            || qjs::JS_GetOwnPropertyNames(
                cx,
                &mut tab,
                &mut len,
                props,
                (qjs::JS_GPN_STRING_MASK | qjs::JS_GPN_ENUM_ONLY) as c_int,
            ) != 0
        {
            return next;
        }
        let mut failed = None;
        for i in 0..len as usize {
            let atom = (*tab.add(i)).atom;
            let name = qjs::JS_AtomToCStringLen(cx, std::ptr::null_mut(), atom);
            if name.is_null() {
                failed = Some(qjs::JS_EXCEPTION);
                break;
            }
            let name_str = CStr::from_ptr(name).to_string_lossy().into_owned();
            qjs::JS_FreeCString(cx, name);
            if skip.contains(&name_str) {
                continue;
            }
            let v = qjs::JS_GetProperty(cx, props, atom);
            if qjs::JS_IsException(v) {
                failed = Some(v);
                break;
            }
            let old = if qjs::JS_IsObject(prev) {
                qjs::JS_GetProperty(cx, prev, atom)
            } else {
                UNDEFINED
            };
            let r = if qjs::JS_IsStrictEqual(cx, old, v) {
                Ok(())
            } else {
                set_prop(cx, node, &name_str, v, old)
            };
            qjs::JS_FreeValue(cx, old);
            qjs::JS_SetProperty(cx, next, atom, v);
            if let Err(e) = r {
                failed = Some(e);
                break;
            }
        }
        qjs::JS_FreePropertyEnum(cx, tab, len);
        match failed {
            Some(e) => {
                qjs::JS_FreeValue(cx, next);
                e
            }
            None => next,
        }
    }
}

// --- Error boundaries ---------------------------------------------------------

/// `ErrorBoundary({ fallback, children })`: its children, or its fallback
/// once something under them failed (a component's body, a binding, an
/// effect, a listener). `fallback(error, reset)` gets the thrown value and
/// what renders the children again; a fallback that takes no arguments is
/// shown as is.
pub(crate) unsafe extern "C" fn error_boundary(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let failed = reactive::new_signal(cx, UNDEFINED);
    let mut data = [failed, arg(argc, argv, 0)];
    // SAFETY: the function keeps dups of `data`; ours is freed.
    let func =
        unsafe { qjs::JS_NewCFunctionData(cx, Some(boundary_run), 0, 0, 2, data.as_mut_ptr()) };
    unsafe { qjs::JS_FreeValue(cx, failed) };
    if unsafe { qjs::JS_IsException(func) } {
        return func;
    }
    let view = reactive::memo(cx, func, false);
    reactive::finish_value(cx, view)
}

/// A run of an `ErrorBoundary`'s memo: the fallback while its signal holds
/// an error, else a memo rendering the children.
unsafe extern "C" fn boundary_run(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values (`error_boundary`); values got are freed.
    let (failed, props) = unsafe { (*data, *data.add(1)) };
    let state = unsafe { qjs::JS_Call(cx, failed, UNDEFINED, 0, ptr::null_mut()) };
    if unsafe { qjs::JS_IsException(state) } {
        return state;
    }
    // SAFETY: inline checks; values got are owned.
    unsafe {
        if !qjs::JS_IsArray(state) {
            qjs::JS_FreeValue(cx, state);
            let mut fdata = [failed, props];
            let func =
                qjs::JS_NewCFunctionData(cx, Some(boundary_view), 0, 0, 2, fdata.as_mut_ptr());
            if qjs::JS_IsException(func) {
                return func;
            }
            return reactive::memo(cx, func, false);
        }
        let error = qjs::JS_GetPropertyUint32(cx, state, 0);
        qjs::JS_FreeValue(cx, state);
        reactive::untracked_with(cx, || {
            let fallback = qjs::JS_GetPropertyStr(cx, props, c"fallback".as_ptr());
            if !qjs::JS_IsFunction(cx, fallback) || !node_ptr(cx, fallback).is_null() {
                qjs::JS_FreeValue(cx, error);
                return fallback;
            }
            let mut length = 0i32;
            let l = qjs::JS_GetPropertyStr(cx, fallback, c"length".as_ptr());
            qjs::JS_ToInt32(cx, &mut length, l);
            qjs::JS_FreeValue(cx, l);
            if length == 0 {
                // An element to show, as children are.
                qjs::JS_FreeValue(cx, error);
                return fallback;
            }
            let mut rdata = [failed];
            let reset =
                qjs::JS_NewCFunctionData(cx, Some(boundary_reset), 0, 0, 1, rdata.as_mut_ptr());
            let mut args = [error, reset];
            let r = qjs::JS_Call(cx, fallback, UNDEFINED, 2, args.as_mut_ptr());
            for v in [fallback, error, reset] {
                qjs::JS_FreeValue(cx, v);
            }
            r
        })
    }
}

/// The memo of an `ErrorBoundary`'s children: names the boundary in its
/// context, then renders them (untracked).
unsafe extern "C" fn boundary_view(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values (`boundary_run`); values got are owned.
    let (failed, props) = unsafe { (*data, *data.add(1)) };
    reactive::provide(cx, reactive::boundary_key(cx), failed);
    reactive::untracked_with(cx, || unsafe {
        let children = qjs::JS_GetPropertyStr(cx, props, c"children".as_ptr());
        if qjs::JS_IsException(children) {
            // Caught here, not as the memo's failure: a failed memo runs
            // again when it is read, and the children would render twice.
            reactive::catch_pending_at(cx, failed);
            return UNDEFINED;
        }
        if qjs::JS_IsFunction(cx, children) && node_ptr(cx, children).is_null() {
            return item_memo(cx, children);
        }
        match memoize_run(cx, children) {
            Some(memos) => {
                qjs::JS_FreeValue(cx, children);
                memos
            }
            None => children,
        }
    })
}

/// An `ErrorBoundary`'s `reset`: renders its children again.
unsafe extern "C" fn boundary_reset(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: one data value (`boundary_run`).
    reactive::set_signal(cx, unsafe { *data }, UNDEFINED);
    reactive::finish_value(cx, UNDEFINED)
}

/// The global `If` (`<If when else>` called as a component: the compiler
/// compiles the tag itself to `cond`).
unsafe extern "C" fn if_component(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let props = arg(argc, argv, 0);
    let mut args = [
        prop_reader(cx, props, "when"),
        prop_reader(cx, props, "children"),
        prop_reader(cx, props, "else"),
    ];
    // SAFETY: `cond` keeps dups of what it uses; ours are freed.
    unsafe {
        let r = cond(cx, UNDEFINED, 3, args.as_mut_ptr());
        for v in args {
            qjs::JS_FreeValue(cx, v);
        }
        r
    }
}

/// The global `Each` (`<Each of key>` called as a component: the compiler
/// compiles the tag itself to `each`).
unsafe extern "C" fn each_component(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let props = arg(argc, argv, 0);
    // SAFETY: values got are owned; `each` keeps dups of what it uses.
    unsafe {
        let mut args = [
            prop_reader(cx, props, "of"),
            qjs::JS_GetPropertyStr(cx, props, c"children".as_ptr()),
            qjs::JS_GetPropertyStr(cx, props, c"key".as_ptr()),
        ];
        let r = if args[1..].iter().any(|v| qjs::JS_IsException(*v)) {
            qjs::JS_EXCEPTION
        } else {
            each(cx, UNDEFINED, 3, args.as_mut_ptr())
        };
        for v in args {
            qjs::JS_FreeValue(cx, v);
        }
        r
    }
}

/// The globals of the renderer: `If` and `Each`, the control flow tags no
/// app imports.
fn install_globals(cx: *mut JsContext) {
    // SAFETY: the global object is ours to free; it takes the functions.
    unsafe {
        let global = qjs::JS_GetGlobalObject(cx);
        let f = reactive::new_function(cx, c"If", if_component, 1);
        qjs::JS_SetPropertyStr(cx, global, c"If".as_ptr(), f);
        let f = reactive::new_function(cx, c"Each", each_component, 1);
        qjs::JS_SetPropertyStr(cx, global, c"Each".as_ptr(), f);
        qjs::JS_FreeValue(cx, global);
    }
}

/// The exports of `nlayer:rt`.
pub(crate) const RT_EXPORTS: &[(&str, crate::modules::Export)] = &[
    ("element", crate::modules::Export::Function(element, 1)),
    ("text", crate::modules::Export::Function(text, 1)),
    ("append", crate::modules::Export::Function(append, 2)),
    ("insert", crate::modules::Export::Function(insert, 3)),
    ("prop", crate::modules::Export::Function(prop, 3)),
    ("bindProp", crate::modules::Export::Function(bind_prop, 3)),
    ("spread", crate::modules::Export::Function(spread, 2)),
    ("component", crate::modules::Export::Function(component, 2)),
    ("cond", crate::modules::Export::Function(cond, 3)),
    ("each", crate::modules::Export::Function(each, 3)),
    ("merge", crate::modules::Export::Function(merge, 2)),
];

/// Installs the renderer: its classes, the root node and the globals.
pub(crate) fn install(cx: &Ctx<'_>) {
    init(cx);
}

/// Moves what nodes collected since the last call into `ops`.
pub(crate) fn take_releases(dom: &Dom, ops: &mut Vec<Op>) {
    let mut releases = dom.releases.borrow_mut();
    for id in releases.drain(..) {
        ops.push(Op::Release { id });
    }
}

/// The renderer of `cx` for the host.
pub(crate) fn of(cx: &Ctx<'_>) -> &'static Dom {
    dom(cx.as_raw().as_ptr())
}
