//! `URL` and `URLSearchParams` (URL Standard), on the `url` crate.
//!
//! A URL's `searchParams` is live both ways: changing the parameters
//! rewrites the query, and setting `search` or `href` parses it again.
//! The behavior follows `nsh-js`'s `web/url.rs`; the objects are this
//! host's classes, not that crate's web class layer.

#![allow(unsafe_op_in_unsafe_fn)]

use std::{ffi::CStr, ptr};

use rquickjs::qjs;
use url::{Url, form_urlencoded, quirks};

use crate::state;

type JsContext = qjs::JSContext;
type JsValue = qjs::JSValue;

const UNDEFINED: JsValue = qjs::JS_UNDEFINED;

/// Class ids of the two globals, registered by [`install`].
#[derive(Clone, Copy, Default)]
pub(crate) struct Classes {
    url: qjs::JSClassID,
    params: qjs::JSClassID,
}

struct UrlData {
    url: Url,
    /// Its `URLSearchParams` (owned).
    params: JsValue,
}

struct ParamsData {
    list: Vec<(String, String)>,
    /// The URL whose query these are, or [`UNDEFINED`] when standalone.
    url: JsValue,
}

/// Parses `input` against `base`.
fn parse(input: &str, base: Option<&str>) -> Option<Url> {
    match base {
        Some(base) => Url::parse(base).ok()?.join(input).ok(),
        None => Url::parse(input).ok(),
    }
}

/// The pairs of a query (`application/x-www-form-urlencoded`).
fn parse_query(query: &str) -> Vec<(String, String)> {
    let query = query.strip_prefix('?').unwrap_or(query);
    form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// The `application/x-www-form-urlencoded` serialization of `list`.
fn serialize(list: &[(String, String)]) -> String {
    let mut s = form_urlencoded::Serializer::new(String::new());
    for (k, v) in list {
        s.append_pair(k, v);
    }
    s.finish()
}

fn arg(argc: i32, argv: *mut JsValue, index: usize) -> JsValue {
    if (index as i32) < argc {
        // SAFETY: `index` is inside `argv`.
        unsafe { *argv.add(index) }
    } else {
        UNDEFINED
    }
}

fn type_error(cx: *mut JsContext, message: &str) -> JsValue {
    let message = std::ffi::CString::new(message).unwrap_or_default();
    // SAFETY: throws on a live context.
    unsafe { qjs::JS_ThrowTypeError(cx, c"%s".as_ptr(), message.as_ptr()) }
}

/// `JS_ToString`, or the pending exception.
fn js_string(cx: *mut JsContext, value: JsValue) -> Result<String, JsValue> {
    // SAFETY: converts a live value; the result is freed.
    unsafe {
        let s = qjs::JS_ToString(cx, value);
        if qjs::JS_IsException(s) {
            return Err(qjs::JS_EXCEPTION);
        }
        let mut len = 0usize;
        let c = qjs::JS_ToCStringLen(cx, &mut len, s);
        if c.is_null() {
            qjs::JS_FreeValue(cx, s);
            return Err(qjs::JS_EXCEPTION);
        }
        let out =
            String::from_utf8_lossy(std::slice::from_raw_parts(c.cast::<u8>(), len)).into_owned();
        qjs::JS_FreeCString(cx, c);
        qjs::JS_FreeValue(cx, s);
        Ok(out)
    }
}

fn new_string(cx: *mut JsContext, text: &str) -> JsValue {
    // SAFETY: a new string.
    unsafe { qjs::JS_NewStringLen(cx, text.as_ptr().cast(), text.len() as _) }
}

fn classes(cx: *mut JsContext) -> Classes {
    // SAFETY: the state `install` made.
    unsafe { (*state::state(cx)).url_classes }
}

fn url_data<'a>(cx: *mut JsContext, this: JsValue) -> Result<&'a mut UrlData, JsValue> {
    // SAFETY: `GetOpaque2` throws when `this` is not a URL; the box lives
    // until the finalizer.
    unsafe {
        let p = qjs::JS_GetOpaque2(cx, this, classes(cx).url) as *mut UrlData;
        if p.is_null() {
            return Err(qjs::JS_EXCEPTION);
        }
        Ok(&mut *p)
    }
}

fn params_data<'a>(cx: *mut JsContext, this: JsValue) -> Result<&'a mut ParamsData, JsValue> {
    // SAFETY: as [`url_data`], for a URLSearchParams.
    unsafe {
        let p = qjs::JS_GetOpaque2(cx, this, classes(cx).params) as *mut ParamsData;
        if p.is_null() {
            return Err(qjs::JS_EXCEPTION);
        }
        Ok(&mut *p)
    }
}

fn is_params(cx: *mut JsContext, value: JsValue) -> bool {
    // SAFETY: the class id of a non-object is not ours.
    unsafe { qjs::JS_IsObject(value) && qjs::JS_GetClassID(value) == classes(cx).params }
}

/// An object of `class_id` holding `data`. `new_target` selects the
/// prototype (`undefined`: the class's own).
fn object_with(
    cx: *mut JsContext,
    new_target: JsValue,
    class_id: qjs::JSClassID,
    data: *mut std::ffi::c_void,
) -> Result<JsValue, JsValue> {
    // SAFETY: the prototype is freed; the object takes `data`.
    unsafe {
        let proto = if qjs::JS_IsUndefined(new_target) {
            qjs::JS_GetClassProto(cx, class_id)
        } else {
            let proto = qjs::JS_GetPropertyStr(cx, new_target, c"prototype".as_ptr());
            if qjs::JS_IsException(proto) {
                return Err(qjs::JS_EXCEPTION);
            }
            proto
        };
        let obj = qjs::JS_NewObjectProtoClass(cx, proto, class_id);
        qjs::JS_FreeValue(cx, proto);
        if qjs::JS_IsException(obj) {
            return Err(obj);
        }
        qjs::JS_SetOpaque(obj, data);
        Ok(obj)
    }
}

/// A new URL object for `url`, with its parameters.
fn new_url(cx: *mut JsContext, new_target: JsValue, url: Url) -> Result<JsValue, JsValue> {
    let list = parse_query(url.query().unwrap_or_default());
    let params_ptr = Box::into_raw(Box::new(ParamsData {
        list,
        url: UNDEFINED,
    }));
    let params = match object_with(cx, UNDEFINED, classes(cx).params, params_ptr.cast()) {
        Ok(obj) => obj,
        Err(e) => {
            // SAFETY: the box never became an object.
            drop(unsafe { Box::from_raw(params_ptr) });
            return Err(e);
        }
    };
    let url_ptr = Box::into_raw(Box::new(UrlData {
        url,
        // SAFETY: an extra ref, freed by the URL's finalizer.
        params: unsafe { qjs::JS_DupValue(cx, params) },
    }));
    let obj = match object_with(cx, new_target, classes(cx).url, url_ptr.cast()) {
        Ok(obj) => obj,
        Err(e) => {
            // SAFETY: the params object is unreferenced; free it, and the
            // URL box (its dup of `params` included).
            unsafe {
                qjs::JS_FreeValue(cx, params);
                drop(Box::from_raw(url_ptr));
            }
            return Err(e);
        }
    };
    // SAFETY: `params` is the object just made; the URL ref is freed by
    // the parameters' finalizer. The local `params` ref is dropped: the
    // URL's dup keeps it.
    unsafe {
        (*params_ptr).url = qjs::JS_DupValue(cx, obj);
        qjs::JS_FreeValue(cx, params);
    }
    Ok(obj)
}

fn inputs(
    cx: *mut JsContext,
    argc: i32,
    argv: *mut JsValue,
) -> Result<(String, Option<String>), JsValue> {
    let input = js_string(cx, arg(argc, argv, 0))?;
    let base_val = arg(argc, argv, 1);
    let base = if unsafe { qjs::JS_IsUndefined(base_val) } {
        None
    } else {
        Some(js_string(cx, base_val)?)
    };
    Ok((input, base))
}

unsafe extern "C" fn url_constructor(
    cx: *mut JsContext,
    new_target: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    match (|| {
        let (input, base) = inputs(cx, argc, argv)?;
        let url = parse(&input, base.as_deref())
            .ok_or_else(|| type_error(cx, &format!("Invalid URL: {input}")))?;
        new_url(cx, new_target, url)
    })() {
        Ok(v) => v,
        Err(e) => e,
    }
}

unsafe extern "C" fn url_can_parse(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    match inputs(cx, argc, argv) {
        Ok((input, base)) => {
            if parse(&input, base.as_deref()).is_some() {
                qjs::JS_TRUE
            } else {
                qjs::JS_FALSE
            }
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn url_parse(
    cx: *mut JsContext,
    _this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    match (|| {
        let (input, base) = inputs(cx, argc, argv)?;
        match parse(&input, base.as_deref()) {
            Some(url) => new_url(cx, UNDEFINED, url),
            None => Ok(qjs::JS_NULL),
        }
    })() {
        Ok(v) => v,
        Err(e) => e,
    }
}

/// After the query changed: the parameters again.
fn reparse(cx: *mut JsContext, this: JsValue) -> Result<(), JsValue> {
    let (params, query) = {
        let u = url_data(cx, this)?;
        (
            unsafe { qjs::JS_DupValue(cx, u.params) },
            u.url.query().unwrap_or_default().to_owned(),
        )
    };
    let result = (|| {
        params_data(cx, params)?.list = parse_query(&query);
        Ok(())
    })();
    // SAFETY: the dup above.
    unsafe { qjs::JS_FreeValue(cx, params) };
    result
}

fn component(url: &Url, magic: i32) -> String {
    match magic {
        0 => quirks::href(url).into(),
        1 => quirks::origin(url),
        2 => quirks::protocol(url).into(),
        3 => quirks::username(url).into(),
        4 => quirks::password(url).into(),
        5 => quirks::host(url).into(),
        6 => quirks::hostname(url).into(),
        7 => quirks::port(url).into(),
        8 => quirks::pathname(url).into(),
        9 => quirks::search(url).into(),
        _ => quirks::hash(url).into(),
    }
}

unsafe extern "C" fn url_get(cx: *mut JsContext, this: JsValue, magic: i32) -> JsValue {
    if magic == 11 {
        return match url_data(cx, this) {
            Ok(u) => unsafe { qjs::JS_DupValue(cx, u.params) },
            Err(e) => e,
        };
    }
    match url_data(cx, this) {
        Ok(u) => new_string(cx, &component(&u.url, magic)),
        Err(e) => e,
    }
}

unsafe extern "C" fn url_set(
    cx: *mut JsContext,
    this: JsValue,
    value: JsValue,
    magic: i32,
) -> JsValue {
    let text = match js_string(cx, value) {
        Ok(text) => text,
        Err(e) => return e,
    };
    let set = (|| {
        let u = url_data(cx, this)?;
        let url = &mut u.url;
        // Invalid values leave the URL as it was, except `href`, which throws.
        match magic {
            0 => {
                if quirks::set_href(url, &text).is_err() {
                    return Err(type_error(cx, &format!("Invalid URL: {text}")));
                }
            }
            2 => {
                let _ = quirks::set_protocol(url, &text);
            }
            3 => {
                let _ = quirks::set_username(url, &text);
            }
            4 => {
                let _ = quirks::set_password(url, &text);
            }
            5 => {
                let _ = quirks::set_host(url, &text);
            }
            6 => {
                let _ = quirks::set_hostname(url, &text);
            }
            7 => {
                let _ = quirks::set_port(url, &text);
            }
            8 => {
                quirks::set_pathname(url, &text);
            }
            9 => quirks::set_search(url, &text),
            _ => quirks::set_hash(url, &text),
        }
        Ok(())
    })();
    if let Err(e) = set {
        return e;
    }
    if (magic == 0 || magic == 9)
        && let Err(e) = reparse(cx, this)
    {
        return e;
    }
    UNDEFINED
}

unsafe extern "C" fn url_to_string(
    cx: *mut JsContext,
    this: JsValue,
    _argc: i32,
    _argv: *mut JsValue,
) -> JsValue {
    match url_data(cx, this) {
        Ok(u) => new_string(cx, quirks::href(&u.url)),
        Err(e) => e,
    }
}

/// After the parameters changed: the URL's query.
fn update(cx: *mut JsContext, this: JsValue) -> Result<(), JsValue> {
    let (url, query) = {
        let p = params_data(cx, this)?;
        (unsafe { qjs::JS_DupValue(cx, p.url) }, serialize(&p.list))
    };
    let result = if unsafe { qjs::JS_IsUndefined(url) } {
        Ok(())
    } else {
        let u = url_data(cx, url)?;
        if query.is_empty() {
            u.url.set_query(None);
        } else {
            u.url.set_query(Some(&query));
        }
        Ok(())
    };
    // SAFETY: the dup above.
    unsafe { qjs::JS_FreeValue(cx, url) };
    result
}

fn init_list(cx: *mut JsContext, init: JsValue) -> Result<Vec<(String, String)>, JsValue> {
    if unsafe { qjs::JS_IsUndefined(init) } {
        return Ok(Vec::new());
    }
    if is_params(cx, init) {
        return Ok(params_data(cx, init)?.list.clone());
    }
    // SAFETY: arrays and records are objects; anything else is a query string.
    if unsafe { qjs::JS_IsObject(init) } {
        if unsafe { qjs::JS_IsArray(init) } {
            return array_pairs(cx, init);
        }
        return record(cx, init);
    }
    Ok(parse_query(&js_string(cx, init)?))
}

fn array_pairs(cx: *mut JsContext, init: JsValue) -> Result<Vec<(String, String)>, JsValue> {
    // SAFETY: `init` is an array; each value got is freed.
    unsafe {
        let len_val = qjs::JS_GetPropertyStr(cx, init, c"length".as_ptr());
        if qjs::JS_IsException(len_val) {
            return Err(qjs::JS_EXCEPTION);
        }
        let mut len = 0i32;
        let failed = qjs::JS_ToInt32(cx, &mut len, len_val);
        qjs::JS_FreeValue(cx, len_val);
        if failed != 0 {
            return Err(qjs::JS_EXCEPTION);
        }
        let mut list = Vec::with_capacity(len.max(0) as usize);
        for i in 0..len.max(0) {
            let pair = qjs::JS_GetPropertyUint32(cx, init, i as u32);
            if qjs::JS_IsException(pair) {
                return Err(qjs::JS_EXCEPTION);
            }
            let parsed = pair_of(cx, pair);
            qjs::JS_FreeValue(cx, pair);
            list.push(parsed?);
        }
        Ok(list)
    }
}

fn pair_of(cx: *mut JsContext, pair: JsValue) -> Result<(String, String), JsValue> {
    // SAFETY: a live value. A pair is an array of two strings; both items
    // are freed.
    unsafe {
        if !qjs::JS_IsArray(pair) {
            return Err(type_error(cx, "Each pair must have two items"));
        }
        let len_val = qjs::JS_GetPropertyStr(cx, pair, c"length".as_ptr());
        if qjs::JS_IsException(len_val) {
            return Err(qjs::JS_EXCEPTION);
        }
        let mut len = 0i32;
        let failed = qjs::JS_ToInt32(cx, &mut len, len_val);
        qjs::JS_FreeValue(cx, len_val);
        if failed != 0 {
            return Err(qjs::JS_EXCEPTION);
        }
        if len != 2 {
            return Err(type_error(cx, "Each pair must have two items"));
        }
        let k = qjs::JS_GetPropertyUint32(cx, pair, 0);
        let v = qjs::JS_GetPropertyUint32(cx, pair, 1);
        let parsed = (|| Ok((js_string(cx, k)?, js_string(cx, v)?)))();
        qjs::JS_FreeValue(cx, k);
        qjs::JS_FreeValue(cx, v);
        parsed
    }
}

fn record(cx: *mut JsContext, init: JsValue) -> Result<Vec<(String, String)>, JsValue> {
    // SAFETY: own enumerable string keys, in order; every value is freed.
    unsafe {
        let mut tab: *mut qjs::JSPropertyEnum = ptr::null_mut();
        let mut len: u32 = 0;
        if qjs::JS_GetOwnPropertyNames(
            cx,
            &mut tab,
            &mut len,
            init,
            (qjs::JS_GPN_STRING_MASK | qjs::JS_GPN_ENUM_ONLY) as i32,
        ) != 0
        {
            return Err(qjs::JS_EXCEPTION);
        }
        let mut list = Vec::with_capacity(len as usize);
        let mut failed = None;
        for i in 0..len as usize {
            let atom = (*tab.add(i)).atom;
            let name = qjs::JS_AtomToCStringLen(cx, ptr::null_mut(), atom);
            if name.is_null() {
                failed = Some(qjs::JS_EXCEPTION);
                break;
            }
            let key = CStr::from_ptr(name).to_string_lossy().into_owned();
            qjs::JS_FreeCString(cx, name);
            let value = qjs::JS_GetProperty(cx, init, atom);
            if qjs::JS_IsException(value) {
                failed = Some(qjs::JS_EXCEPTION);
                break;
            }
            match js_string(cx, value) {
                Ok(text) => list.push((key, text)),
                Err(e) => {
                    qjs::JS_FreeValue(cx, value);
                    failed = Some(e);
                    break;
                }
            }
            qjs::JS_FreeValue(cx, value);
        }
        qjs::JS_FreePropertyEnum(cx, tab, len);
        match failed {
            Some(e) => Err(e),
            None => Ok(list),
        }
    }
}

unsafe extern "C" fn params_constructor(
    cx: *mut JsContext,
    new_target: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    match (|| {
        let list = init_list(cx, arg(argc, argv, 0))?;
        let data = Box::into_raw(Box::new(ParamsData {
            list,
            url: UNDEFINED,
        }));
        match object_with(cx, new_target, classes(cx).params, data.cast()) {
            Ok(obj) => Ok(obj),
            Err(e) => {
                drop(unsafe { Box::from_raw(data) });
                Err(e)
            }
        }
    })() {
        Ok(v) => v,
        Err(e) => e,
    }
}

unsafe extern "C" fn params_size(cx: *mut JsContext, this: JsValue, _magic: i32) -> JsValue {
    match params_data(cx, this) {
        Ok(p) => qjs::JS_NewFloat64(p.list.len() as f64),
        Err(e) => e,
    }
}

fn changed(cx: *mut JsContext, this: JsValue) -> JsValue {
    match update(cx, this) {
        Ok(()) => UNDEFINED,
        Err(e) => e,
    }
}

unsafe extern "C" fn params_append(
    cx: *mut JsContext,
    this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let k = match js_string(cx, arg(argc, argv, 0)) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let v = match js_string(cx, arg(argc, argv, 1)) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match params_data(cx, this) {
        Ok(p) => p.list.push((k, v)),
        Err(e) => return e,
    }
    changed(cx, this)
}

unsafe extern "C" fn params_delete(
    cx: *mut JsContext,
    this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let k = match js_string(cx, arg(argc, argv, 0)) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let value = if argc > 1 && !qjs::JS_IsUndefined(arg(argc, argv, 1)) {
        match js_string(cx, arg(argc, argv, 1)) {
            Ok(v) => Some(v),
            Err(e) => return e,
        }
    } else {
        None
    };
    match params_data(cx, this) {
        Ok(p) => p
            .list
            .retain(|(n, v)| *n != k || value.as_ref().is_some_and(|x| x != v)),
        Err(e) => return e,
    }
    changed(cx, this)
}

unsafe extern "C" fn params_get(
    cx: *mut JsContext,
    this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let k = match js_string(cx, arg(argc, argv, 0)) {
        Ok(k) => k,
        Err(e) => return e,
    };
    match params_data(cx, this) {
        Ok(p) => match p.list.iter().find(|(n, _)| *n == k) {
            Some((_, v)) => new_string(cx, v),
            None => qjs::JS_NULL,
        },
        Err(e) => e,
    }
}

unsafe extern "C" fn params_get_all(
    cx: *mut JsContext,
    this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let k = match js_string(cx, arg(argc, argv, 0)) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let all = match params_data(cx, this) {
        Ok(p) => p
            .list
            .iter()
            .filter(|(n, _)| *n == k)
            .map(|(_, v)| v.clone())
            .collect::<Vec<_>>(),
        Err(e) => return e,
    };
    // SAFETY: a new array; each string is taken by the property.
    unsafe {
        let array = qjs::JS_NewArray(cx);
        for (i, v) in all.iter().enumerate() {
            qjs::JS_SetPropertyUint32(cx, array, i as u32, new_string(cx, v));
        }
        array
    }
}

unsafe extern "C" fn params_has(
    cx: *mut JsContext,
    this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let k = match js_string(cx, arg(argc, argv, 0)) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let value = if argc > 1 && !qjs::JS_IsUndefined(arg(argc, argv, 1)) {
        match js_string(cx, arg(argc, argv, 1)) {
            Ok(v) => Some(v),
            Err(e) => return e,
        }
    } else {
        None
    };
    match params_data(cx, this) {
        Ok(p) => {
            let has = p
                .list
                .iter()
                .any(|(n, v)| *n == k && value.as_ref().is_none_or(|x| x == v));
            if has { qjs::JS_TRUE } else { qjs::JS_FALSE }
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn params_set(
    cx: *mut JsContext,
    this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let k = match js_string(cx, arg(argc, argv, 0)) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let v = match js_string(cx, arg(argc, argv, 1)) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match params_data(cx, this) {
        Ok(p) => match p.list.iter().position(|(n, _)| *n == k) {
            Some(i) => {
                p.list[i].1 = v;
                let mut j = 0;
                p.list.retain(|(n, _)| {
                    j += 1;
                    j - 1 <= i || *n != k
                });
            }
            None => p.list.push((k, v)),
        },
        Err(e) => return e,
    }
    changed(cx, this)
}

unsafe extern "C" fn params_sort(
    cx: *mut JsContext,
    this: JsValue,
    _argc: i32,
    _argv: *mut JsValue,
) -> JsValue {
    match params_data(cx, this) {
        // Stable, by UTF-16 code units of the names.
        Ok(p) => p
            .list
            .sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16())),
        Err(e) => return e,
    }
    changed(cx, this)
}

unsafe extern "C" fn params_to_string(
    cx: *mut JsContext,
    this: JsValue,
    _argc: i32,
    _argv: *mut JsValue,
) -> JsValue {
    match params_data(cx, this) {
        Ok(p) => new_string(cx, &serialize(&p.list)),
        Err(e) => e,
    }
}

unsafe extern "C" fn params_for_each(
    cx: *mut JsContext,
    this: JsValue,
    argc: i32,
    argv: *mut JsValue,
) -> JsValue {
    let f = arg(argc, argv, 0);
    // SAFETY: a function check of a live value.
    if unsafe { !qjs::JS_IsFunction(cx, f) } {
        return type_error(cx, "the callback is not a function");
    }
    let this_arg = arg(argc, argv, 1);
    let mut i = 0;
    loop {
        let pair = match params_data(cx, this) {
            Ok(p) => p.list.get(i).cloned(),
            Err(e) => return e,
        };
        let Some((k, v)) = pair else { break };
        let k = new_string(cx, &k);
        let v = new_string(cx, &v);
        // SAFETY: the callback owns neither string; both are freed. `this`
        // is borrowed for the call (the callback's third argument).
        let r = unsafe {
            let this = qjs::JS_DupValue(cx, this);
            let mut args = [v, k, this];
            let r = qjs::JS_Call(cx, f, this_arg, 3, args.as_mut_ptr());
            for arg in args {
                qjs::JS_FreeValue(cx, arg);
            }
            r
        };
        if unsafe { qjs::JS_IsException(r) } {
            return r;
        }
        unsafe { qjs::JS_FreeValue(cx, r) };
        i += 1;
    }
    UNDEFINED
}

unsafe extern "C" fn finalize_url(rt: *mut qjs::JSRuntime, obj: JsValue) {
    // SAFETY: the opaque is the box `new_url` made, or null if it never
    // attached.
    let p = unsafe { qjs::JS_GetOpaque(obj, qjs::JS_GetClassID(obj)) } as *mut UrlData;
    if p.is_null() {
        return;
    }
    unsafe {
        let data = Box::from_raw(p);
        qjs::JS_FreeValueRT(rt, data.params);
    }
}

unsafe extern "C" fn finalize_params(rt: *mut qjs::JSRuntime, obj: JsValue) {
    // SAFETY: as [`finalize_url`].
    let p = unsafe { qjs::JS_GetOpaque(obj, qjs::JS_GetClassID(obj)) } as *mut ParamsData;
    if p.is_null() {
        return;
    }
    unsafe {
        let data = Box::from_raw(p);
        qjs::JS_FreeValueRT(rt, data.url);
    }
}

unsafe extern "C" fn mark_url(rt: *mut qjs::JSRuntime, obj: JsValue, mark: qjs::JS_MarkFunc) {
    // SAFETY: marking reads the parameters the URL owns.
    let p = unsafe { qjs::JS_GetOpaque(obj, qjs::JS_GetClassID(obj)) } as *const UrlData;
    if !p.is_null() {
        unsafe { qjs::JS_MarkValue(rt, (*p).params, mark) };
    }
}

unsafe extern "C" fn mark_params(rt: *mut qjs::JSRuntime, obj: JsValue, mark: qjs::JS_MarkFunc) {
    // SAFETY: marking reads the URL the parameters update.
    let p = unsafe { qjs::JS_GetOpaque(obj, qjs::JS_GetClassID(obj)) } as *const ParamsData;
    if !p.is_null() {
        unsafe { qjs::JS_MarkValue(rt, (*p).url, mark) };
    }
}

fn new_function(
    cx: *mut JsContext,
    name: &CStr,
    f: unsafe extern "C" fn(*mut JsContext, JsValue, i32, *mut JsValue) -> JsValue,
    length: i32,
) -> JsValue {
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

fn accessor(
    cx: *mut JsContext,
    proto: JsValue,
    name: &CStr,
    get: unsafe extern "C" fn(*mut JsContext, JsValue, i32) -> JsValue,
    set: Option<unsafe extern "C" fn(*mut JsContext, JsValue, JsValue, i32) -> JsValue>,
    magic: i32,
) {
    // SAFETY: the functions match QuickJS's getter and setter magics; the
    // property takes both values.
    unsafe {
        type HostFn = unsafe extern "C" fn(*mut JsContext, JsValue, i32, *mut JsValue) -> JsValue;
        let getter = std::mem::transmute::<
            unsafe extern "C" fn(*mut JsContext, JsValue, i32) -> JsValue,
            HostFn,
        >(get);
        let getter = qjs::JS_NewCFunction2(
            cx,
            Some(getter),
            name.as_ptr(),
            0,
            qjs::JSCFunctionEnum_JS_CFUNC_getter_magic,
            magic,
        );
        let setter = match set {
            Some(set) => {
                let setter = std::mem::transmute::<
                    unsafe extern "C" fn(*mut JsContext, JsValue, JsValue, i32) -> JsValue,
                    HostFn,
                >(set);
                qjs::JS_NewCFunction2(
                    cx,
                    Some(setter),
                    name.as_ptr(),
                    1,
                    qjs::JSCFunctionEnum_JS_CFUNC_setter_magic,
                    magic,
                )
            }
            None => UNDEFINED,
        };
        let atom = qjs::JS_NewAtom(cx, name.as_ptr());
        qjs::JS_DefinePropertyGetSet(
            cx,
            proto,
            atom,
            getter,
            setter,
            (qjs::JS_PROP_CONFIGURABLE | qjs::JS_PROP_ENUMERABLE) as i32,
        );
        qjs::JS_FreeAtom(cx, atom);
    }
}

fn method(
    cx: *mut JsContext,
    proto: JsValue,
    name: &CStr,
    f: unsafe extern "C" fn(*mut JsContext, JsValue, i32, *mut JsValue) -> JsValue,
    length: i32,
) {
    // SAFETY: a non-enumerable method, as a Web IDL operation is.
    unsafe {
        let func = new_function(cx, name, f, length);
        qjs::JS_DefinePropertyValueStr(
            cx,
            proto,
            name.as_ptr(),
            func,
            (qjs::JS_PROP_CONFIGURABLE | qjs::JS_PROP_WRITABLE) as i32,
        );
    }
}

fn register(
    cx: *mut JsContext,
    name: &'static CStr,
    finalizer: qjs::JSClassFinalizer,
    mark: qjs::JSClassGCMark,
) -> qjs::JSClassID {
    // SAFETY: registering a class on this context's runtime.
    unsafe {
        let rt = qjs::JS_GetRuntime(cx);
        let mut id: qjs::JSClassID = 0;
        qjs::JS_NewClassID(rt, &mut id);
        let def = qjs::JSClassDef {
            class_name: name.as_ptr(),
            finalizer,
            gc_mark: mark,
            call: None,
            exotic: ptr::null_mut(),
        };
        qjs::JS_NewClass(rt, id, &def);
        id
    }
}

/// Installs `URL` and `URLSearchParams` on `cx`.
pub(crate) fn install(cx: &rquickjs::Ctx<'_>) {
    let raw = cx.as_raw().as_ptr();
    let url_id = register(raw, c"URL", Some(finalize_url), Some(mark_url));
    let params_id = register(
        raw,
        c"URLSearchParams",
        Some(finalize_params),
        Some(mark_params),
    );
    // SAFETY: the state `install` made, before the app runs.
    unsafe {
        (*state::state(raw)).url_classes = Classes {
            url: url_id,
            params: params_id,
        };
    }
    // SAFETY: prototypes and constructors are new; the globals take the
    // constructors, the classes take the prototypes.
    unsafe {
        let url_proto = qjs::JS_NewObject(raw);
        for (name, magic) in [
            (c"href", 0),
            (c"origin", 1),
            (c"protocol", 2),
            (c"username", 3),
            (c"password", 4),
            (c"host", 5),
            (c"hostname", 6),
            (c"port", 7),
            (c"pathname", 8),
            (c"search", 9),
            (c"hash", 10),
        ] {
            let set = if magic == 1 { None } else { Some(url_set as _) };
            accessor(raw, url_proto, name, url_get, set, magic);
        }
        accessor(raw, url_proto, c"searchParams", url_get, None, 11);
        method(raw, url_proto, c"toString", url_to_string, 0);
        method(raw, url_proto, c"toJSON", url_to_string, 0);
        let url_ctor = qjs::JS_NewCFunction2(
            raw,
            Some(url_constructor),
            c"URL".as_ptr(),
            1,
            qjs::JSCFunctionEnum_JS_CFUNC_constructor,
            0,
        );
        qjs::JS_SetPropertyStr(
            raw,
            url_ctor,
            c"canParse".as_ptr(),
            new_function(raw, c"canParse", url_can_parse, 1),
        );
        qjs::JS_SetPropertyStr(
            raw,
            url_ctor,
            c"parse".as_ptr(),
            new_function(raw, c"parse", url_parse, 1),
        );
        qjs::JS_SetConstructor(raw, url_ctor, url_proto);
        qjs::JS_SetClassProto(raw, url_id, url_proto);
        crate::standard::set_global(raw, c"URL", url_ctor);

        let params_proto = qjs::JS_NewObject(raw);
        accessor(raw, params_proto, c"size", params_size, None, 0);
        for (name, f, length) in [
            (
                c"append",
                params_append
                    as unsafe extern "C" fn(*mut JsContext, JsValue, i32, *mut JsValue) -> JsValue,
                2,
            ),
            (c"delete", params_delete, 1),
            (c"get", params_get, 1),
            (c"getAll", params_get_all, 1),
            (c"has", params_has, 1),
            (c"set", params_set, 2),
            (c"sort", params_sort, 0),
            (c"toString", params_to_string, 0),
            (c"forEach", params_for_each, 1),
        ] {
            method(raw, params_proto, name, f, length);
        }
        let params_ctor = qjs::JS_NewCFunction2(
            raw,
            Some(params_constructor),
            c"URLSearchParams".as_ptr(),
            0,
            qjs::JSCFunctionEnum_JS_CFUNC_constructor,
            0,
        );
        qjs::JS_SetConstructor(raw, params_ctor, params_proto);
        qjs::JS_SetClassProto(raw, params_id, params_proto);
        crate::standard::set_global(raw, c"URLSearchParams", params_ctor);
    }
}
