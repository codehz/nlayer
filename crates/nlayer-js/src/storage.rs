//! `storage` of `@nlayer/runtime`: the app's key-value storage, strings by
//! string key kept on the device (`nlayer_core::storage`).
//!
//! The calls are synchronous here; the prelude's `storage` wraps them in
//! the promises the TS API returns. A write past the quota throws an
//! `Error` named `QuotaExceededError` with code `ERR_NLAYER_QUOTA_EXCEEDED`,
//! which the promise rejects with.

use std::ffi::CStr;

use nlayer_core::storage::{StorageFull, Store};
use rquickjs::qjs;

use crate::standard::{self, string};

type JsContext = qjs::JSContext;
type JsValue = qjs::JSValue;

const UNDEFINED: JsValue = qjs::JS_UNDEFINED;

/// The error code of a write refused by the quota.
const QUOTA_CODE: &str = "ERR_NLAYER_QUOTA_EXCEEDED";

/// Installs the calls the prelude's `storage` wraps. The prelude deletes
/// them once it has made the object.
pub(crate) fn install(cx: *mut JsContext) {
    for (name, f, length) in [
        (c"__nlayerStorageGet", get as Native, 1),
        (c"__nlayerStorageSet", set as Native, 2),
        (c"__nlayerStorageRemove", remove as Native, 1),
        (c"__nlayerStorageKeys", keys as Native, 0),
        (c"__nlayerStorageClear", clear as Native, 0),
    ] {
        let func = standard::new_function(cx, name, f, length);
        standard::set_global(cx, name, func);
    }
}

type Native = unsafe extern "C" fn(*mut JsContext, JsValue, i32, *mut JsValue) -> JsValue;

/// Runs `f` on the context's storage, which is in memory until the host
/// sets one.
fn with_store<R>(cx: *mut JsContext, f: impl FnOnce(&Store) -> R) -> R {
    // SAFETY: the state `install` made for this context.
    unsafe {
        let state = &mut *crate::state::state(cx);
        let store = state.storage.get_or_insert_with(|| Store::new(None));
        f(store)
    }
}

fn arg(argc: i32, argv: *mut JsValue, i: usize) -> JsValue {
    if i < argc as usize {
        // SAFETY: `argv` has `argc` values.
        unsafe { *argv.add(i) }
    } else {
        UNDEFINED
    }
}

/// `__nlayerStorageGet(key)`: the value, or `null`.
unsafe extern "C" fn get(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let Some(key) = string(cx, arg(argc, argv, 0)) else {
        return qjs::JS_EXCEPTION;
    };
    match with_store(cx, |store| store.get(&key)) {
        Some(value) => unsafe { qjs::JS_NewStringLen(cx, value.as_ptr().cast(), value.len() as _) },
        None => qjs::JS_NULL,
    }
}

/// `__nlayerStorageSet(key, value)`: throws when the storage would grow
/// past its quota (the old value stays).
unsafe extern "C" fn set(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let (Some(key), Some(value)) = (
        string(cx, arg(argc, argv, 0)),
        string(cx, arg(argc, argv, 1)),
    ) else {
        return qjs::JS_EXCEPTION;
    };
    match with_store(cx, |store| store.set(key, value)) {
        Ok(()) => UNDEFINED,
        Err(full) => quota_error(cx, full),
    }
}

/// `__nlayerStorageRemove(key)`.
unsafe extern "C" fn remove(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let Some(key) = string(cx, arg(argc, argv, 0)) else {
        return qjs::JS_EXCEPTION;
    };
    with_store(cx, |store| store.remove(&key));
    UNDEFINED
}

/// `__nlayerStorageKeys()`: every key, in order.
unsafe extern "C" fn keys(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: i32,
    _argv: *mut JsValue,
) -> JsValue {
    let keys = with_store(cx, |store| store.keys());
    let mut values: Vec<JsValue> = keys
        .iter()
        .map(|key| unsafe { qjs::JS_NewStringLen(cx, key.as_ptr().cast(), key.len() as _) })
        .collect();
    // SAFETY: the array takes the values.
    unsafe { qjs::JS_NewArrayFrom(cx, values.len() as i32, values.as_mut_ptr()) }
}

/// `__nlayerStorageClear()`.
unsafe extern "C" fn clear(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: i32,
    _argv: *mut JsValue,
) -> JsValue {
    with_store(cx, |store| store.clear());
    UNDEFINED
}

/// A thrown `Error` for a refused write: `name` and `code` as the TS
/// declaration says.
fn quota_error(cx: *mut JsContext, full: StorageFull) -> JsValue {
    // SAFETY: new values only; the error takes the properties.
    unsafe {
        let error = qjs::JS_NewError(cx);
        let message = full.to_string();
        set_string(cx, error, c"message", &message);
        set_string(cx, error, c"name", "QuotaExceededError");
        set_string(cx, error, c"code", QUOTA_CODE);
        qjs::JS_Throw(cx, error)
    }
}

fn set_string(cx: *mut JsContext, object: JsValue, name: &CStr, value: &str) {
    // SAFETY: a new string the object takes.
    unsafe {
        let text = qjs::JS_NewStringLen(cx, value.as_ptr().cast(), value.len() as _);
        qjs::JS_SetPropertyStr(cx, object, name.as_ptr(), text);
    }
}
