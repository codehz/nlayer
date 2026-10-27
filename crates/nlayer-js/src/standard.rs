//! The platform API an app finds without importing anything: `console`, the
//! timers, `URL` / `URLSearchParams`, `TextEncoder` / `TextDecoder`,
//! `atob` / `btoa`, `performance.now` and `crypto.getRandomValues`.
//!
//! `fetch` (with `Request`, `Response`, `Headers`, `Blob` and
//! `AbortController`) queues requests for the engine's HTTP stack and
//! resolves their promises here.

use std::ffi::CStr;
use std::time::Instant;

use rquickjs::{Ctx, qjs};

type JsContext = qjs::JSContext;
type JsValue = qjs::JSValue;

const UNDEFINED: JsValue = qjs::JS_UNDEFINED;

/// A classic script run once per VM, after the natives: the few globals
/// that are plain JavaScript over them.
const PRELUDE: &str = r#"
(function () {
    var resolved = Promise.resolve();
    globalThis.queueMicrotask = function queueMicrotask(fn) {
        if (typeof fn !== "function") {
            throw new TypeError("queueMicrotask: the callback is not a function");
        }
        resolved.then(function () { return fn(); });
    };
    var codec = globalThis.__nlayerCodec;
    delete globalThis.__nlayerCodec;
    function TextEncoder() {}
    TextEncoder.prototype.encode = function encode(input) {
        return codec.encodeUtf8(input === undefined ? "" : String(input));
    };
    Object.defineProperty(TextEncoder.prototype, "encoding", { get: function () { return "utf-8"; } });
    function TextDecoder(label) {
        var name = String(label === undefined ? "utf-8" : label).toLowerCase();
        if (name !== "utf-8" && name !== "utf8") {
            throw new RangeError("TextDecoder: unsupported encoding " + label);
        }
    }
    TextDecoder.prototype.decode = function decode(input) {
        return codec.decodeUtf8(input);
    };
    Object.defineProperty(TextDecoder.prototype, "encoding", { get: function () { return "utf-8"; } });
    Object.defineProperty(globalThis, "TextEncoder", { value: TextEncoder, writable: true, configurable: true });
    Object.defineProperty(globalThis, "TextDecoder", { value: TextDecoder, writable: true, configurable: true });
    var setTimer = globalThis.__nlayerTimerSet;
    var clearTimer = globalThis.__nlayerTimerClear;
    delete globalThis.__nlayerTimerSet;
    delete globalThis.__nlayerTimerClear;
    var timers = new Map();
    var nextTimer = 1;
    function setTimeout(fn, ms) {
        if (typeof fn !== "function") {
            throw new TypeError("setTimeout: the callback is not a function");
        }
        var args = Array.prototype.slice.call(arguments, 2);
        var id = nextTimer++;
        timers.set(id, { fn: fn, args: args, ms: Math.max(0, Number(ms) || 0), repeat: null });
        setTimer(id, Math.max(0, Number(ms) || 0));
        return id;
    }
    function setInterval(fn, ms) {
        if (typeof fn !== "function") {
            throw new TypeError("setInterval: the callback is not a function");
        }
        var args = Array.prototype.slice.call(arguments, 2);
        var id = nextTimer++;
        var delay = Math.max(0, Number(ms) || 0);
        timers.set(id, { fn: fn, args: args, ms: delay, repeat: delay });
        setTimer(id, delay);
        return id;
    }
    function clearTimeout(id) {
        if (timers.delete(id)) clearTimer(id);
    }
    function __nlayerFireTimer(id) {
        var timer = timers.get(id);
        if (!timer) return;
        if (timer.repeat === null) {
            timers.delete(id);
        } else {
            setTimer(id, timer.repeat);
        }
        timer.fn.apply(undefined, timer.args);
    }
    Object.defineProperty(globalThis, "setTimeout", { value: setTimeout, writable: true, configurable: true });
    Object.defineProperty(globalThis, "setInterval", { value: setInterval, writable: true, configurable: true });
    Object.defineProperty(globalThis, "clearTimeout", { value: clearTimeout, writable: true, configurable: true });
    Object.defineProperty(globalThis, "clearInterval", { value: clearTimeout, writable: true, configurable: true });
    Object.defineProperty(globalThis, "__nlayerFireTimer", { value: __nlayerFireTimer, writable: false, configurable: false });

    // --- fetch, Request, Response, Headers, Blob and AbortController ---

    function Headers(init) {
        this._pairs = [];
        if (init instanceof Headers) {
            for (var i = 0; i < init._pairs.length; i++) {
                this._pairs.push([init._pairs[i][0], init._pairs[i][1]]);
            }
        } else if (Array.isArray(init)) {
            for (var j = 0; j < init.length; j++) this.append(init[j][0], init[j][1]);
        } else if (init) {
            var names = Object.keys(init);
            for (var k = 0; k < names.length; k++) this.append(names[k], init[names[k]]);
        }
    }
    Headers.prototype.append = function (name, value) {
        this._pairs.push([String(name).toLowerCase(), String(value)]);
    };
    Headers.prototype.set = function (name, value) {
        var key = String(name).toLowerCase();
        this.delete(key);
        this.append(key, value);
    };
    Headers.prototype.get = function (name) {
        var key = String(name).toLowerCase();
        var found = null;
        for (var i = 0; i < this._pairs.length; i++) {
            if (this._pairs[i][0] !== key) continue;
            found = found === null ? this._pairs[i][1] : found + ", " + this._pairs[i][1];
        }
        return found;
    };
    Headers.prototype.has = function (name) {
        var key = String(name).toLowerCase();
        return this._pairs.some(function (pair) { return pair[0] === key; });
    };
    Headers.prototype.delete = function (name) {
        var key = String(name).toLowerCase();
        this._pairs = this._pairs.filter(function (pair) { return pair[0] !== key; });
    };
    Headers.prototype.forEach = function (callback, thisArg) {
        var seen = new Set();
        for (var i = 0; i < this._pairs.length; i++) {
            var key = this._pairs[i][0];
            if (seen.has(key)) continue;
            seen.add(key);
            callback.call(thisArg, this.get(key), key, this);
        }
    };
    Headers.prototype.entries = function () {
        var self = this;
        var seen = new Set();
        var keys = [];
        for (var i = 0; i < this._pairs.length; i++) {
            var key = this._pairs[i][0];
            if (!seen.has(key)) { seen.add(key); keys.push(key); }
        }
        var at = 0;
        return {
            next: function () {
                if (at >= keys.length) return { done: true, value: undefined };
                var key = keys[at++];
                return { done: false, value: [key, self.get(key)] };
            },
        };
    };

    function bytesOf(part) {
        if (part === undefined || part === null) return new Uint8Array(0);
        if (typeof part === "string") return new TextEncoder().encode(part);
        if (part instanceof Blob) return part._bytes;
        if (part instanceof ArrayBuffer) return new Uint8Array(part.slice(0));
        if (ArrayBuffer.isView(part)) {
            return new Uint8Array(part.buffer.slice(part.byteOffset, part.byteOffset + part.byteLength));
        }
        throw new TypeError("a body is a string, a Blob, an ArrayBuffer or a typed array");
    }
    function concatBytes(parts) {
        var total = 0;
        for (var i = 0; i < parts.length; i++) total += parts[i].length;
        var out = new Uint8Array(total);
        var at = 0;
        for (var j = 0; j < parts.length; j++) {
            out.set(parts[j], at);
            at += parts[j].length;
        }
        return out;
    }

    function Blob(parts, options) {
        this._bytes = concatBytes((parts || []).map(bytesOf));
        this.size = this._bytes.length;
        this.type = options && options.type ? String(options.type).toLowerCase() : "";
    }
    Blob.prototype.arrayBuffer = function () {
        return Promise.resolve(this._bytes.buffer.slice(this._bytes.byteOffset, this._bytes.byteOffset + this._bytes.byteLength));
    };
    Blob.prototype.text = function () {
        return Promise.resolve(new TextDecoder().decode(this._bytes));
    };

    function abortError() {
        var error = new Error("This operation was aborted");
        error.name = "AbortError";
        return error;
    }

    function AbortSignal() {
        this.aborted = false;
        this.reason = undefined;
        this._listeners = [];
        this.onabort = null;
    }
    AbortSignal.prototype.addEventListener = function (type, listener) {
        if (type === "abort" && typeof listener === "function") this._listeners.push(listener);
    };
    AbortSignal.prototype.removeEventListener = function (type, listener) {
        if (type !== "abort") return;
        var at = this._listeners.indexOf(listener);
        if (at >= 0) this._listeners.splice(at, 1);
    };
    AbortSignal.prototype._abort = function (reason) {
        if (this.aborted) return;
        this.aborted = true;
        this.reason = reason === undefined ? abortError() : reason;
        var listeners = this._listeners.slice();
        this._listeners.length = 0;
        for (var i = 0; i < listeners.length; i++) listeners[i].call(this, { type: "abort", target: this });
        if (typeof this.onabort === "function") this.onabort({ type: "abort", target: this });
    };
    function AbortController() {
        this.signal = new AbortSignal();
    }
    AbortController.prototype.abort = function (reason) {
        this.signal._abort(reason);
    };

    function Request(input, init) {
        init = init || {};
        if (input instanceof Request) {
            this.url = input.url;
            this.method = input.method;
            this.headers = new Headers(input.headers);
            this._body = input._body;
            this.follow = input.follow;
            this.signal = init.signal || input.signal;
        } else {
            this.url = String(input);
            this.method = String(init.method || "GET").toUpperCase();
            this.headers = new Headers(init.headers);
            this._body = (init.body === undefined || init.body === null) ? null : bytesOf(init.body);
            this.follow = init.redirect !== "manual";
            this.signal = init.signal || null;
        }
    }

    function Response(init) {
        if (init && init.status !== undefined) {
            this.status = init.status;
            this.statusText = init.statusText || "";
            this.url = init.url || "";
            this.redirected = !!init.redirected;
            this.headers = init.headers instanceof Headers ? init.headers : new Headers(init.headers);
            this._body = init.body || new Uint8Array(0);
            this.ok = this.status >= 200 && this.status < 300;
        } else {
            this.status = 200;
            this.statusText = "";
            this.url = "";
            this.redirected = false;
            this.headers = new Headers();
            this._body = bytesOf(init);
            this.ok = true;
        }
    }
    Response.prototype._share = function () {
        var self = this;
        if (self._bodyConsumed) {
            return Promise.reject(new TypeError("the body was already read"));
        }
        self._bodyConsumed = true;
        return Promise.resolve(self._body);
    };
    Response.prototype.arrayBuffer = function () {
        return this._share().then(function (bytes) {
            return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
        });
    };
    Response.prototype.text = function () {
        return this._share().then(function (bytes) { return new TextDecoder().decode(bytes); });
    };
    Response.prototype.json = function () {
        return this.text().then(function (text) { return JSON.parse(text); });
    };
    Response.prototype.blob = function () {
        return this._share().then(function (bytes) { return new Blob([bytes]); });
    };

    var pendingFetch = new Map();
    function fetch(input, init) {
        var request;
        try {
            request = new Request(input, init);
        } catch (error) {
            return Promise.reject(error);
        }
        var signal = request.signal;
        if (signal && signal.aborted) return Promise.reject(signal.reason || abortError());
        return new Promise(function (resolve, reject) {
            var start = globalThis.__nlayerHttpStart;
            var id;
            try {
                id = start(
                    request.method,
                    request.url,
                    request.headers._pairs,
                    request._body,
                    request.follow
                );
            } catch (error) {
                reject(error);
                return;
            }
            pendingFetch.set(id, { resolve: resolve, reject: reject, signal: signal });
            if (signal) {
                var onAbort = function () {
                    if (pendingFetch.delete(id)) reject(signal.reason || abortError());
                };
                signal.addEventListener("abort", onAbort);
            }
        });
    }

    function __nlayerFetchDone(id, error, data) {
        var pending = pendingFetch.get(id);
        if (!pending) return;
        pendingFetch.delete(id);
        if (error !== null && error !== undefined) {
            pending.reject(new TypeError(String(error)));
            return;
        }
        pending.resolve(new Response({
            status: data.status,
            statusText: data.statusText,
            url: data.url,
            redirected: data.redirected,
            headers: data.headers,
            body: data.body,
        }));
    }
    Object.defineProperty(globalThis, "__nlayerFetchDone", { value: __nlayerFetchDone, writable: false, configurable: false });
    var globals = {
        Headers: Headers,
        Request: Request,
        Response: Response,
        Blob: Blob,
        AbortController: AbortController,
        AbortSignal: AbortSignal,
        fetch: fetch,
    };
    for (var name in globals) {
        Object.defineProperty(globalThis, name, { value: globals[name], writable: true, configurable: true });
    }
})();
"#;

type Native = unsafe extern "C" fn(*mut JsContext, JsValue, i32, *mut JsValue) -> JsValue;

fn new_function(cx: *mut JsContext, name: &CStr, f: Native, length: i32) -> JsValue {
    // SAFETY: a plain C function.
    unsafe {
        qjs::JS_NewCFunction2(
            cx,
            Some(f),
            name.as_ptr(),
            length,
            qjs::JSCFunctionEnum_JS_CFUNC_generic,
            0,
        )
    }
}

fn string(cx: *mut JsContext, value: JsValue) -> Option<String> {
    // SAFETY: converts a live value; the result is freed.
    unsafe {
        let s = qjs::JS_ToString(cx, value);
        if qjs::JS_IsException(s) {
            qjs::JS_FreeValue(cx, qjs::JS_GetException(cx));
            return None;
        }
        let mut len = 0usize;
        let c = qjs::JS_ToCStringLen(cx, &mut len, s);
        if c.is_null() {
            qjs::JS_FreeValue(cx, s);
            return None;
        }
        let out =
            String::from_utf8_lossy(std::slice::from_raw_parts(c.cast::<u8>(), len)).into_owned();
        qjs::JS_FreeCString(cx, c);
        qjs::JS_FreeValue(cx, s);
        Some(out)
    }
}

fn set_global(cx: *mut JsContext, name: &CStr, value: JsValue) {
    // SAFETY: the global object takes the value.
    unsafe {
        let global = qjs::JS_GetGlobalObject(cx);
        qjs::JS_SetPropertyStr(cx, global, name.as_ptr(), value);
        qjs::JS_FreeValue(cx, global);
    }
}

/// Installs the platform API into a fresh context.
pub(crate) fn install(cx: &Ctx<'_>) {
    let raw = cx.as_raw().as_ptr();
    console(raw);
    timers(raw);
    performance(raw);
    codec(raw);
    random(raw);
    encoding(raw);
    url(cx);
    network(raw);
    // SAFETY: a classic script's completion value is freed.
    unsafe {
        // QuickJS reads up to the NUL it requires after the source.
        let mut source = PRELUDE.to_owned();
        let len = source.len();
        source.push('\0');
        let value = qjs::JS_Eval(
            raw,
            source.as_ptr().cast::<std::ffi::c_char>(),
            len as _,
            c"nlayer:standard".as_ptr(),
            (qjs::JS_EVAL_TYPE_GLOBAL) as i32,
        );
        if qjs::JS_IsException(value) {
            let exception = qjs::JS_GetException(raw);
            let text = qjs::JS_ToCStringLen(raw, std::ptr::null_mut(), exception);
            if !text.is_null() {
                log::error!(
                    "nlayer:standard: {}",
                    CStr::from_ptr(text).to_string_lossy()
                );
                qjs::JS_FreeCString(raw, text);
            }
            qjs::JS_FreeValue(raw, exception);
        }
        qjs::JS_FreeValue(raw, value);
    }
}

/// `console.log` / `info` / `debug` / `warn` / `error`, writing to the log.
fn console(cx: *mut JsContext) {
    // SAFETY: a new object of C functions; the global object takes it.
    unsafe {
        let console = qjs::JS_NewObject(cx);
        for (name, level) in [
            (c"log", 0),
            (c"info", 1),
            (c"debug", 2),
            (c"warn", 3),
            (c"error", 4),
        ] {
            let mut data = [qjs::JS_NewFloat64(level as f64)];
            let f = qjs::JS_NewCFunctionData(cx, Some(console_write), 0, 0, 1, data.as_mut_ptr());
            qjs::JS_FreeValue(cx, data[0]);
            qjs::JS_SetPropertyStr(cx, console, name.as_ptr(), f);
        }
        set_global(cx, c"console", console);
    }
}

unsafe extern "C" fn console_write(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
    _magic: i32,
    data: *mut JsValue,
) -> JsValue {
    // SAFETY: one data value (`console`); arguments are borrowed.
    let mut level = 0f64;
    unsafe {
        qjs::JS_ToFloat64(cx, &mut level, *data);
    }
    let mut parts = Vec::with_capacity(argc.max(0) as usize);
    for i in 0..argc.max(0) as usize {
        // SAFETY: QuickJS passes `argc` values.
        let value = unsafe { *argv.add(i) };
        match string(cx, value) {
            Some(text) => parts.push(text),
            None => return qjs::JS_EXCEPTION,
        }
    }
    let line = parts.join(" ");
    match level as u8 {
        3 => log::warn!("{line}"),
        4 => log::error!("{line}"),
        2 => log::debug!("{line}"),
        _ => log::info!("{line}"),
    }
    UNDEFINED
}

/// `__nlayerTimerSet` / `__nlayerTimerClear`: the queue the engine's loop
/// waits on. The prelude's `setTimeout` / `setInterval` keep the callbacks
/// (in JS, where the collector sees them) and call these for the clock.
fn timers(cx: *mut JsContext) {
    set_global(
        cx,
        c"__nlayerTimerSet",
        new_function(cx, c"__nlayerTimerSet", timer_set, 2),
    );
    set_global(
        cx,
        c"__nlayerTimerClear",
        new_function(cx, c"__nlayerTimerClear", timer_clear, 1),
    );
}

unsafe extern "C" fn timer_set(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    // SAFETY: state installed with the context; two borrowed arguments.
    let (ms, id) = unsafe {
        let id = if argc > 0 { *argv } else { UNDEFINED };
        let ms = if argc > 1 { *argv.add(1) } else { UNDEFINED };
        let mut id_number = 0f64;
        let mut ms_number = 0f64;
        if qjs::JS_ToFloat64(cx, &mut id_number, id) != 0
            || qjs::JS_ToFloat64(cx, &mut ms_number, ms) != 0
        {
            return qjs::JS_EXCEPTION;
        }
        (ms_number.max(0.0), id_number as u32)
    };
    // SAFETY: the live state.
    unsafe { (*crate::state::state(cx)).timers.set(id, ms) };
    UNDEFINED
}

unsafe extern "C" fn timer_clear(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let id = if argc > 0 {
        unsafe { *argv }
    } else {
        UNDEFINED
    };
    let mut number = 0f64;
    // SAFETY: a live state; a borrowed value.
    unsafe {
        if qjs::JS_ToFloat64(cx, &mut number, id) == 0 {
            (*crate::state::state(cx)).timers.clear(number as u32);
        }
    }
    UNDEFINED
}

/// `__nlayerHttpStart`: queues a `fetch` for the engine and returns its id.
fn network(cx: *mut JsContext) {
    set_global(
        cx,
        c"__nlayerHttpStart",
        new_function(cx, c"__nlayerHttpStart", http_start, 5),
    );
}

unsafe extern "C" fn http_start(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let arg = |i: usize| {
        if (i as i32) < argc {
            // SAFETY: QuickJS passes `argc` values.
            unsafe { *argv.add(i) }
        } else {
            UNDEFINED
        }
    };
    let (Some(method), Some(url)) = (string(cx, arg(0)), string(cx, arg(1))) else {
        return qjs::JS_EXCEPTION;
    };
    // The headers: an array of `[name, value]` pairs.
    let mut headers = Vec::new();
    // SAFETY: reads of a live array; every value got is freed.
    unsafe {
        let list = arg(2);
        if qjs::JS_IsArray(list) {
            let mut len: i64 = 0;
            if qjs::JS_GetLength(cx, list, &mut len) == 0 {
                for i in 0..len.max(0) as u32 {
                    let pair = qjs::JS_GetPropertyUint32(cx, list, i);
                    if qjs::JS_IsException(pair) {
                        return pair;
                    }
                    if qjs::JS_IsArray(pair) {
                        let name = qjs::JS_GetPropertyUint32(cx, pair, 0);
                        let value = qjs::JS_GetPropertyUint32(cx, pair, 1);
                        if let (Some(name), Some(value)) = (string(cx, name), string(cx, value)) {
                            headers.push((name, value));
                        }
                        qjs::JS_FreeValue(cx, name);
                        qjs::JS_FreeValue(cx, value);
                    }
                    qjs::JS_FreeValue(cx, pair);
                }
            }
        }
    }
    let body = bytes_of(cx, arg(3)).filter(|bytes| !bytes.is_empty());
    // SAFETY: truthiness of a live value runs no JS.
    let follow = argc > 4 && unsafe { qjs::JS_ToBool(cx, arg(4)) } != 0;
    let request = crate::net::HttpRequest {
        method: method.to_ascii_uppercase(),
        url,
        headers,
        body,
        follow_redirects: follow,
    };
    // SAFETY: the live state.
    let id = unsafe { (*crate::state::state(cx)).net.push(request) };
    qjs::JS_NewFloat64(id as f64)
}

/// `performance.now()`: milliseconds since the VM started.
fn performance(cx: *mut JsContext) {
    // SAFETY: new object and function; the global object takes it.
    unsafe {
        let performance = qjs::JS_NewObject(cx);
        let now = qjs::JS_NewCFunction2(
            cx,
            Some(performance_now),
            c"now".as_ptr(),
            0,
            qjs::JSCFunctionEnum_JS_CFUNC_generic,
            0,
        );
        qjs::JS_SetPropertyStr(cx, performance, c"now".as_ptr(), now);
        set_global(cx, c"performance", performance);
    }
}

unsafe extern "C" fn performance_now(
    cx: *mut JsContext,
    _this: JsValue,
    _argc: i32,
    _argv: *mut JsValue,
) -> JsValue {
    // SAFETY: reading the state installed by `state::install`.
    let started = unsafe { (*crate::state::state(cx)).started };
    qjs::JS_NewFloat64(started.elapsed().as_secs_f64() * 1000.0)
}

/// `__nlayerCodec.encodeUtf8` / `decodeUtf8`, the prelude's TextEncoder and
/// TextDecoder.
fn codec(cx: *mut JsContext) {
    // SAFETY: a new object of C functions; the global object takes it.
    unsafe {
        let codec = qjs::JS_NewObject(cx);
        let encode = new_function(cx, c"encodeUtf8", encode_utf8, 1);
        qjs::JS_SetPropertyStr(cx, codec, c"encodeUtf8".as_ptr(), encode);
        let decode = new_function(cx, c"decodeUtf8", decode_utf8, 1);
        qjs::JS_SetPropertyStr(cx, codec, c"decodeUtf8".as_ptr(), decode);
        set_global(cx, c"__nlayerCodec", codec);
    }
}

unsafe extern "C" fn encode_utf8(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    // SAFETY: a borrowed argument; the array takes a copy of the bytes.
    let value = if argc > 0 {
        unsafe { *argv }
    } else {
        UNDEFINED
    };
    let Some(text) = string(cx, value) else {
        return qjs::JS_EXCEPTION;
    };
    unsafe { qjs::JS_NewUint8ArrayCopy(cx, text.as_ptr(), text.len() as _) }
}

unsafe extern "C" fn decode_utf8(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let value = if argc > 0 {
        unsafe { *argv }
    } else {
        UNDEFINED
    };
    let bytes = bytes_of(cx, value);
    let Some(bytes) = bytes else {
        return qjs::JS_EXCEPTION;
    };
    let text = String::from_utf8_lossy(&bytes);
    unsafe { qjs::JS_NewStringLen(cx, text.as_ptr().cast(), text.len() as _) }
}

/// The bytes of an `ArrayBuffer` or a typed array (borrowed).
fn bytes_of(cx: *mut JsContext, value: JsValue) -> Option<Vec<u8>> {
    // SAFETY: reads of a live value; the bytes are copied before JS runs.
    unsafe {
        if qjs::JS_IsArrayBuffer(value) {
            let mut size = 0u64;
            let data = qjs::JS_GetArrayBuffer(cx, &mut size, value);
            if data.is_null() {
                return None;
            }
            return Some(std::slice::from_raw_parts(data, size as usize).to_vec());
        }
        if qjs::JS_IsObject(value) {
            let mut offset = 0u64;
            let mut length = 0u64;
            let mut _element = 0u64;
            let buffer =
                qjs::JS_GetTypedArrayBuffer(cx, value, &mut offset, &mut length, &mut _element);
            if qjs::JS_IsException(buffer) {
                qjs::JS_FreeValue(cx, qjs::JS_GetException(cx));
                return None;
            }
            let mut size = 0u64;
            let data = qjs::JS_GetArrayBuffer(cx, &mut size, buffer);
            let out = if data.is_null() {
                None
            } else {
                let offset = offset as usize;
                let end = (offset + length as usize).min(size as usize);
                Some(std::slice::from_raw_parts(data.add(offset), end - offset).to_vec())
            };
            qjs::JS_FreeValue(cx, buffer);
            return out;
        }
    }
    None
}

/// `crypto.getRandomValues(array)`: fills a typed array with random bytes.
fn random(cx: *mut JsContext) {
    // SAFETY: a new object and function; the global object takes it.
    unsafe {
        let crypto = qjs::JS_NewObject(cx);
        let get = new_function(cx, c"getRandomValues", get_random_values, 1);
        qjs::JS_SetPropertyStr(cx, crypto, c"getRandomValues".as_ptr(), get);
        set_global(cx, c"crypto", crypto);
    }
}

unsafe extern "C" fn get_random_values(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let value = if argc > 0 {
        unsafe { *argv }
    } else {
        UNDEFINED
    };
    // The bytes of a typed array, written in place.
    // SAFETY: reads and writes of the live array, no JS runs meanwhile.
    unsafe {
        if !qjs::JS_IsObject(value) {
            return qjs::JS_ThrowTypeError(
                cx,
                c"crypto.getRandomValues: expected a typed array".as_ptr(),
            );
        }
        let mut offset = 0u64;
        let mut length = 0u64;
        let mut _element = 0u64;
        let buffer =
            qjs::JS_GetTypedArrayBuffer(cx, value, &mut offset, &mut length, &mut _element);
        if qjs::JS_IsException(buffer) {
            qjs::JS_FreeValue(cx, qjs::JS_GetException(cx));
            return qjs::JS_ThrowTypeError(
                cx,
                c"crypto.getRandomValues: expected a typed array".as_ptr(),
            );
        }
        let mut size = 0u64;
        let data = qjs::JS_GetArrayBuffer(cx, &mut size, buffer);
        if !data.is_null() {
            let bytes = std::slice::from_raw_parts_mut(data.add(offset as usize), length as usize);
            if getrandom::fill(bytes).is_err() {
                qjs::JS_FreeValue(cx, buffer);
                return qjs::JS_ThrowInternalError(
                    cx,
                    c"crypto.getRandomValues: no randomness".as_ptr(),
                );
            }
        }
        qjs::JS_FreeValue(cx, buffer);
    }
    // The argument is borrowed; the return value is owned.
    unsafe { qjs::JS_DupValue(cx, value) }
}

/// `atob` / `btoa`: base64, Latin-1 strings as the web's.
fn encoding(cx: *mut JsContext) {
    set_global(cx, c"atob", new_function(cx, c"atob", atob, 1));
    set_global(cx, c"btoa", new_function(cx, c"btoa", btoa, 1));
}

unsafe extern "C" fn atob(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let value = if argc > 0 {
        unsafe { *argv }
    } else {
        UNDEFINED
    };
    use base64::Engine as _;
    let Some(text) = string(cx, value) else {
        return qjs::JS_EXCEPTION;
    };
    match base64::engine::general_purpose::STANDARD.decode(text.trim()) {
        Ok(bytes) => {
            let text: String = bytes.into_iter().map(char::from).collect();
            unsafe { qjs::JS_NewStringLen(cx, text.as_ptr().cast(), text.len() as _) }
        }
        Err(_) => unsafe {
            qjs::JS_ThrowTypeError(cx, c"atob: the string is not valid base64".as_ptr())
        },
    }
}

unsafe extern "C" fn btoa(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let value = if argc > 0 {
        unsafe { *argv }
    } else {
        UNDEFINED
    };
    use base64::Engine as _;
    let Some(text) = string(cx, value) else {
        return qjs::JS_EXCEPTION;
    };
    if !text.is_ascii() {
        return unsafe {
            qjs::JS_ThrowTypeError(
                cx,
                c"btoa: the string has characters outside Latin-1".as_ptr(),
            )
        };
    }
    let out = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    unsafe { qjs::JS_NewStringLen(cx, out.as_ptr().cast(), out.len() as _) }
}

/// `URL` and `URLSearchParams`.
fn url(cx: &Ctx<'_>) {
    if let Err(error) = quickjs_stdlib::url::init(cx) {
        log::error!("could not install URL: {error}");
    }
}

/// The VM's start, for `performance.now`.
pub(crate) fn started() -> Instant {
    Instant::now()
}
