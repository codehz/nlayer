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

use crate::reactive::Graph;

pub(crate) struct State {
    pub(crate) graph: Graph,
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
        let state = Box::into_raw(Box::new(State { graph }));
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
        crate::reactive::free_all(&mut state.graph, raw);
    }
}
