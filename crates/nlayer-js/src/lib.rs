//! QuickJS host for nlayer.
//!
//! [`Host`] owns one QuickJS runtime and context and serves the native
//! modules an app imports (`@nlayer/runtime`: the reactive graph, `mount`,
//! `context` and `ErrorBoundary`; `@nlayer/ui`: the host components and
//! `platform.os`; `nlayer:rt`: the JSX runtime compiled code calls). A host
//! is single-threaded (`!Send`): create and use it on the engine thread.

mod bridge;
mod clock;
mod dom;
mod error;
mod list;
mod modules;
mod net;
mod protocol;
#[cfg(all(target_arch = "wasm32", getrandom_backend = "custom"))]
mod random;
mod reactive;
mod resource;
mod standard;
mod state;
mod storage;
mod url;

use std::time::Duration;

use nlayer_core::{EventKind, NodeId, Op};
use rquickjs::qjs;
use rquickjs::{CatchResultExt, Context, Module, Persistent, Promise, Runtime};

pub use crate::bridge::TestBridge;
pub use crate::error::JsError;
pub use crate::modules::native_module_exports;
pub use crate::net::{HttpRequest, HttpResponse, NetworkOp, NetworkRequest};

/// What a native view reports to JS with an event.
#[derive(Debug, Clone, PartialEq)]
pub enum EventPayload {
    /// Nothing (`onPress`).
    None,
    /// The input's text (`onChange`).
    Text(String),
    /// The selection or caret of an input, in UTF-16 units
    /// (`onSelectionChange`).
    Selection {
        /// Start of the range.
        start: u32,
        /// End of the range (the caret when equal to `start`).
        end: u32,
    },
    /// An input method's composition (`onCompositionChange`).
    Composition {
        /// A composition is in progress; `text` is what is being composed.
        composing: bool,
        /// The composed text, empty once the composition ended.
        text: String,
    },
    /// Where a scroll view is, in points. `x` and `y` are the content
    /// offset (negative while iOS bounces). `content_w` and `content_h` are
    /// the content size. `width` and `height` are the visible size.
    Scroll {
        /// Content offset x.
        x: f32,
        /// Content offset y.
        y: f32,
        /// Content width.
        content_w: f32,
        /// Content height.
        content_h: f32,
        /// Visible width.
        width: f32,
        /// Visible height.
        height: f32,
    },
}

/// The operating system a surface runs on (`platform.os`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Os {
    /// A browser page.
    #[default]
    Web,
    /// iOS.
    Ios,
    /// Android.
    Android,
}

impl Os {
    /// `platform.os`: `"web"`, `"ios"` or `"android"`.
    pub const fn name(self) -> &'static str {
        match self {
            Os::Web => "web",
            Os::Ios => "ios",
            Os::Android => "android",
        }
    }
}

/// Name of the app bundle's module (stack traces, imports it awaits).
pub const BUNDLE_NAME: &str = "bundle.js";

/// A QuickJS VM running one app.
pub struct Host {
    rt: Runtime,
    cx: Context,
}

impl Host {
    /// Makes a VM with the native modules installed.
    pub fn new() -> Result<Host, JsError> {
        let rt = Runtime::new()?;
        rt.set_loader(modules::Resolver, modules::Loader);
        let cx = Context::full(&rt)?;
        let installed = cx.with(|cx| -> Result<(), JsError> {
            state::install(&cx);
            dom::install(&cx);
            standard::install(&cx);
            Ok(())
        });
        installed?;
        Ok(Host { rt, cx })
    }

    /// The app's key-value storage (`storage` of `@nlayer/runtime`). Set
    /// it before the app runs; unset, it is in memory for the VM's life.
    pub fn set_storage(&self, storage: nlayer_core::storage::Store) {
        self.cx.with(|cx| {
            // SAFETY: the state `install` made for this context.
            unsafe { (*state::state(cx.as_raw().as_ptr())).storage = Some(storage) }
        });
    }

    /// The OS [`Os::name`] `platform.os` reports. Set it before the app
    /// imports `@nlayer/ui`: the object is made then, and does not change.
    /// Unset, it is [`Os::Web`].
    pub fn set_os(&self, os: Os) {
        self.cx.with(|cx| {
            // SAFETY: the state `install` made for this context.
            unsafe { (*state::state(cx.as_raw().as_ptr())).os.set(os) }
        });
    }

    /// The system color scheme `platform.colorScheme()` reports. Set it
    /// before the app runs, and again whenever the system changes: styles
    /// that read it are laid out again.
    pub fn set_color_scheme(&self, scheme: nlayer_core::ColorScheme) {
        self.cx.with(|cx| {
            let raw = cx.as_raw().as_ptr();
            // SAFETY: the state `install` made for this context, and the
            // signal it owns; the write takes the new value.
            unsafe {
                let signal = (*state::state(raw)).color_scheme.get();
                let name = scheme.name();
                let value = qjs::JS_NewStringLen(raw, name.as_ptr().cast(), name.len() as _);
                reactive::set_signal(raw, signal, value);
            }
        });
    }

    /// Delivers the platform's back button (Android) or history back (a
    /// page that pushed an entry): whether a `BackHandler` listener
    /// handled it. Listeners run newest first.
    pub fn dispatch_back(&self) -> bool {
        let handled = self.cx.with(|cx| {
            let raw = cx.as_raw().as_ptr();
            // SAFETY: the prelude's global, called on the live context; the
            // function and its result are freed here.
            unsafe {
                let global = qjs::JS_GetGlobalObject(raw);
                let f = qjs::JS_GetPropertyStr(raw, global, c"__nlayerDispatchBack".as_ptr());
                qjs::JS_FreeValue(raw, global);
                if !qjs::JS_IsFunction(raw, f) {
                    qjs::JS_FreeValue(raw, f);
                    return false;
                }
                let value = qjs::JS_Call(raw, f, qjs::JS_UNDEFINED, 0, std::ptr::null_mut());
                qjs::JS_FreeValue(raw, f);
                if qjs::JS_IsException(value) {
                    let error = JsError::from_engine(&cx, rquickjs::Error::Exception);
                    log::error!("{}", error.message);
                    return false;
                }
                let handled = qjs::JS_ToBool(raw, value) == 1;
                qjs::JS_FreeValue(raw, value);
                handled
            }
        });
        self.run_jobs();
        handled
    }

    /// Evaluates the app bundle (an ES module) and runs its jobs: its
    /// synchronous part and what its microtasks reach.
    pub fn eval_source(&self, source: &str) -> Result<(), JsError> {
        let promise = self.cx.with(|cx| -> Result<_, JsError> {
            let module = Module::declare(cx.clone(), BUNDLE_NAME, source)
                .catch(&cx)
                .map_err(JsError::caught)?;
            let (_, promise) = module.eval().catch(&cx).map_err(JsError::caught)?;
            Ok(Persistent::save(&cx, promise))
        })?;
        self.settle(promise)
    }

    /// Renders the cells list `list` needs, in `(slot, item)` order.
    /// Returns false when no such list is live.
    pub fn bind_list(&self, list: NodeId, binds: &[(u32, u32)]) -> bool {
        self.cx
            .with(|cx| crate::list::bind(cx.as_raw().as_ptr(), list, binds))
    }

    /// Moves the changes JS made since the last call into `out`.
    pub fn take_ops(&self, out: &mut Vec<Op>) {
        self.cx.with(|cx| {
            let d = dom::of(&cx);
            d.take_ops(out);
            dom::take_releases(d, out);
        });
    }

    /// Calls node `id`'s handler of `event` with `payload`, then runs its
    /// jobs. A listener's failure goes to the error boundary above the
    /// node, else it is reported.
    pub fn dispatch_event(&self, id: NodeId, event: EventKind, payload: &EventPayload) {
        let result = self
            .cx
            .with(|cx| dom::dispatch(cx.as_raw().as_ptr(), id, event, payload));
        if let Err(_e) = result {
            let error = self
                .cx
                .with(|cx| JsError::from_engine(&cx, rquickjs::Error::Exception));
            log::error!("{}", error.message);
        }
        self.run_jobs();
    }

    /// Moves the network work `fetch` queued into `out`.
    pub fn take_network(&self, out: &mut Vec<NetworkOp>) {
        self.cx.with(|cx| {
            // SAFETY: the live state.
            unsafe { (*state::state(cx.as_raw().as_ptr())).net.take(out) }
        });
    }

    /// Hands request `id` its response, resolving the promise JS made.
    pub fn complete_network(&self, id: u32, result: Result<HttpResponse, String>) {
        self.cx.with(|cx| {
            let raw = cx.as_raw().as_ptr();
            // SAFETY: the globals the prelude made; the response object is
            // new and taken by the call.
            unsafe {
                let global = qjs::JS_GetGlobalObject(raw);
                let done = qjs::JS_GetPropertyStr(raw, global, c"__nlayerFetchDone".as_ptr());
                if !qjs::JS_IsFunction(raw, done) {
                    qjs::JS_FreeValue(raw, done);
                    qjs::JS_FreeValue(raw, global);
                    return;
                }
                let error = match &result {
                    Ok(_) => qjs::JS_NULL,
                    Err(message) => {
                        qjs::JS_NewStringLen(raw, message.as_ptr().cast(), message.len() as _)
                    }
                };
                let response = match result {
                    Ok(response) => network_response(raw, &response),
                    Err(_) => qjs::JS_NULL,
                };
                let mut args = [qjs::JS_NewFloat64(id as f64), error, response];
                let r = qjs::JS_Call(raw, done, qjs::JS_UNDEFINED, 3, args.as_mut_ptr());
                if qjs::JS_IsException(r) {
                    let e = qjs::JS_GetException(raw);
                    let text = qjs::JS_ToCStringLen(raw, std::ptr::null_mut(), e);
                    if !text.is_null() {
                        log::error!(
                            "fetch: {}",
                            std::ffi::CStr::from_ptr(text).to_string_lossy()
                        );
                        qjs::JS_FreeCString(raw, text);
                    }
                    qjs::JS_FreeValue(raw, e);
                } else {
                    qjs::JS_FreeValue(raw, r);
                }
                for value in args {
                    qjs::JS_FreeValue(raw, value);
                }
                qjs::JS_FreeValue(raw, done);
                qjs::JS_FreeValue(raw, global);
            }
        });
        self.run_jobs();
    }

    /// How long until the first timer is due (`None`: none waits).
    pub fn next_timer(&self) -> Option<Duration> {
        self.cx.with(|cx| {
            // SAFETY: the live state.
            unsafe { (*state::state(cx.as_raw().as_ptr())).timers.next() }
        })
    }

    /// Runs the timers due now.
    pub fn run_due_timers(&self) {
        let due = self.cx.with(|cx| {
            // SAFETY: the live state.
            unsafe { (*state::state(cx.as_raw().as_ptr())).timers.run_due() }
        });
        if due.is_empty() {
            return;
        }
        self.cx.with(|cx| {
            let raw = cx.as_raw().as_ptr();
            // SAFETY: the globals the prelude made; each call's exception is
            // reported and cleared.
            unsafe {
                let global = qjs::JS_GetGlobalObject(raw);
                let fire = qjs::JS_GetPropertyStr(raw, global, c"__nlayerFireTimer".as_ptr());
                if qjs::JS_IsFunction(raw, fire) {
                    for id in due {
                        let mut args = [qjs::JS_NewFloat64(id as f64)];
                        let r = qjs::JS_Call(raw, fire, qjs::JS_UNDEFINED, 1, args.as_mut_ptr());
                        if qjs::JS_IsException(r) {
                            let e = qjs::JS_GetException(raw);
                            let text = qjs::JS_ToCStringLen(raw, std::ptr::null_mut(), e);
                            if !text.is_null() {
                                log::error!(
                                    "timer: {}",
                                    std::ffi::CStr::from_ptr(text).to_string_lossy()
                                );
                                qjs::JS_FreeCString(raw, text);
                            }
                            qjs::JS_FreeValue(raw, e);
                        } else {
                            qjs::JS_FreeValue(raw, r);
                        }
                        qjs::JS_FreeValue(raw, args[0]);
                    }
                }
                qjs::JS_FreeValue(raw, fire);
                qjs::JS_FreeValue(raw, global);
            }
        });
        self.run_jobs();
    }

    /// Runs the collector now: unreachable nodes are released in the next
    /// batch.
    pub fn collect_garbage(&self) {
        self.rt.run_gc();
    }

    /// Runs pending promise jobs and microtasks until the queue is empty.
    /// Every call from Rust into JS ends with it: it is the end of the turn.
    pub fn run_jobs(&self) {
        while let Ok(true) = self.rt.execute_pending_job() {}
    }

    /// Runs jobs until `promise` settles (or has no jobs left to run), then
    /// reports its failure.
    fn settle(&self, promise: Persistent<Promise<'static>>) -> Result<(), JsError> {
        self.run_jobs();
        self.cx.with(|cx| -> Result<(), JsError> {
            let promise = promise
                .restore(&cx)
                .map_err(|e| JsError::from_engine(&cx, e))?;
            match promise.result::<()>() {
                Some(result) => result.catch(&cx).map_err(JsError::caught),
                // Waiting on native work; the engine's next turn settles it.
                None => Ok(()),
            }
        })
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.cx.with(|cx| state::uninstall(&cx));
    }
}

/// The JS object `__nlayerFetchDone` reads a response from (owned).
fn network_response(raw: *mut qjs::JSContext, response: &HttpResponse) -> qjs::JSValue {
    // SAFETY: new values the object takes; every intermediate is freed.
    unsafe {
        let object = qjs::JS_NewObject(raw);
        let string = |text: &str| qjs::JS_NewStringLen(raw, text.as_ptr().cast(), text.len() as _);
        qjs::JS_SetPropertyStr(
            raw,
            object,
            c"status".as_ptr(),
            qjs::JS_NewFloat64(response.status as f64),
        );
        qjs::JS_SetPropertyStr(
            raw,
            object,
            c"statusText".as_ptr(),
            string(&response.status_text),
        );
        qjs::JS_SetPropertyStr(raw, object, c"url".as_ptr(), string(&response.url));
        qjs::JS_SetPropertyStr(
            raw,
            object,
            c"redirected".as_ptr(),
            if response.redirected {
                qjs::JS_TRUE
            } else {
                qjs::JS_FALSE
            },
        );
        let headers = qjs::JS_NewArray(raw);
        for (i, (name, value)) in response.headers.iter().enumerate() {
            let mut pair = [string(name), string(value)];
            let entry = qjs::JS_NewArrayFrom(raw, 2, pair.as_mut_ptr());
            qjs::JS_SetPropertyUint32(raw, headers, i as u32, entry);
        }
        qjs::JS_SetPropertyStr(raw, object, c"headers".as_ptr(), headers);
        let body = qjs::JS_NewUint8ArrayCopy(raw, response.body.as_ptr(), response.body.len() as _);
        qjs::JS_SetPropertyStr(raw, object, c"body".as_ptr(), body);
        object
    }
}
