//! The call `@nlayer/test` makes into the runner (`globalThis.__nlayerTest`).
//! An app's VM never installs it.

use std::{cell::RefCell, ffi::CString};

use rquickjs::qjs;

use crate::{Host, standard};

type JsContext = qjs::JSContext;
type JsValue = qjs::JSValue;

/// One call from `@nlayer/test`: an op and a JSON payload, returning JSON
/// or an error the call throws.
pub type TestBridge = Box<dyn Fn(&str, &str) -> Result<String, String>>;

thread_local! {
    static BRIDGE: RefCell<Option<TestBridge>> = const { RefCell::new(None) };
}

impl Host {
    /// Installs `bridge` as `globalThis.__nlayerTest`. One bridge per thread:
    /// `nlayer test` sets it on the test file's VM and clears it after. An
    /// app's VM never sets it, so the global is absent there.
    pub fn set_test_bridge(&self, bridge: TestBridge) {
        BRIDGE.with(|slot| *slot.borrow_mut() = Some(bridge));
        self.cx.with(|cx| {
            let raw = cx.as_raw().as_ptr();
            let func = crate::reactive::new_function(raw, c"__nlayerTest", test_bridge, 2);
            standard::set_global(raw, c"__nlayerTest", func);
        });
    }

    /// Drops the bridge [`Self::set_test_bridge`] installed on this thread.
    pub fn clear_test_bridge(&self) {
        BRIDGE.with(|slot| *slot.borrow_mut() = None);
    }

    /// Sets the string global `name` (the test filter).
    pub fn set_global_string(&self, name: &str, value: &str) {
        let Ok(name) = CString::new(name) else {
            return;
        };
        self.cx.with(|cx| {
            let raw = cx.as_raw().as_ptr();
            // SAFETY: a new string the global object takes.
            unsafe {
                let js = qjs::JS_NewStringLen(raw, value.as_ptr().cast(), value.len() as _);
                let global = qjs::JS_GetGlobalObject(raw);
                qjs::JS_SetPropertyStr(raw, global, name.as_ptr(), js);
                qjs::JS_FreeValue(raw, global);
            }
        });
    }

    /// The string global `name`, if that global is a string.
    pub fn global_string(&self, name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        self.cx.with(|cx| {
            let raw = cx.as_raw().as_ptr();
            // SAFETY: the global is borrowed and freed; the string is copied.
            unsafe {
                let global = qjs::JS_GetGlobalObject(raw);
                let value = qjs::JS_GetPropertyStr(raw, global, name.as_ptr());
                qjs::JS_FreeValue(raw, global);
                let text = js_string(raw, value);
                qjs::JS_FreeValue(raw, value);
                text
            }
        })
    }
}

fn js_string(cx: *mut JsContext, value: JsValue) -> Option<String> {
    // SAFETY: `value` is live; a non-string is left untouched.
    unsafe {
        if !qjs::JS_IsString(value) {
            return None;
        }
        standard::string(cx, value)
    }
}

/// `__nlayerTest(op, payload)`: `payload` is JSON, the return is JSON.
unsafe extern "C" fn test_bridge(
    cx: *mut JsContext,
    _this: JsValue,
    argc: std::ffi::c_int,
    argv: *mut JsValue,
) -> JsValue {
    let op = standard::string(cx, arg(argc, argv, 0)).unwrap_or_default();
    let payload = standard::string(cx, arg(argc, argv, 1)).unwrap_or_default();
    let result = BRIDGE.with(|slot| match slot.borrow().as_ref() {
        Some(bridge) => bridge(&op, &payload),
        None => Err("`@nlayer/test` runs in `nlayer test` only".to_owned()),
    });
    match result {
        Ok(json) => unsafe { qjs::JS_NewStringLen(cx, json.as_ptr().cast(), json.len() as _) },
        Err(message) => {
            let message = CString::new(message.replace('\0', "")).unwrap_or_default();
            // SAFETY: throwing on the live context; `%s` keeps the text.
            unsafe { qjs::JS_ThrowTypeError(cx, c"%s".as_ptr(), message.as_ptr()) }
        }
    }
}

fn arg(argc: std::ffi::c_int, argv: *mut JsValue, index: usize) -> JsValue {
    if index < argc as usize {
        // SAFETY: `argv` has `argc` values.
        unsafe { *argv.add(index) }
    } else {
        qjs::JS_UNDEFINED
    }
}
