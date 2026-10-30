//! `<List>`: a virtualized, recycling list over a native scroll view.
//!
//! The core decides which items are rendered and asks for cells
//! (`Host::bind_list`). A cell is a `<cell>` node, a layout root of its own;
//! its index is a signal, and its content is a render effect over
//! `props.children(item, index)`, so a recycled cell shows its new row
//! without rebuilding the cell. Header, footer and empty are non-recycled
//! cells. Row-local state travels with the cell, so apps keep per-item state
//! outside, keyed by id.
//!
//! Extracted from `nsh-js/src/list.rs`, cut to v2: no sticky headers,
//! viewability, parent viewports or inverted lists.

use std::{
    ffi::{CString, c_int},
    ptr,
};

use nlayer_core::{Command, NodeId, Op, Tag};
use rquickjs::qjs;

use crate::{dom, reactive};

type JsValue = qjs::JSValue;
type JsContext = qjs::JSContext;

const UNDEFINED: JsValue = qjs::JS_UNDEFINED;

/// What a cell keeps: its node, its index signal and the owner its content
/// runs under.
struct Cell {
    node: JsValue,
    index: JsValue,
    owner: u32,
}

struct ListData {
    props: JsValue,
    node: JsValue,
    /// Cells by slot.
    cells: Vec<Option<Cell>>,
    /// Owners of the header, footer and empty cells.
    extras: Vec<u32>,
}

/// The lists of one context, by node id.
#[derive(Default)]
pub(crate) struct Lists {
    map: std::cell::RefCell<rustc_hash::FxHashMap<NodeId, Box<ListData>>>,
}

impl Lists {
    /// Frees the values the lists hold (the state is unregistered).
    pub(crate) fn free_all(&mut self, raw: *mut JsContext) {
        for (_, data) in self.map.borrow_mut().drain() {
            free_data(raw, *data);
        }
    }
}

fn lists(cx: *mut JsContext) -> &'static Lists {
    // SAFETY: the state of this context, set before the list module runs;
    // a projection of one field.
    unsafe { &(*crate::state::state(cx)).lists }
}

fn free_data(cx: *mut JsContext, data: ListData) {
    // SAFETY: values the list owned.
    unsafe {
        qjs::JS_FreeValue(cx, data.props);
        qjs::JS_FreeValue(cx, data.node);
        for cell in data.cells.into_iter().flatten() {
            qjs::JS_FreeValue(cx, cell.node);
            qjs::JS_FreeValue(cx, cell.index);
        }
    }
}

fn get(cx: *mut JsContext, obj: JsValue, key: &str) -> JsValue {
    let k = CString::new(key).unwrap_or_default();
    // SAFETY: a property read of a live value.
    unsafe {
        if !qjs::JS_IsObject(obj) {
            return UNDEFINED;
        }
        qjs::JS_GetPropertyStr(cx, obj, k.as_ptr())
    }
}

fn has(cx: *mut JsContext, obj: JsValue, key: &str) -> bool {
    let k = CString::new(key).unwrap_or_default();
    // SAFETY: as `get`.
    unsafe {
        if !qjs::JS_IsObject(obj) {
            return false;
        }
        let atom = qjs::JS_NewAtom(cx, k.as_ptr());
        let r = qjs::JS_HasProperty(cx, obj, atom);
        qjs::JS_FreeAtom(cx, atom);
        r == 1
    }
}

fn length(cx: *mut JsContext, array: JsValue) -> i64 {
    let mut len: i64 = 0;
    // SAFETY: reads a live value's length (0 for non-arrays).
    unsafe {
        if !qjs::JS_IsArray(array) || qjs::JS_GetLength(cx, array, &mut len) != 0 {
            return 0;
        }
    }
    len
}

fn number(cx: *mut JsContext, value: JsValue) -> Option<f64> {
    // SAFETY: inline tag check; conversion of a live value.
    unsafe {
        if qjs::JS_IsNumber(value) {
            let mut n = 0.0;
            qjs::JS_ToFloat64(cx, &mut n, value);
            Some(n)
        } else {
            None
        }
    }
}

fn new_string(cx: *mut JsContext, s: &str) -> JsValue {
    // SAFETY: a new string.
    unsafe { qjs::JS_NewStringLen(cx, s.as_ptr().cast(), s.len() as _) }
}

fn arg(argc: c_int, argv: *mut JsValue, i: usize) -> JsValue {
    if i < argc.max(0) as usize {
        // SAFETY: the call's arguments.
        unsafe { *argv.add(i) }
    } else {
        UNDEFINED
    }
}

type Native = unsafe extern "C" fn(
    *mut JsContext,
    JsValue,
    c_int,
    *mut JsValue,
    c_int,
    *mut JsValue,
) -> JsValue;

fn native(cx: *mut JsContext, f: Native, length: c_int, data: &mut [JsValue]) -> JsValue {
    // SAFETY: the function keeps dups of `data`.
    unsafe {
        qjs::JS_NewCFunctionData(
            cx,
            Some(f),
            length,
            0,
            data.len() as c_int,
            data.as_mut_ptr(),
        )
    }
}

/// Sets prop `name` of `node` to `value` (owned) and frees the value.
fn set(cx: *mut JsContext, node: JsValue, name: &str, value: JsValue) -> Result<(), JsValue> {
    let r = dom::set_prop_value(cx, node, name, value);
    // SAFETY: ours.
    unsafe { qjs::JS_FreeValue(cx, value) };
    r
}

fn set_number(cx: *mut JsContext, node: JsValue, name: &str, n: f64) -> Result<(), JsValue> {
    // SAFETY: a new number; `set` frees it.
    set(cx, node, name, qjs::JS_NewFloat64(n))
}

/// `children(item, index)` with both accessors.
fn content_value(cx: *mut JsContext, props: JsValue, item: JsValue, index: JsValue) -> JsValue {
    let children = get(cx, props, "children");
    let mut args = [item, index];
    // SAFETY: a call of a live function (untracked); the result is owned.
    let content = unsafe {
        reactive::untracked_with(cx, || {
            qjs::JS_Call(cx, children, UNDEFINED, 2, args.as_mut_ptr())
        })
    };
    unsafe { qjs::JS_FreeValue(cx, children) };
    content
}

/// A slot value as it renders: a component is called (untracked) with `{}`.
fn slot(cx: *mut JsContext, value: JsValue) -> JsValue {
    // SAFETY: an inline check; a component is called untracked.
    unsafe {
        if qjs::JS_IsFunction(cx, value) && !dom::is_node_value(cx, value) {
            let mut args = [qjs::JS_NewObject(cx)];
            let r = reactive::untracked_with(cx, || {
                qjs::JS_Call(cx, value, UNDEFINED, 1, args.as_mut_ptr())
            });
            qjs::JS_FreeValue(cx, args[0]);
            qjs::JS_FreeValue(cx, value);
            return r;
        }
    }
    value
}

unsafe extern "C" fn slot_run(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values (props, key).
    unsafe {
        let (props, key) = (*data, *data.add(1));
        let Some(key) = dom::string_value(cx, key) else {
            return qjs::JS_EXCEPTION;
        };
        let value = get(cx, props, &key);
        slot(cx, value)
    }
}

/// A non-recycled cell (`header`, `footer`, `empty`); returns its owner.
fn extra_cell(cx: *mut JsContext, id: NodeId, role: &str, key: &str) -> Result<u32, JsValue> {
    let (props, list) = {
        let map = lists(cx).map.borrow();
        let Some(d) = map.get(&id) else {
            return Err(qjs::JS_EXCEPTION);
        };
        (d.props, d.node)
    };
    let (root, r) = reactive::with_root(cx, || -> Result<(), JsValue> {
        let cell = dom::create_element(cx, Tag::Cell);
        let r = (|| -> Result<(), JsValue> {
            set(cx, cell, "slot", new_string(cx, role))?;
            dom::insert_node(cx, list, cell, None)?;
            let key = new_string(cx, key);
            let reader = native(cx, slot_run, 0, &mut [props, key]);
            // SAFETY: ours.
            unsafe { qjs::JS_FreeValue(cx, key) };
            let r = dom::insert_value_at(cx, cell, reader, None);
            unsafe { qjs::JS_FreeValue(cx, reader) };
            r
        })();
        // The list holds it now.
        // SAFETY: our reference.
        unsafe { qjs::JS_FreeValue(cx, cell) };
        r
    });
    r.map(|()| root)
}

/// `List`'s component: renders the list node for `props` (borrowed).
pub(crate) unsafe extern "C" fn list(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let props = arg(argc, argv, 0);
    match render(cx, props) {
        Ok(node) => reactive::finish_value(cx, node),
        Err(e) => e,
    }
}

fn render(cx: *mut JsContext, props: JsValue) -> Result<JsValue, JsValue> {
    let node = dom::create_element(cx, Tag::List);
    let id = dom::node_id(cx, node).expect("a node");
    // SAFETY: dups the list owns.
    let data = Box::new(ListData {
        props: unsafe { qjs::JS_DupValue(cx, props) },
        node: unsafe { qjs::JS_DupValue(cx, node) },
        cells: Vec::new(),
        extras: Vec::new(),
    });
    lists(cx).map.borrow_mut().insert(id, data);
    let fail = |e: JsValue| {
        // SAFETY: ours.
        unsafe { qjs::JS_FreeValue(cx, node) };
        e
    };
    // The item list: tells the core how it changed (by identity: the
    // common start and end stay), so measurements follow their items.
    let func = native(cx, splice_run, 1, &mut [node, props]);
    let r = reactive::render_effect(cx, func);
    // SAFETY: the effect keeps its own value.
    unsafe { qjs::JS_FreeValue(cx, r) };
    // Every other prop goes to the scroll view (style, testID, events).
    dom::spread_props_except(
        cx,
        node,
        props,
        &[
            "each",
            "children",
            "ref",
            "header",
            "footer",
            "empty",
            "separator",
            "keyExtractor",
            "estimatedItemSize",
            "numColumns",
            "initialScrollIndex",
            "loadMoreThreshold",
        ],
    )
    .map_err(fail)?;
    for (name, fallback) in [
        ("estimatedItemSize", 50.0),
        ("numColumns", 1.0),
        ("loadMoreThreshold", 1.0),
    ] {
        let value = get(cx, props, name);
        let n = number(cx, value).filter(|n| *n > 0.0).unwrap_or(fallback);
        // SAFETY: ours.
        unsafe { qjs::JS_FreeValue(cx, value) };
        set_number(cx, node, name, n).map_err(fail)?;
    }
    // Header and footer cells always; the empty one only without items.
    for (role, key) in [("header", "header"), ("footer", "footer")] {
        if has(cx, props, key) {
            let owner = extra_cell(cx, id, role, key).map_err(fail)?;
            if let Some(d) = lists(cx).map.borrow_mut().get_mut(&id) {
                d.extras.push(owner);
            }
        }
    }
    if has(cx, props, "empty") {
        let owner = extra_cell(cx, id, "empty", "empty").map_err(fail)?;
        if let Some(d) = lists(cx).map.borrow_mut().get_mut(&id) {
            d.extras.push(owner);
        }
    }
    let initial = get(cx, props, "initialScrollIndex");
    if let Some(index) = number(cx, initial) {
        dom::push_op(
            cx,
            Op::Command {
                id,
                command: Command::ScrollToIndex {
                    index: index.max(0.0) as u32,
                    animated: false,
                },
            },
        );
    }
    // SAFETY: ours.
    unsafe { qjs::JS_FreeValue(cx, initial) };
    let handle = create_handle(cx, id);
    let ref_ = get(cx, props, "ref");
    // SAFETY: a borrowed value; objects are checked inline.
    unsafe {
        if qjs::JS_IsFunction(cx, ref_) {
            let mut args = [handle];
            let r = reactive::untracked_with(cx, || {
                qjs::JS_Call(cx, ref_, UNDEFINED, 1, args.as_mut_ptr())
            });
            qjs::JS_FreeValue(cx, r);
        } else if qjs::JS_IsObject(ref_) {
            qjs::JS_SetPropertyStr(cx, ref_, c"current".as_ptr(), qjs::JS_DupValue(cx, handle));
        }
        qjs::JS_FreeValue(cx, ref_);
        qjs::JS_FreeValue(cx, handle);
    }
    // Gone with its owner: the cells and the binding.
    let cleanup = native(cx, dispose, 0, &mut [qjs::JS_NewFloat64(id as f64)]);
    reactive::add_cleanup(cx, cleanup);
    Ok(node)
}

/// `{ scrollToIndex(index, { animated }?), scrollToEnd({ animated }?) }`.
fn create_handle(cx: *mut JsContext, id: NodeId) -> JsValue {
    // SAFETY: a new object; methods are new functions.
    unsafe {
        let object = qjs::JS_NewObject(cx);
        let index = qjs::JS_NewCFunctionData(
            cx,
            Some(scroll_to_index),
            1,
            id as c_int,
            0,
            ptr::null_mut(),
        );
        qjs::JS_SetPropertyStr(cx, object, c"scrollToIndex".as_ptr(), index);
        let end =
            qjs::JS_NewCFunctionData(cx, Some(scroll_to_end), 1, id as c_int, 0, ptr::null_mut());
        qjs::JS_SetPropertyStr(cx, object, c"scrollToEnd".as_ptr(), end);
        object
    }
}

fn animated(cx: *mut JsContext, argc: c_int, argv: *mut JsValue) -> bool {
    let options = arg(argc, argv, 1);
    if !unsafe { qjs::JS_IsObject(options) } {
        return false;
    }
    let value = get(cx, options, "animated");
    // SAFETY: a live value; JS_ToBool of undefined is 0.
    let animated = unsafe { qjs::JS_ToBool(cx, value) == 1 };
    unsafe { qjs::JS_FreeValue(cx, value) };
    animated
}

unsafe extern "C" fn scroll_to_index(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    magic: c_int,
    _data: *mut JsValue,
) -> JsValue {
    let index = number(cx, arg(argc, argv, 0)).unwrap_or(0.0).max(0.0) as u32;
    let animated = animated(cx, argc, argv);
    dom::push_op(
        cx,
        Op::Command {
            id: magic as NodeId,
            command: Command::ScrollToIndex { index, animated },
        },
    );
    UNDEFINED
}

unsafe extern "C" fn scroll_to_end(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    magic: c_int,
    _data: *mut JsValue,
) -> JsValue {
    let animated = animated(cx, argc, argv);
    dom::push_op(
        cx,
        Op::Command {
            id: magic as NodeId,
            command: Command::ScrollToEnd { animated },
        },
    );
    UNDEFINED
}

unsafe extern "C" fn dispose(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: one data value (the id).
    let id = unsafe { number(cx, *data) }.unwrap_or(-1.0) as NodeId;
    let Some(data) = lists(cx).map.borrow_mut().remove(&id) else {
        return UNDEFINED;
    };
    for cell in data.cells.iter().flatten() {
        reactive::dispose_owner(cx, cell.owner);
    }
    for &owner in &data.extras {
        reactive::dispose_owner(cx, owner);
    }
    free_data(cx, *data);
    UNDEFINED
}

/// A run of the items effect: the items set last in, the current out.
unsafe extern "C" fn splice_run(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values; values got are freed or returned.
    unsafe {
        let (node, props) = (*data, *data.add(1));
        let old = arg(argc, argv, 0);
        let next = get(cx, props, "each");
        if qjs::JS_IsException(next) {
            return next;
        }
        let (old_len, next_len) = (length(cx, old), length(cx, next));
        if old_len > 0 && !qjs::JS_IsStrictEqual(cx, old, next) {
            let max = old_len.min(next_len);
            let same = |i: i64, j: i64| {
                let a = qjs::JS_GetPropertyInt64(cx, old, i);
                let b = qjs::JS_GetPropertyInt64(cx, next, j);
                let s = qjs::JS_IsStrictEqual(cx, a, b);
                qjs::JS_FreeValue(cx, a);
                qjs::JS_FreeValue(cx, b);
                s
            };
            let mut start = 0;
            while start < max && same(start, start) {
                start += 1;
            }
            let mut end = 0;
            while end < max - start && same(old_len - 1 - end, next_len - 1 - end) {
                end += 1;
            }
            let removed = old_len - start - end;
            let inserted = next_len - start - end;
            if removed > 0 || inserted > 0 {
                let splice = qjs::JS_NewArray(cx);
                qjs::JS_SetPropertyUint32(cx, splice, 0, qjs::JS_NewFloat64(start as f64));
                qjs::JS_SetPropertyUint32(cx, splice, 1, qjs::JS_NewFloat64(removed as f64));
                qjs::JS_SetPropertyUint32(cx, splice, 2, qjs::JS_NewFloat64(inserted as f64));
                let _ = set(cx, node, "splice", splice);
            }
        }
        let _ = set_number(cx, node, "count", next_len as f64);
        next
    }
}

/// `Host::bind_list`: answers `(slot, index)` with rendered cells.
pub(crate) fn bind(cx: *mut JsContext, list: NodeId, binds: &[(u32, u32)]) -> bool {
    if !lists(cx).map.borrow().contains_key(&list) {
        return false;
    }
    let mut failed = false;
    reactive::batch_with(cx, || {
        for &(slot, index) in binds {
            let existing = lists(cx)
                .map
                .borrow()
                .get(&list)
                .and_then(|d| d.cells.get(slot as usize))
                .and_then(|c| c.as_ref().map(|c| c.index));
            match existing {
                Some(signal) => {
                    let current = reactive::peek(cx, signal);
                    if number(cx, current) != Some(index as f64) {
                        reactive::set_signal(cx, signal, qjs::JS_NewFloat64(index as f64));
                    }
                }
                None => {
                    if let Err(e) = make_cell(cx, list, slot, index) {
                        eprintln!("make_cell failed: exception={}", unsafe {
                            qjs::JS_IsException(e)
                        });
                        failed = true;
                    }
                }
            }
        }
    });
    let r = reactive::finish_value(cx, UNDEFINED);
    // SAFETY: an inline tag check.
    if unsafe { qjs::JS_IsException(r) } {
        failed = true;
    }
    for &(slot, index) in binds {
        let node = lists(cx)
            .map
            .borrow()
            .get(&list)
            .and_then(|d| d.cells.get(slot as usize))
            .and_then(|c| c.as_ref().map(|c| c.node));
        if let Some(node) = node
            && let Err(e) = set_number(cx, node, "cellIndex", index as f64)
        {
            eprintln!("cellIndex failed: exception={}", unsafe {
                qjs::JS_IsException(e)
            });
            failed = true;
        }
    }
    !failed
}

/// A new cell for `slot`, showing item `first`.
fn make_cell(cx: *mut JsContext, list: NodeId, slot: u32, first: u32) -> Result<(), JsValue> {
    let (props, list_node) = {
        let map = lists(cx).map.borrow();
        let d = map.get(&list).ok_or(qjs::JS_EXCEPTION)?;
        (d.props, d.node)
    };
    let mut made: Option<(JsValue, JsValue)> = None;
    let (root, r) = reactive::with_root(cx, || -> Result<(), JsValue> {
        let index = reactive::new_signal(cx, qjs::JS_NewFloat64(first as f64));
        let item_fn = native(cx, item_run, 0, &mut [props, index]);
        // The memo takes the function.
        let item = reactive::memo(cx, item_fn, false);
        let cell = dom::create_element(cx, Tag::Cell);
        made = Some((cell, index));
        set_number(cx, cell, "slot", slot as f64)?;
        dom::insert_node(cx, list_node, cell, None)?;
        // The separator is made once per cell and reused.
        unsafe {
            let holder = qjs::JS_NewArray(cx);
            let content = native(cx, content_run, 0, &mut [props, index, item, holder]);
            let r = dom::insert_value_at(cx, cell, content, None);
            qjs::JS_FreeValue(cx, content);
            qjs::JS_FreeValue(cx, holder);
            qjs::JS_FreeValue(cx, item);
            r
        }
    });
    if let Some((node, index)) = made {
        let mut map = lists(cx).map.borrow_mut();
        match map.get_mut(&list) {
            Some(d) => {
                let slot = slot as usize;
                if d.cells.len() <= slot {
                    d.cells.resize_with(slot + 1, || None);
                }
                d.cells[slot] = Some(Cell {
                    node,
                    index,
                    owner: root,
                });
            }
            None => {
                drop(map);
                // SAFETY: ours (the list went meanwhile).
                unsafe {
                    qjs::JS_FreeValue(cx, node);
                    qjs::JS_FreeValue(cx, index);
                }
            }
        }
    }
    r
}

/// A cell's item: `props.each[index()]`, or the last one past the end (the
/// data shrank before the core unbound the cell).
unsafe extern "C" fn item_run(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values; values got are freed or returned.
    unsafe {
        let (props, index) = (*data, *data.add(1));
        let i = qjs::JS_Call(cx, index, UNDEFINED, 0, ptr::null_mut());
        let Some(n) = number(cx, i) else {
            qjs::JS_FreeValue(cx, i);
            return qjs::JS_DupValue(cx, arg(argc, argv, 0));
        };
        qjs::JS_FreeValue(cx, i);
        let each = get(cx, props, "each");
        let len = length(cx, each);
        let r = if n >= 0.0 && (n as i64) < len {
            qjs::JS_GetPropertyInt64(cx, each, n as i64)
        } else {
            qjs::JS_DupValue(cx, arg(argc, argv, 0))
        };
        qjs::JS_FreeValue(cx, each);
        r
    }
}

/// A cell's content: `children(item, index)`, with the separator after it
/// unless it is the last row.
unsafe extern "C" fn content_run(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: four data values; values got are freed or returned.
    unsafe {
        let (props, index, item, holder) = (*data, *data.add(1), *data.add(2), *data.add(3));
        let content = content_value(cx, props, item, index);
        if qjs::JS_IsException(content) {
            return content;
        }
        if !has(cx, props, "separator") {
            return content;
        }
        let i = qjs::JS_Call(cx, index, UNDEFINED, 0, ptr::null_mut());
        let Some(n) = number(cx, i) else {
            qjs::JS_FreeValue(cx, i);
            return content;
        };
        qjs::JS_FreeValue(cx, i);
        let each = get(cx, props, "each");
        let len = length(cx, each);
        qjs::JS_FreeValue(cx, each);
        if n as i64 >= len - 1 {
            return content;
        }
        let component = get(cx, props, "separator");
        let mut separator = qjs::JS_GetPropertyUint32(cx, holder, 0);
        if qjs::JS_IsUndefined(separator) {
            separator = slot(cx, component);
            if qjs::JS_IsException(separator) {
                qjs::JS_FreeValue(cx, content);
                return separator;
            }
            qjs::JS_SetPropertyUint32(cx, holder, 0, qjs::JS_DupValue(cx, separator));
        } else {
            qjs::JS_FreeValue(cx, component);
        }
        let mut pair = [content, separator];
        qjs::JS_NewArrayFrom(cx, 2, pair.as_mut_ptr())
    }
}

#[cfg(test)]
mod tests {
    use nlayer_core::{Op, Prop};

    use crate::Host;

    fn list_with_rows(host: &Host) -> nlayer_core::NodeId {
        host.eval_source(
            "import { mount } from \"@nlayer/runtime\";\n\
             import { List, Text } from \"@nlayer/ui\";\n\
             mount(() => List({\n\
               each: [1, 2, 3],\n\
               children: (item, index) => Text({ children: String(item()) + \"@\" + index() }),\n\
             }));",
        )
        .unwrap();
        let mut ops = Vec::new();
        host.take_ops(&mut ops);
        let list = ops
            .iter()
            .find_map(|op| match op {
                Op::Create {
                    id,
                    tag: nlayer_core::Tag::List,
                } => Some(*id),
                _ => None,
            })
            .expect("a list");
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::SetProp {
                prop: Prop::Count(3),
                ..
            }
        )));
        list
    }

    #[test]
    fn a_list_binds_and_recycles_rows() {
        let host = Host::new().unwrap();
        let list = list_with_rows(&host);
        assert!(host.bind_list(list, &[(0, 0)]));
        let mut ops = Vec::new();
        host.take_ops(&mut ops);
        let cell = ops
            .iter()
            .find_map(|op| match op {
                Op::Create {
                    id,
                    tag: nlayer_core::Tag::Cell,
                } => Some(*id),
                _ => None,
            })
            .expect("a cell");
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::SetProp {
                id,
                prop: Prop::Slot(nlayer_core::CellSlot::Item(0)),
            } if *id == cell
        )));
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::SetProp {
                id,
                prop: Prop::CellIndex(0),
            } if *id == cell
        )));
        // The same slot takes another item without a new cell.
        assert!(host.bind_list(list, &[(0, 2)]));
        let mut ops = Vec::new();
        host.take_ops(&mut ops);
        assert!(!ops.iter().any(|op| matches!(
            op,
            Op::Create {
                tag: nlayer_core::Tag::Cell,
                ..
            }
        )));
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::SetProp {
                id,
                prop: Prop::CellIndex(2),
            } if *id == cell
        )));
    }

    #[test]
    fn a_list_drops_cleanly() {
        let host = Host::new().unwrap();
        let list = list_with_rows(&host);
        assert!(host.bind_list(list, &[(0, 0), (1, 1)]));
        drop(host);
    }

    #[test]
    fn a_list_forwards_load_more_and_reads_the_handle() {
        let host = Host::new().unwrap();
        host.eval_source(
            "import { mount, signal } from \"@nlayer/runtime\";\n\
             import { List, Text } from \"@nlayer/ui\";\n\
             const calls = [];\n\
             const handle = (h) => { calls.push(h); h.scrollToEnd(); };\n\
             mount(() => List({\n\
               each: [],\n\
               ref: handle,\n\
               onLoadMore: () => calls.push(\"more\"),\n\
               children: () => Text({ children: \"empty?\" }),\n\
             }));",
        )
        .unwrap();
        let mut ops = Vec::new();
        host.take_ops(&mut ops);
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::Listen {
                event: nlayer_core::EventKind::LoadMore,
                enabled: true,
                ..
            }
        )));
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::Command {
                command: nlayer_core::Command::ScrollToEnd { animated: false },
                ..
            }
        )));
    }
}
