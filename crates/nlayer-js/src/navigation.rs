//! Navigation and dialogs: `createNavStack`, `NavStack`, `NavPage` and
//! `Dialog` (`@nlayer/ui` exports them).
//!
//! A stack renders each route of its state with the app's function; pages
//! stay mounted while their route is in the stack. A page the user pops
//! natively removes its route. Pages and dialogs provide their insets to
//! what they render.
//!
//! M1: JS surface only. Native containers (UINavigationController, Android
//! stack, web history) come in M2.

#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::{CStr, c_int};

use nlayer_core::Tag;
use rquickjs::qjs;

use crate::{dom, reactive};

type JsValue = qjs::JSValue;
type JsContext = qjs::JSContext;

const UNDEFINED: JsValue = qjs::JS_UNDEFINED;

type DataFn = unsafe extern "C" fn(
    *mut JsContext,
    JsValue,
    c_int,
    *mut JsValue,
    c_int,
    *mut JsValue,
) -> JsValue;

fn native(cx: *mut JsContext, f: DataFn, length: c_int, data: &mut [JsValue]) -> JsValue {
    // SAFETY: a function data closure that keeps dups of `data`.
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

fn free(cx: *mut JsContext, v: JsValue) {
    // SAFETY: freeing an owned value.
    unsafe { qjs::JS_FreeValue(cx, v) }
}

/// `@nlayer/ui`'s navigation exports.
pub(crate) const EXPORTS: &[(&str, fn(*mut JsContext) -> JsValue)] = &[
    ("createNavStack", create_nav_stack_export),
    ("NavStack", stack_export),
    ("NavPage", screen_export),
    ("Dialog", modal_export),
];

// --- The navigator --------------------------------------------------------------

/// The routes (a JS array) as values (owned).
fn routes_of(cx: *mut JsContext, routes: JsValue) -> Vec<JsValue> {
    let mut result = Vec::new();
    // SAFETY: reading array elements.
    unsafe {
        if !qjs::JS_IsArray(routes) {
            return result;
        }
        let mut len: i64 = 0;
        if qjs::JS_GetLength(cx, routes, &mut len) != 0 {
            return result;
        }
        for i in 0..len.max(0) as u32 {
            let route = qjs::JS_GetPropertyUint32(cx, routes, i);
            result.push(route);
        }
    }
    result
}

/// `createNavStack(routes)`: the route stack of a `<NavStack>`.
fn create_nav_stack_export(cx: *mut JsContext) -> JsValue {
    reactive::new_function(cx, c"createNavStack", create_nav_stack, 1)
}

unsafe extern "C" fn create_nav_stack(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    // SAFETY: first argument (routes array).
    let initial = unsafe { if argc > 0 { *argv } else { UNDEFINED } };
    let routes = routes_of(cx, initial);

    // Each route becomes an entry: an object with { route }.
    let entries: Vec<JsValue> = routes
        .into_iter()
        .map(|route| {
            let e = entry(cx, route);
            free(cx, route);
            e
        })
        .collect();

    // The entries signal.
    let arr = new_array(cx, entries);
    let signal = reactive::new_signal(cx, arr);
    free(cx, arr);

    // The state object: { routes, push, pop, ... }.
    // SAFETY: creating object and setting properties.
    let state = unsafe { qjs::JS_NewObject(cx) };

    // Hidden property holding the entries signal.
    let entries_dup = unsafe { qjs::JS_DupValue(cx, signal) };
    unsafe {
        qjs::JS_DefinePropertyValueStr(
            cx,
            state,
            ENTRIES.as_ptr(),
            entries_dup,
            qjs::JS_PROP_CONFIGURABLE as i32,
        );
    }

    // routes() accessor
    let mut data = [signal];
    let routes_fn = native(cx, read_routes, 0, &mut data);
    unsafe {
        qjs::JS_DefinePropertyValueStr(
            cx,
            state,
            c"routes".as_ptr(),
            routes_fn,
            (qjs::JS_PROP_ENUMERABLE | qjs::JS_PROP_CONFIGURABLE) as i32,
        );
    }

    // Mutation methods: push (0), pop (1), popToRoot (2), replace (3), setRoutes (4), remove (5)
    for (magic, name) in [
        (0, c"push"),
        (1, c"pop"),
        (2, c"popToRoot"),
        (3, c"replace"),
        (4, c"setRoutes"),
        (5, c"remove"),
    ] {
        let mut data = [signal];
        // SAFETY: creating method with magic and data.
        let f = unsafe {
            qjs::JS_NewCFunctionData(
                cx,
                Some(navigate),
                if magic == 4 {
                    1
                } else {
                    if magic == 1 { 0 } else { 1 }
                },
                magic,
                1,
                data.as_mut_ptr(),
            )
        };
        unsafe {
            qjs::JS_DefinePropertyValueStr(
                cx,
                state,
                name.as_ptr(),
                f,
                (qjs::JS_PROP_ENUMERABLE | qjs::JS_PROP_CONFIGURABLE) as i32,
            );
        }
    }

    free(cx, signal);
    reactive::finish_value(cx, state)
}

/// The hidden property of a navigator holding its entries' signal.
const ENTRIES: &CStr = c"__nlayerEntries";

/// One screen of a navigator: an object holding the route.
fn entry(cx: *mut JsContext, route: JsValue) -> JsValue {
    // SAFETY: creating { route }.
    unsafe {
        let obj = qjs::JS_NewObject(cx);
        let route_dup = qjs::JS_DupValue(cx, route);
        qjs::JS_SetPropertyStr(cx, obj, c"route".as_ptr(), route_dup);
        obj
    }
}

/// The route of an entry (owned).
fn route_of(cx: *mut JsContext, entry: JsValue) -> JsValue {
    // SAFETY: reading a property (owned).
    unsafe { qjs::JS_GetPropertyStr(cx, entry, c"route".as_ptr()) }
}

unsafe extern "C" fn read_routes(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: data[0] is the entries signal; calling it reads the value.
    let signal = unsafe { *data };
    let entries = unsafe { qjs::JS_Call(cx, signal, UNDEFINED, 0, std::ptr::null_mut()) };
    if unsafe { qjs::JS_IsException(entries) } {
        return entries;
    }
    let routes = array_items(cx, entries);
    let result = routes.iter().map(|&e| route_of(cx, e)).collect::<Vec<_>>();
    free_values(cx, routes);
    free(cx, entries);
    new_array(cx, result)
}

/// The stack state's functions, by magic: push (0), pop (1), popToRoot (2),
/// replace (3), setRoutes (4), remove (5), removeExact (6).
unsafe extern "C" fn navigate(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: data[0] is the entries signal.
    let signal = unsafe { *data };
    let current = unsafe { qjs::JS_Call(cx, signal, UNDEFINED, 0, std::ptr::null_mut()) };
    if unsafe { qjs::JS_IsException(current) } {
        return current;
    }
    let mut entries = array_items(cx, current);
    free(cx, current);

    match magic {
        0 => {
            // push(route)
            if argc > 0 {
                let route = unsafe { *argv };
                entries.push(entry(cx, route));
            }
        }
        1 => {
            // pop(count?)
            let count = if argc > 0 {
                let mut n = 0f64;
                unsafe { qjs::JS_ToFloat64(cx, &mut n, *argv) };
                (n as usize).max(1)
            } else {
                1
            };
            let keep = entries.len().saturating_sub(count).max(1);
            let removed = entries.split_off(keep);
            free_values(cx, removed);
        }
        2 => {
            // popToRoot()
            let removed = entries.split_off(1);
            free_values(cx, removed);
        }
        3 => {
            // replace(route)
            if argc > 0 && !entries.is_empty() {
                let route = unsafe { *argv };
                let old = entries.pop().unwrap();
                free(cx, old);
                entries.push(entry(cx, route));
            }
        }
        4 => {
            // setRoutes(routes): a route that stays keeps its page (entry).
            if argc > 0 {
                let new_routes = routes_of(cx, *argv);
                let mut old: Vec<Option<JsValue>> = entries.drain(..).map(Some).collect();
                let mut next = Vec::with_capacity(new_routes.len());
                for route in new_routes {
                    let kept = old.iter_mut().position(|slot| {
                        slot.as_ref().is_some_and(|e| {
                            let r = route_of(cx, *e);
                            let equal = strict_equal(cx, r, route);
                            free(cx, r);
                            equal
                        })
                    });
                    match kept.and_then(|at| old[at].take()) {
                        Some(e) => {
                            free(cx, route);
                            next.push(e);
                        }
                        None => {
                            next.push(entry(cx, route));
                            free(cx, route);
                        }
                    }
                }
                let left: Vec<JsValue> = old.into_iter().flatten().collect();
                free_values(cx, left);
                entries = next;
            }
        }
        5 => {
            // remove(route): the topmost screen showing that route.
            if argc > 0 && entries.len() > 1 {
                let target = unsafe { *argv };
                if let Some(pos) = entries.iter().rposition(|&e| {
                    let r = route_of(cx, e);
                    let equal = strict_equal(cx, r, target);
                    free(cx, r);
                    equal
                }) {
                    let removed = entries.remove(pos);
                    free(cx, removed);
                }
            }
        }
        6 => {
            // remove this exact entry (the user popped its page).
            if argc > 0 {
                let target = unsafe { *argv };
                if let Some(pos) = entries.iter().position(|&e| strict_equal(cx, e, target)) {
                    let removed = entries.remove(pos);
                    free(cx, removed);
                }
            }
        }
        _ => {}
    }

    let arr = new_array(cx, entries);
    // Write to signal by calling it with the new value (takes ownership).
    reactive::set_signal(cx, signal, arr);
    UNDEFINED
}

// --- Stack ------------------------------------------------------------------------

/// `<NavStack state>{(route, index) => <NavPage …>}</NavStack>`.
fn stack_export(cx: *mut JsContext) -> JsValue {
    reactive::new_function(cx, c"NavStack", stack_component, 1)
}

unsafe extern "C" fn stack_component(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let props = reactive::arg(argc, argv, 0);
    let node = dom::create_element(cx, Tag::Stack);
    // SAFETY: an inline tag check.
    if unsafe { qjs::JS_IsException(node) } {
        return node;
    }
    let result = (|| -> Result<(), JsValue> {
        dom::spread_props_except(cx, node, props, &["state", "children"])?;
        // SAFETY: the navigator and its hidden entries signal, both owned.
        let navigator = unsafe { qjs::JS_GetPropertyStr(cx, props, c"state".as_ptr()) };
        let entries = unsafe { qjs::JS_GetPropertyStr(cx, navigator, ENTRIES.as_ptr()) };
        free(cx, navigator);
        if unsafe { qjs::JS_IsException(entries) } {
            return Err(entries);
        }
        // SAFETY: a row per entry, kept while the entry is in the stack.
        let render = native(cx, stack_row, 2, &mut [props]);
        let mut args = [entries, render];
        let rows = unsafe { crate::dom::each(cx, UNDEFINED, 2, args.as_mut_ptr()) };
        free(cx, entries);
        free(cx, render);
        if unsafe { qjs::JS_IsException(rows) } {
            return Err(rows);
        }
        let r = dom::insert_value_at(cx, node, rows, None);
        free(cx, rows);
        r
    })();
    match result {
        Ok(()) => reactive::finish_value(cx, node),
        Err(e) => {
            free(cx, node);
            e
        }
    }
}

/// One route of a stack: its screen (the app's `<NavPage>`, or one around
/// what it rendered); a native pop removes the route.
unsafe extern "C" fn stack_row(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: one data value (the stack's props); `item` is an accessor.
    let props = unsafe { *data };
    let (item, index) = (reactive::arg(argc, argv, 0), reactive::arg(argc, argv, 1));
    // SAFETY: the entry accessor called untracked.
    let entry = reactive::untracked_with(cx, || unsafe {
        qjs::JS_Call(cx, item, UNDEFINED, 0, std::ptr::null_mut())
    });
    if unsafe { qjs::JS_IsException(entry) } {
        return entry;
    }
    let route = route_of(cx, entry);
    // SAFETY: the app's function, called untracked.
    let children = unsafe { qjs::JS_GetPropertyStr(cx, props, c"children".as_ptr()) };
    let mut args = [route, index];
    let rendered = reactive::untracked_with(cx, || unsafe {
        qjs::JS_Call(cx, children, UNDEFINED, 2, args.as_mut_ptr())
    });
    free(cx, children);
    free(cx, route);
    if unsafe { qjs::JS_IsException(rendered) } {
        free(cx, entry);
        return rendered;
    }
    // SAFETY: the navigator's entries signal, owned.
    let navigator = unsafe { qjs::JS_GetPropertyStr(cx, props, c"state".as_ptr()) };
    let entries = unsafe { qjs::JS_GetPropertyStr(cx, navigator, ENTRIES.as_ptr()) };
    free(cx, navigator);
    let popped = native(cx, pop_route, 2, &mut [entries, entry]);
    free(cx, entries);
    free(cx, entry);
    // An accessor (e.g. a component made hot) may render another screen
    // later.
    let result =
        if unsafe { qjs::JS_IsFunction(cx, rendered) } && dom::node_id(cx, rendered).is_none() {
            let wrap = native(cx, screen_of_accessor, 0, &mut [rendered, popped]);
            reactive::memo(cx, wrap, false)
        } else {
            screen_for(cx, rendered, popped)
        };
    free(cx, rendered);
    free(cx, popped);
    result
}

/// The route entry a screen popped natively stands for: removes that entry.
unsafe extern "C" fn pop_route(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values (the entries signal, the entry); the entry
    // is the argument.
    let mut args = [unsafe { *data.add(1) }];
    unsafe { navigate(cx, UNDEFINED, 1, args.as_mut_ptr(), 6, data) }
}

unsafe extern "C" fn screen_of_accessor(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: two data values (the accessor, the popped callback).
    let (accessor, popped) = unsafe { (*data, *data.add(1)) };
    let rendered = unsafe { qjs::JS_Call(cx, accessor, UNDEFINED, 0, std::ptr::null_mut()) };
    if unsafe { qjs::JS_IsException(rendered) } {
        return rendered;
    }
    let r = reactive::untracked_with(cx, || screen_for(cx, rendered, popped));
    free(cx, rendered);
    r
}

/// The screen for what a route rendered (borrowed): itself when it is one,
/// else a screen around it; told about native pops (`popped`).
fn screen_for(cx: *mut JsContext, rendered: JsValue, popped: JsValue) -> JsValue {
    let node = if dom::node_tag(cx, rendered) == Some(Tag::Screen) {
        // SAFETY: a dup returned.
        unsafe { qjs::JS_DupValue(cx, rendered) }
    } else {
        // SAFETY: new props for a screen around the content.
        let props = unsafe { qjs::JS_NewObject(cx) };
        unsafe {
            qjs::JS_SetPropertyStr(
                cx,
                props,
                c"children".as_ptr(),
                qjs::JS_DupValue(cx, rendered),
            );
        }
        let mut args = [props];
        let node = unsafe { screen_component(cx, UNDEFINED, 1, args.as_mut_ptr()) };
        free(cx, props);
        node
    };
    // SAFETY: an inline tag check.
    if !unsafe { qjs::JS_IsException(node) } {
        dom::set_popped(cx, node, popped);
    }
    node
}

// --- Screen -----------------------------------------------------------------------

/// `<NavPage title … header>`: one page of a stack.
fn screen_export(cx: *mut JsContext) -> JsValue {
    reactive::new_function(cx, c"NavPage", screen_component, 1)
}

unsafe extern "C" fn screen_component(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let props = reactive::arg(argc, argv, 0);

    // Create the screen element
    let node = dom::create_element(cx, Tag::Screen);
    if qjs::JS_IsException(node) {
        return node;
    }

    // Spread all props except our special ones
    let skip = &[
        "children",
        "header",
        "headerLeft",
        "headerRight",
        "headerTitle",
        "onDismiss",
        "onBackPress",
        "onInsets",
    ];
    if let Err(e) = dom::spread_props_except(cx, node, props, skip) {
        free(cx, node);
        return e;
    }

    // Set up insets: create signal and handler
    let insets_signal = reactive::new_signal(cx, UNDEFINED);
    let insets_handler = {
        let mut data = [insets_signal];
        native(cx, on_insets_handler, 1, &mut data)
    };
    if let Err(e) = dom::set_prop_value(cx, node, "onInsets", insets_handler) {
        free(cx, insets_handler);
        free(cx, insets_signal);
        free(cx, node);
        return e;
    }
    free(cx, insets_handler);

    // A native pop always removes the route, whether the app listens.
    let on_dismiss = qjs::JS_GetPropertyStr(cx, props, c"onDismiss".as_ptr());
    let dismiss_handler = {
        let mut data = [on_dismiss, node];
        native(cx, on_dismiss_handler, 2, &mut data)
    };
    if let Err(e) = dom::set_prop_value(cx, node, "onDismiss", dismiss_handler) {
        free(cx, dismiss_handler);
        free(cx, on_dismiss);
        free(cx, insets_signal);
        free(cx, node);
        return e;
    }
    free(cx, dismiss_handler);
    free(cx, on_dismiss);

    // Set up onBackPress if provided
    let on_back = qjs::JS_GetPropertyStr(cx, props, c"onBackPress".as_ptr());
    if !qjs::JS_IsUndefined(on_back) && !qjs::JS_IsNull(on_back) {
        let back_handler = {
            let mut data = [on_back];
            native(cx, on_back_handler, 1, &mut data)
        };
        if let Err(e) = dom::set_prop_value(cx, node, "onBackPress", back_handler) {
            free(cx, back_handler);
            free(cx, on_back);
            free(cx, insets_signal);
            free(cx, node);
            return e;
        }
        free(cx, back_handler);
    }
    free(cx, on_back);

    // Create header slot items
    let slots = [
        ("headerLeft", "left"),
        ("headerRight", "right"),
        ("headerTitle", "title"),
        ("header", "header"),
    ];

    let mut slot_nodes = Vec::new();
    for (prop_key, slot_name) in &slots {
        let key_str = format!("{}\0", prop_key);
        let c_key = match CStr::from_bytes_with_nul(key_str.as_bytes()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let content = unsafe { qjs::JS_GetPropertyStr(cx, props, c_key.as_ptr()) };
        // SAFETY: content is a live JS value
        if !unsafe {
            qjs::JS_IsUndefined(content) || qjs::JS_IsNull(content) || qjs::JS_IsBool(content)
        } {
            let slot_fn = {
                let mut data = [content, new_string(cx, slot_name)];
                native(cx, header_slot_fn, 2, &mut data)
            };
            let slot_memo = reactive::memo(cx, slot_fn, false);
            slot_nodes.push(slot_memo);
        }
        free(cx, content);
    }

    // Wrap children with insets context provider
    let children = qjs::JS_GetPropertyStr(cx, props, c"children".as_ptr());
    let children_fn = {
        let mut data = [children];
        native(cx, unwrap_children, 1, &mut data)
    };
    free(cx, children);

    let insets_ctx = insets_context(cx);
    let provider_fn = qjs::JS_GetPropertyStr(cx, insets_ctx, c"Provider".as_ptr());
    free(cx, insets_ctx);

    let provider_props = qjs::JS_NewObject(cx);
    qjs::JS_SetPropertyStr(
        cx,
        provider_props,
        c"value".as_ptr(),
        qjs::JS_DupValue(cx, insets_signal),
    );
    qjs::JS_SetPropertyStr(cx, provider_props, c"children".as_ptr(), children_fn);

    let mut provider_args = [provider_props];
    let wrapped_children = qjs::JS_Call(cx, provider_fn, UNDEFINED, 1, provider_args.as_mut_ptr());
    free(cx, provider_fn);
    free(cx, provider_props);
    free(cx, insets_signal);

    if qjs::JS_IsException(wrapped_children) {
        for slot in slot_nodes {
            free(cx, slot);
        }
        free(cx, node);
        return wrapped_children;
    }

    // Insert all parts: slots + wrapped children
    slot_nodes.push(wrapped_children);
    if let Err(e) = insert_array(cx, node, slot_nodes) {
        free(cx, node);
        return e;
    }

    node
}

unsafe extern "C" fn on_insets_handler(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: data points to valid signal value owned by the function data
    let signal = unsafe { *data };
    // `set_signal` takes the value; the dispatcher frees the argument it
    // passed us, so hand it a duplicate.
    let payload = unsafe { qjs::JS_DupValue(cx, reactive::arg(argc, argv, 0)) };
    reactive::set_signal(cx, signal, payload);
    UNDEFINED
}

unsafe extern "C" fn on_dismiss_handler(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: data[0] = user callback, data[1] = node
    let callback = unsafe { *data };
    let node = unsafe { *data.add(1) };

    // First, call the popped callback to remove the route
    let popped = dom::popped(cx, node);
    if !unsafe { qjs::JS_IsUndefined(popped) } {
        let result = unsafe { qjs::JS_Call(cx, popped, UNDEFINED, 0, std::ptr::null_mut()) };
        if unsafe { qjs::JS_IsException(result) } {
            unsafe { qjs::JS_FreeValue(cx, result) };
        } else {
            unsafe { qjs::JS_FreeValue(cx, result) };
        }
        // Free the popped callback after calling it
        unsafe { qjs::JS_FreeValue(cx, popped) };
        dom::set_popped(cx, node, UNDEFINED);
    }

    // Then call the user's onDismiss callback
    if !unsafe { qjs::JS_IsFunction(cx, callback) } {
        return UNDEFINED;
    }
    let mut args = [];
    let result = unsafe { qjs::JS_Call(cx, callback, UNDEFINED, 0, args.as_mut_ptr()) };
    if unsafe { qjs::JS_IsException(result) } {
        unsafe { qjs::JS_FreeValue(cx, result) };
    }
    UNDEFINED
}

unsafe extern "C" fn on_back_handler(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: data points to valid callback value owned by the function data
    let callback = unsafe { *data };
    // SAFETY: cx is valid, callback is a live JS value
    if !unsafe { qjs::JS_IsFunction(cx, callback) } {
        return UNDEFINED;
    }
    let mut args = [];
    // SAFETY: cx and callback are valid, args pointer is valid
    let result = unsafe { qjs::JS_Call(cx, callback, UNDEFINED, 0, args.as_mut_ptr()) };
    // SAFETY: cx and result are valid
    if unsafe { qjs::JS_IsException(result) } {
        // SAFETY: cx and result are valid
        unsafe { qjs::JS_FreeValue(cx, result) };
    }
    UNDEFINED
}

unsafe extern "C" fn header_slot_fn(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: data points to valid array of JS values owned by the function data
    let content = unsafe { *data };
    let slot_name = unsafe { *data.add(1) };

    // SAFETY: cx is valid, content is a live JS value
    if unsafe {
        qjs::JS_IsUndefined(content)
            || qjs::JS_IsNull(content)
            || (qjs::JS_IsBool(content) && qjs::JS_ToBool(cx, content) == 0)
    } {
        return UNDEFINED;
    }

    reactive::untracked_with(cx, || {
        let item = dom::create_element(cx, Tag::HeaderItem);
        if qjs::JS_IsException(item) {
            return item;
        }

        if let Err(e) = dom::set_prop_value(cx, item, "headerSlot", slot_name) {
            free(cx, item);
            return e;
        }

        if let Err(e) = dom::insert_value_at(cx, item, content, None) {
            free(cx, item);
            return e;
        }

        item
    })
}

unsafe extern "C" fn unwrap_children(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    let children = *data;
    if qjs::JS_IsFunction(cx, children) {
        let mut args = [];
        qjs::JS_Call(cx, children, UNDEFINED, 0, args.as_mut_ptr())
    } else {
        qjs::JS_DupValue(cx, children)
    }
}

// --- Dialog -----------------------------------------------------------------------

/// `<Dialog open onOpenChange …>`: content over everything else.
fn modal_export(cx: *mut JsContext) -> JsValue {
    reactive::new_function(cx, c"Dialog", modal_component, 1)
}

unsafe extern "C" fn modal_component(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
) -> JsValue {
    let props = reactive::arg(argc, argv, 0);

    // Create state: closed_by_user flag, close_reported flag, and tick signal
    let state = qjs::JS_NewObject(cx);
    qjs::JS_SetPropertyStr(cx, state, c"closedByUser".as_ptr(), qjs::JS_FALSE);
    qjs::JS_SetPropertyStr(cx, state, c"closeReported".as_ptr(), qjs::JS_FALSE);
    let initial_tick = qjs::JS_NewFloat64(0.0);
    let tick = reactive::new_signal(cx, initial_tick);
    free(cx, initial_tick);
    qjs::JS_SetPropertyStr(cx, state, c"tick".as_ptr(), tick);

    let mut data = [props, state];
    let render_fn = native(cx, modal_render, 2, &mut data);
    free(cx, state);

    let memo = reactive::memo(cx, render_fn, false);
    reactive::finish_value(cx, memo)
}

unsafe extern "C" fn modal_render(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: data points to valid array of JS values owned by the function data
    let props = unsafe { *data };
    let state = unsafe { *data.add(1) };

    // Read tick to track it
    // SAFETY: cx and state are valid, c-string is valid
    let tick_signal = unsafe { qjs::JS_GetPropertyStr(cx, state, c"tick".as_ptr()) };
    // SAFETY: cx and tick_signal are valid, calling a signal accessor
    let _tick_value = unsafe { qjs::JS_Call(cx, tick_signal, UNDEFINED, 0, std::ptr::null_mut()) };
    free(cx, _tick_value);
    free(cx, tick_signal);

    // Check if open
    // SAFETY: cx and props are valid, c-string is valid
    let open = unsafe { qjs::JS_GetPropertyStr(cx, props, c"open".as_ptr()) };
    // SAFETY: cx and open are valid, calling a signal or reading a boolean
    let is_open = unsafe { qjs::JS_ToBool(cx, open) == 1 };
    free(cx, open);

    // Check closedByUser flag
    // SAFETY: cx and state are valid, c-string is valid
    let closed_by_user = unsafe { qjs::JS_GetPropertyStr(cx, state, c"closedByUser".as_ptr()) };
    // SAFETY: cx and closed_by_user are valid, reading a boolean
    let was_closed = unsafe { qjs::JS_ToBool(cx, closed_by_user) } == 1;
    free(cx, closed_by_user);

    // Check was_closed FIRST - if user closed, call onClose before unmounting,
    // regardless of whether open prop is true or false (controlled vs uncontrolled)
    if was_closed {
        // User closed the dialog - report onClose once before unmounting
        let close_reported =
            unsafe { qjs::JS_GetPropertyStr(cx, state, c"closeReported".as_ptr()) };
        let already_reported = unsafe { qjs::JS_ToBool(cx, close_reported) } == 1;
        free(cx, close_reported);

        if !already_reported {
            // Call onClose once before unmounting
            let on_close = unsafe { qjs::JS_GetPropertyStr(cx, props, c"onClose".as_ptr()) };
            if unsafe { qjs::JS_IsFunction(cx, on_close) } {
                // Set reported flag BEFORE calling to prevent re-entrant calls
                unsafe {
                    qjs::JS_SetPropertyStr(cx, state, c"closeReported".as_ptr(), qjs::JS_TRUE)
                };

                let mut args = [];
                let result = unsafe { qjs::JS_Call(cx, on_close, UNDEFINED, 0, args.as_mut_ptr()) };
                if !unsafe { qjs::JS_IsException(result) } {
                    free(cx, result);
                }
            }
            free(cx, on_close);
        }
        return UNDEFINED;
    }

    if !is_open {
        // Reset closed flag and closeReported when open becomes false programmatically
        // (not via user action)
        // SAFETY: cx and state are valid, setting boolean properties
        unsafe {
            qjs::JS_SetPropertyStr(cx, state, c"closedByUser".as_ptr(), qjs::JS_FALSE);
            qjs::JS_SetPropertyStr(cx, state, c"closeReported".as_ptr(), qjs::JS_FALSE);
        }
        return UNDEFINED;
    }

    reactive::untracked_with(cx, || {
        let node = dom::create_element(cx, Tag::Modal);
        // SAFETY: node is a live JS value
        if unsafe { qjs::JS_IsException(node) } {
            return node;
        }

        // Spread props except our special ones
        let skip = &[
            "children",
            "style",
            "class",
            "backdropStyle",
            "backdropClass",
            "backdropTestID",
            "open",
            "animation",
            "onOpen",
            "onClose",
            "onCancel",
            "onOpenChange",
            "onInsets",
        ];
        if let Err(e) = dom::spread_props_except(cx, node, props, skip) {
            free(cx, node);
            return e;
        }

        // Handle animation with reduceMotion check
        let has_backdrop_result = has_backdrop(cx, props);
        let animation_value = compute_animation(cx, props, has_backdrop_result);
        if let Err(e) = dom::set_prop_value(cx, node, "animation", animation_value) {
            free(cx, animation_value);
            free(cx, node);
            return e;
        }
        free(cx, animation_value);

        // Set up insets
        let insets_signal = reactive::new_signal(cx, UNDEFINED);
        let insets_handler = {
            let mut fdata = [insets_signal];
            native(cx, on_insets_handler, 1, &mut fdata)
        };
        if let Err(e) = dom::set_prop_value(cx, node, "onInsets", insets_handler) {
            free(cx, insets_handler);
            free(cx, insets_signal);
            free(cx, node);
            return e;
        }
        free(cx, insets_handler);

        // Set up onShow -> onOpen
        let on_open = qjs::JS_GetPropertyStr(cx, props, c"onOpen".as_ptr());
        if !qjs::JS_IsUndefined(on_open) && !qjs::JS_IsNull(on_open) {
            let show_handler = {
                let mut fdata = [on_open];
                native(cx, call_callback, 1, &mut fdata)
            };
            if let Err(e) = dom::set_prop_value(cx, node, "onShow", show_handler) {
                free(cx, show_handler);
                free(cx, on_open);
                free(cx, insets_signal);
                free(cx, node);
                return e;
            }
            free(cx, show_handler);
        }
        free(cx, on_open);

        // Set up onDismiss -> onClose
        let on_close = qjs::JS_GetPropertyStr(cx, props, c"onClose".as_ptr());
        if !qjs::JS_IsUndefined(on_close) && !qjs::JS_IsNull(on_close) {
            let dismiss_handler = {
                let mut fdata = [on_close];
                native(cx, call_callback, 1, &mut fdata)
            };
            if let Err(e) = dom::set_prop_value(cx, node, "onDismiss", dismiss_handler) {
                free(cx, dismiss_handler);
                free(cx, on_close);
                free(cx, insets_signal);
                free(cx, node);
                return e;
            }
            free(cx, dismiss_handler);
        }
        free(cx, on_close);

        // Set up onRequestClose with close logic
        let close_handler = {
            let mut fdata = [props, state];
            native(cx, on_request_close, 2, &mut fdata)
        };
        if let Err(e) = dom::set_prop_value(cx, node, "onRequestClose", close_handler) {
            free(cx, close_handler);
            free(cx, insets_signal);
            free(cx, node);
            return e;
        }
        free(cx, close_handler);

        // Create backdrop and popup
        let mut parts = Vec::new();
        if has_backdrop_result {
            let backdrop_fn = {
                let mut fdata = [props, state];
                native(cx, create_backdrop, 2, &mut fdata)
            };
            let backdrop_memo = reactive::memo(cx, backdrop_fn, false);
            parts.push(backdrop_memo);
        }

        let popup = create_popup(cx, props, insets_signal);
        if qjs::JS_IsException(popup) {
            for part in parts {
                free(cx, part);
            }
            free(cx, insets_signal);
            free(cx, node);
            return popup;
        }
        free(cx, insets_signal);

        parts.push(popup);
        if let Err(e) = insert_array(cx, node, parts) {
            free(cx, node);
            return e;
        }

        node
    })
}

fn has_backdrop(cx: *mut JsContext, props: JsValue) -> bool {
    unsafe {
        let backdrop_class = qjs::JS_GetPropertyStr(cx, props, c"backdropClass".as_ptr());
        let backdrop_style = qjs::JS_GetPropertyStr(cx, props, c"backdropStyle".as_ptr());
        let has = !qjs::JS_IsUndefined(backdrop_class) && !qjs::JS_IsNull(backdrop_class)
            || !qjs::JS_IsUndefined(backdrop_style) && !qjs::JS_IsNull(backdrop_style);
        free(cx, backdrop_class);
        free(cx, backdrop_style);
        has
    }
}

fn compute_animation(cx: *mut JsContext, props: JsValue, has_backdrop: bool) -> JsValue {
    unsafe {
        let animation_prop = qjs::JS_GetPropertyStr(cx, props, c"animation".as_ptr());
        let style_prop = qjs::JS_GetPropertyStr(cx, props, c"style".as_ptr());
        let style_has_translate = if !qjs::JS_IsUndefined(style_prop) && !qjs::JS_IsNull(style_prop)
        {
            let translate = qjs::JS_GetPropertyStr(cx, style_prop, c"translate".as_ptr());
            let has = !qjs::JS_IsUndefined(translate) && !qjs::JS_IsNull(translate);
            free(cx, translate);
            has
        } else {
            false
        };
        free(cx, style_prop);

        let should_slide = has_backdrop
            && (qjs::JS_IsUndefined(animation_prop)
                || qjs::JS_IsNull(animation_prop)
                || (qjs::JS_IsString(animation_prop) && {
                    let s = qjs::JS_ToCString(cx, animation_prop);
                    let is_slide = !s.is_null() && CStr::from_ptr(s).to_bytes() == b"slide";
                    qjs::JS_FreeCString(cx, s);
                    is_slide
                }))
            && !style_has_translate
            && !reduce_motion(cx);

        if should_slide {
            let result = new_string(cx, "fade");
            free(cx, animation_prop);
            result
        } else {
            animation_prop
        }
    }
}

fn create_popup(cx: *mut JsContext, props: JsValue, insets_signal: JsValue) -> JsValue {
    unsafe {
        let popup = dom::create_element(cx, Tag::View);
        if qjs::JS_IsException(popup) {
            return popup;
        }

        let class_prop = qjs::JS_GetPropertyStr(cx, props, c"class".as_ptr());
        if !qjs::JS_IsUndefined(class_prop) && !qjs::JS_IsNull(class_prop) {
            let class_fn = {
                let mut fdata = [class_prop];
                native(cx, dup_value, 1, &mut fdata)
            };
            if let Err(e) = dom::set_prop_value(cx, popup, "class", class_fn) {
                free(cx, class_fn);
                free(cx, class_prop);
                free(cx, popup);
                return e;
            }
            free(cx, class_fn);
        }
        free(cx, class_prop);

        let style_prop = qjs::JS_GetPropertyStr(cx, props, c"style".as_ptr());
        if !qjs::JS_IsUndefined(style_prop) && !qjs::JS_IsNull(style_prop) {
            let style_fn = {
                let mut fdata = [style_prop];
                native(cx, dup_value, 1, &mut fdata)
            };
            if let Err(e) = dom::set_prop_value(cx, popup, "style", style_fn) {
                free(cx, style_fn);
                free(cx, style_prop);
                free(cx, popup);
                return e;
            }
            free(cx, style_fn);
        }
        free(cx, style_prop);

        // Wrap children with insets context
        let children = qjs::JS_GetPropertyStr(cx, props, c"children".as_ptr());
        let children_fn = {
            let mut fdata = [children];
            native(cx, unwrap_children, 1, &mut fdata)
        };
        free(cx, children);

        let insets_ctx = insets_context(cx);
        let provider_fn = qjs::JS_GetPropertyStr(cx, insets_ctx, c"Provider".as_ptr());
        free(cx, insets_ctx);

        let provider_props = qjs::JS_NewObject(cx);
        qjs::JS_SetPropertyStr(
            cx,
            provider_props,
            c"value".as_ptr(),
            qjs::JS_DupValue(cx, insets_signal),
        );
        qjs::JS_SetPropertyStr(cx, provider_props, c"children".as_ptr(), children_fn);

        let mut provider_args = [provider_props];
        let wrapped_children =
            qjs::JS_Call(cx, provider_fn, UNDEFINED, 1, provider_args.as_mut_ptr());
        free(cx, provider_fn);
        free(cx, provider_props);

        if qjs::JS_IsException(wrapped_children) {
            free(cx, popup);
            return wrapped_children;
        }

        if let Err(e) = dom::insert_value_at(cx, popup, wrapped_children, None) {
            free(cx, wrapped_children);
            free(cx, popup);
            return e;
        }
        free(cx, wrapped_children);

        popup
    }
}

unsafe extern "C" fn on_request_close(
    cx: *mut JsContext,
    _this: JsValue,
    argc: c_int,
    argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    let props = *data;
    let state = *data.add(1);

    let event = reactive::arg(argc, argv, 0);
    // A core-emitted close (back, Escape) carries no reason: "cancel".
    let reason_val = if unsafe { qjs::JS_IsObject(event) } {
        qjs::JS_GetPropertyStr(cx, event, c"reason".as_ptr())
    } else {
        UNDEFINED
    };
    let reason_cstr = if qjs::JS_IsUndefined(reason_val) {
        std::ptr::null()
    } else {
        qjs::JS_ToCString(cx, reason_val)
    };
    let reason_bytes = if !reason_cstr.is_null() {
        CStr::from_ptr(reason_cstr).to_bytes()
    } else {
        b"cancel"
    };

    let reason = if reason_bytes == b"backdropPress" || reason_bytes == b"swipe" {
        reason_bytes
    } else {
        b"cancel"
    };

    if !reason_cstr.is_null() {
        qjs::JS_FreeCString(cx, reason_cstr);
    }
    free(cx, reason_val);

    // Call onCancel
    let on_cancel = qjs::JS_GetPropertyStr(cx, props, c"onCancel".as_ptr());
    if qjs::JS_IsFunction(cx, on_cancel) {
        let mut args = [];
        let result = qjs::JS_Call(cx, on_cancel, UNDEFINED, 0, args.as_mut_ptr());
        if !qjs::JS_IsException(result) {
            free(cx, result);
        }
    }
    free(cx, on_cancel);

    // Check flag prop
    let flag_name = if reason == b"backdropPress" {
        c"closeOnBackdropPress"
    } else if reason == b"swipe" {
        c"closeOnSwipe"
    } else {
        c"closeOnEscape"
    };

    let flag_prop = qjs::JS_GetPropertyStr(cx, props, flag_name.as_ptr());
    let should_close = if qjs::JS_IsBool(flag_prop) {
        qjs::JS_ToBool(cx, flag_prop) == 1
    } else {
        true
    };
    free(cx, flag_prop);

    let closed_by_user = qjs::JS_GetPropertyStr(cx, state, c"closedByUser".as_ptr());
    let already_closed = qjs::JS_ToBool(cx, closed_by_user) == 1;
    free(cx, closed_by_user);

    if !should_close || already_closed {
        return UNDEFINED;
    }

    // Call onOpenChange
    let on_open_change = qjs::JS_GetPropertyStr(cx, props, c"onOpenChange".as_ptr());
    if qjs::JS_IsFunction(cx, on_open_change) {
        let change_event = qjs::JS_NewObject(cx);
        qjs::JS_SetPropertyStr(cx, change_event, c"open".as_ptr(), qjs::JS_FALSE);
        let reason_str = new_string(cx, std::str::from_utf8(reason).unwrap_or("cancel"));
        qjs::JS_SetPropertyStr(cx, change_event, c"reason".as_ptr(), reason_str);
        let mut args = [change_event];
        let result = qjs::JS_Call(cx, on_open_change, UNDEFINED, 1, args.as_mut_ptr());
        if !qjs::JS_IsException(result) {
            free(cx, result);
        }
        free(cx, change_event);
    }
    free(cx, on_open_change);

    // Set closed flag and bump tick
    qjs::JS_SetPropertyStr(cx, state, c"closedByUser".as_ptr(), qjs::JS_TRUE);

    let tick_signal = qjs::JS_GetPropertyStr(cx, state, c"tick".as_ptr());
    let current = reactive::peek(cx, tick_signal);
    let current_int = qjs::JS_VALUE_GET_INT(current);
    let next = qjs::JS_NewFloat64((current_int + 1) as f64);
    reactive::set_signal(cx, tick_signal, next);
    free(cx, next);
    free(cx, current);
    free(cx, tick_signal);

    UNDEFINED
}

unsafe extern "C" fn create_backdrop(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    let props = *data;
    let state = *data.add(1);

    if !has_backdrop(cx, props) {
        return UNDEFINED;
    }

    reactive::untracked_with(cx, || {
        let view = dom::create_element(cx, Tag::View);
        // SAFETY: view is a live JS value
        if unsafe { qjs::JS_IsException(view) } {
            return view;
        }

        // Set class with absolute inset-0 prefix
        let backdrop_class = qjs::JS_GetPropertyStr(cx, props, c"backdropClass".as_ptr());
        let class_fn = {
            let mut fdata = [backdrop_class];
            native(cx, backdrop_class_fn, 1, &mut fdata)
        };
        if let Err(e) = dom::set_prop_value(cx, view, "class", class_fn) {
            free(cx, class_fn);
            free(cx, backdrop_class);
            free(cx, view);
            return e;
        }
        free(cx, class_fn);
        free(cx, backdrop_class);

        // Set style
        let backdrop_style = qjs::JS_GetPropertyStr(cx, props, c"backdropStyle".as_ptr());
        if !qjs::JS_IsUndefined(backdrop_style) && !qjs::JS_IsNull(backdrop_style) {
            let style_fn = {
                let mut fdata = [backdrop_style];
                native(cx, dup_value, 1, &mut fdata)
            };
            if let Err(e) = dom::set_prop_value(cx, view, "style", style_fn) {
                free(cx, style_fn);
                free(cx, backdrop_style);
                free(cx, view);
                return e;
            }
            free(cx, style_fn);
        }
        free(cx, backdrop_style);

        // Set testID
        let test_id = qjs::JS_GetPropertyStr(cx, props, c"backdropTestID".as_ptr());
        if !qjs::JS_IsUndefined(test_id) && !qjs::JS_IsNull(test_id) {
            let test_fn = {
                let mut fdata = [test_id];
                native(cx, dup_value, 1, &mut fdata)
            };
            if let Err(e) = dom::set_prop_value(cx, view, "testID", test_fn) {
                free(cx, test_fn);
                free(cx, test_id);
                free(cx, view);
                return e;
            }
            free(cx, test_fn);
        }
        free(cx, test_id);

        // Set onPress handler with state
        let press_handler = {
            let mut fdata = [props, state];
            native(cx, on_backdrop_press, 2, &mut fdata)
        };
        if let Err(e) = dom::set_prop_value(cx, view, "onPress", press_handler) {
            free(cx, press_handler);
            free(cx, view);
            return e;
        }
        free(cx, press_handler);

        view
    })
}

unsafe extern "C" fn backdrop_class_fn(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: data points to valid class value
    let class_val = unsafe { *data };

    // SAFETY: cx and class_val are valid
    if unsafe { qjs::JS_IsString(class_val) } {
        // SAFETY: cx and class_val are valid
        let class_str = unsafe { qjs::JS_ToCString(cx, class_val) };
        if !class_str.is_null() {
            // SAFETY: class_str is a valid C string
            let combined = unsafe {
                format!(
                    "absolute inset-0 {}",
                    CStr::from_ptr(class_str).to_string_lossy()
                )
            };
            // SAFETY: cx and class_str are valid
            unsafe { qjs::JS_FreeCString(cx, class_str) };
            return new_string(cx, &combined);
        }
    }

    new_string(cx, "absolute inset-0")
}

unsafe extern "C" fn on_backdrop_press(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    let props = *data;
    let state = *data.add(1);

    // Call onCancel
    let on_cancel = qjs::JS_GetPropertyStr(cx, props, c"onCancel".as_ptr());
    if qjs::JS_IsFunction(cx, on_cancel) {
        let mut args = [];
        let result = qjs::JS_Call(cx, on_cancel, UNDEFINED, 0, args.as_mut_ptr());
        if !qjs::JS_IsException(result) {
            free(cx, result);
        }
    }
    free(cx, on_cancel);

    // Check if should close
    // SAFETY: cx and props are valid, c-string is valid
    let flag = unsafe { qjs::JS_GetPropertyStr(cx, props, c"closeOnBackdropPress".as_ptr()) };
    // SAFETY: flag is a live JS value
    let should_close = if unsafe { qjs::JS_IsBool(flag) } {
        // SAFETY: cx and flag are valid
        unsafe { qjs::JS_ToBool(cx, flag) == 1 }
    } else {
        true
    };
    free(cx, flag);

    if !should_close {
        return UNDEFINED;
    }

    // Call onOpenChange
    let on_change = qjs::JS_GetPropertyStr(cx, props, c"onOpenChange".as_ptr());
    if qjs::JS_IsFunction(cx, on_change) {
        let event = qjs::JS_NewObject(cx);
        qjs::JS_SetPropertyStr(cx, event, c"open".as_ptr(), qjs::JS_FALSE);
        let reason = new_string(cx, "backdropPress");
        qjs::JS_SetPropertyStr(cx, event, c"reason".as_ptr(), reason);
        let mut args = [event];
        let result = qjs::JS_Call(cx, on_change, UNDEFINED, 1, args.as_mut_ptr());
        if !qjs::JS_IsException(result) {
            free(cx, result);
        }
        free(cx, event);
    }
    free(cx, on_change);

    // Set closed flag and bump tick (same as on_request_close)
    qjs::JS_SetPropertyStr(cx, state, c"closedByUser".as_ptr(), qjs::JS_TRUE);

    let tick_signal = qjs::JS_GetPropertyStr(cx, state, c"tick".as_ptr());
    let current = reactive::peek(cx, tick_signal);
    let current_int = qjs::JS_VALUE_GET_INT(current);
    let next = qjs::JS_NewFloat64((current_int + 1) as f64);
    reactive::set_signal(cx, tick_signal, next);
    free(cx, next);
    free(cx, current);
    free(cx, tick_signal);

    UNDEFINED
}

unsafe extern "C" fn call_callback(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    let callback = *data;
    if qjs::JS_IsFunction(cx, callback) {
        let mut args = [];
        let result = qjs::JS_Call(cx, callback, UNDEFINED, 0, args.as_mut_ptr());
        if !qjs::JS_IsException(result) {
            free(cx, result);
        }
    }
    UNDEFINED
}

unsafe extern "C" fn dup_value(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: c_int,
    _argv: *mut JsValue,
    _magic: c_int,
    data: *mut JsValue,
) -> JsValue {
    qjs::JS_DupValue(cx, *data)
}

// --- Helpers ----------------------------------------------------------------------

fn new_array(cx: *mut JsContext, items: Vec<JsValue>) -> JsValue {
    // SAFETY: creating array and setting elements (owned).
    unsafe {
        let arr = qjs::JS_NewArray(cx);
        for (i, v) in items.into_iter().enumerate() {
            qjs::JS_SetPropertyUint32(cx, arr, i as u32, v);
        }
        arr
    }
}

fn array_items(cx: *mut JsContext, arr: JsValue) -> Vec<JsValue> {
    let mut result = Vec::new();
    // SAFETY: reading array elements (owned).
    unsafe {
        if !qjs::JS_IsArray(arr) {
            return result;
        }
        let mut len: i64 = 0;
        if qjs::JS_GetLength(cx, arr, &mut len) != 0 {
            return result;
        }
        for i in 0..len.max(0) as u32 {
            result.push(qjs::JS_GetPropertyUint32(cx, arr, i));
        }
    }
    result
}

fn free_values(cx: *mut JsContext, values: Vec<JsValue>) {
    for v in values {
        free(cx, v);
    }
}

fn strict_equal(cx: *mut JsContext, a: JsValue, b: JsValue) -> bool {
    // SAFETY: an identity comparison of live values.
    unsafe { qjs::JS_IsStrictEqual(cx, a, b) }
}

/// Gets the `reduceMotion` environment bit (internal only, not exported).
pub(crate) fn reduce_motion(cx: *mut JsContext) -> bool {
    // Read from State's reduce_motion signal
    unsafe {
        let state = crate::state::state(cx);
        let signal = (*state).reduce_motion.get();
        if qjs::JS_IsUndefined(signal) {
            return false;
        }
        // Call the signal to get its value
        let value = qjs::JS_Call(cx, signal, qjs::JS_UNDEFINED, 0, std::ptr::null_mut());
        let result = qjs::JS_ToBool(cx, value);
        qjs::JS_FreeValue(cx, value);
        result == 1
    }
}

fn insets_context(cx: *mut JsContext) -> JsValue {
    unsafe {
        let default = qjs::JS_NewObject(cx);
        qjs::JS_SetPropertyStr(cx, default, c"top".as_ptr(), qjs::JS_NewFloat64(0.0));
        qjs::JS_SetPropertyStr(cx, default, c"right".as_ptr(), qjs::JS_NewFloat64(0.0));
        qjs::JS_SetPropertyStr(cx, default, c"bottom".as_ptr(), qjs::JS_NewFloat64(0.0));
        qjs::JS_SetPropertyStr(cx, default, c"left".as_ptr(), qjs::JS_NewFloat64(0.0));

        let mut args = [default];
        let ctx = dom::context(cx, UNDEFINED, 1, args.as_mut_ptr());
        free(cx, default);
        ctx
    }
}

fn insert_array(cx: *mut JsContext, parent: JsValue, items: Vec<JsValue>) -> Result<(), JsValue> {
    let arr = new_array(cx, items);
    let result = dom::insert_value_at(cx, parent, arr, None);
    free(cx, arr);
    result
}

fn new_string(cx: *mut JsContext, s: &str) -> JsValue {
    unsafe {
        let bytes = s.as_bytes();
        qjs::JS_NewStringLen(cx, bytes.as_ptr().cast(), bytes.len() as _)
    }
}
