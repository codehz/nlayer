//! The C ABI `web/boot.js` calls. Wasm only: one running [`PageSession`],
//! strings and bytes as pointer and length into this module's memory.
//! JS copies them before returning.
//!
//! Imports are the module `nlayer` (`nlayer_submit`, `nlayer_request_pump`,
//! `nlayer_report_error`, `nlayer_log`, `nlayer_fetch`). Entropy is
//! `env.__nlayer_random_fill`, which `nlayer-js` already imports. The
//! QuickJS wasm shim also imports `env.__rquickjs_host_now_us` (microseconds
//! since the Unix epoch, as an f64): this module has no clock of its own.
#![allow(unreachable_pub)]
use std::cell::RefCell;

use nlayer_core::{
    NodeId,
    geometry::{Edges, Size, px},
};
use nlayer_engine::HttpResponse;

use crate::{Page, PageSession};

thread_local! {
    static RUNNING: RefCell<Option<PageSession>> = const { RefCell::new(None) };
}

struct HostPage;

impl Page for HostPage {
    fn submit(&self, json: &str) {
        // SAFETY: `json` stays alive for the call. JS copies it before returning.
        unsafe { nlayer_submit(json.as_ptr(), json.len()) }
    }

    fn request_pump(&self) {
        // SAFETY: no pointers. JS schedules a pump; it does not call one here.
        unsafe { nlayer_request_pump() }
    }

    fn report_error(&self, message: &str) {
        // SAFETY: as [`Page::submit`].
        unsafe { nlayer_report_error(message.as_ptr(), message.len()) }
    }

    fn fetch(&self, request: &nlayer_engine::NetworkRequest) {
        let headers =
            serde_json::to_string(&request.request.headers).unwrap_or_else(|_| "[]".into());
        let body = request.request.body.as_deref().unwrap_or(b"");
        let has_body = u32::from(request.request.body.is_some());
        let follow = u32::from(request.request.follow_redirects);
        // SAFETY: the strings and the body stay alive for the call. JS copies
        // them before returning and does not call back into the module.
        unsafe {
            nlayer_fetch(
                request.id,
                request.request.method.as_ptr(),
                request.request.method.len(),
                request.request.url.as_ptr(),
                request.request.url.len(),
                headers.as_ptr(),
                headers.len(),
                body.as_ptr(),
                body.len(),
                has_body,
                follow,
            );
        }
    }
}

#[link(wasm_import_module = "nlayer")]
unsafe extern "C" {
    fn nlayer_submit(ptr: *const u8, len: usize);
    fn nlayer_request_pump();
    fn nlayer_report_error(ptr: *const u8, len: usize);
    fn nlayer_log(level: u32, ptr: *const u8, len: usize);
    fn nlayer_fetch(
        id: u32,
        method: *const u8,
        method_len: usize,
        url: *const u8,
        url_len: usize,
        headers: *const u8,
        headers_len: usize,
        body: *const u8,
        body_len: usize,
        has_body: u32,
        follow: u32,
    );
}

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        let line = record.args().to_string();
        let level = match record.level() {
            log::Level::Error => 0,
            log::Level::Warn => 1,
            log::Level::Info => 2,
            log::Level::Debug => 3,
            log::Level::Trace => 4,
        };
        // SAFETY: `line` stays alive for the call.
        unsafe { nlayer_log(level, line.as_ptr(), line.len()) }
    }

    fn flush(&self) {}
}

fn install_logger() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = log::set_logger(&Logger);
        log::set_max_level(log::LevelFilter::Info);
    });
}

/// Allocates `len` bytes for JS to write into. Freed with [`nlayer_dealloc`].
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Frees what [`nlayer_alloc`] returned.
///
/// # Safety
///
/// `ptr` and `len` come from one [`nlayer_alloc`] call, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlayer_dealloc(ptr: *mut u8, len: usize) {
    if len == 0 {
        return;
    }
    // SAFETY: the pointer and capacity come from [`nlayer_alloc`]. Length 0:
    // the bytes JS wrote are not dropped as elements.
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// # Safety
///
/// `ptr..ptr + len` is `len` bytes from [`nlayer_alloc`], or `len` is 0.
unsafe fn string(ptr: *const u8, len: usize) -> String {
    if len == 0 {
        return String::new();
    }
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned()
}

/// # Safety
///
/// As [`string`].
unsafe fn bytes(ptr: *const u8, len: usize) -> Vec<u8> {
    if len == 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
}

fn delay_ms(next: Option<std::time::Duration>) -> f64 {
    match next {
        Some(delay) => delay.as_secs_f64() * 1000.0,
        None => -1.0,
    }
}

fn with_session(f: impl FnOnce(&mut PageSession) -> f64) -> f64 {
    RUNNING.with(|slot| {
        let mut session = slot.borrow_mut();
        match session.as_mut() {
            Some(session) => f(session),
            None => -1.0,
        }
    })
}

/// Starts `bundle` (JS source, an ES module as `nlayer build` writes it)
/// in a `width` × `height` surface (CSS pixels). Replaces a running app
/// only when this one starts; a bundle that throws leaves the previous
/// one. Returns the delay (ms) until [`nlayer_pump`] is due, or -1.
///
/// # Safety
///
/// `ptr..ptr + len` is memory from [`nlayer_alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlayer_start(ptr: *const u8, len: usize, width: f32, height: f32) -> f64 {
    install_logger();
    // SAFETY: the caller passes a range from [`nlayer_alloc`].
    let bundle = unsafe { string(ptr, len) };
    match PageSession::start(bundle, Size::new(px(width), px(height)), HostPage) {
        Ok(session) => {
            let delay = delay_ms(session.deadline());
            RUNNING.with(|slot| *slot.borrow_mut() = Some(session));
            delay
        }
        Err(error) => {
            HostPage.report_error(&error.message);
            -1.0
        }
    }
}

/// Stops the app. A later [`nlayer_start`] runs another.
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_stop() {
    RUNNING.with(|slot| *slot.borrow_mut() = None);
}

/// Runs what is due. Returns the delay (ms) until the next pump, or -1.
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_pump() -> f64 {
    with_session(|session| delay_ms(session.pump()))
}

/// A click on view `id`. Applied at the next [`nlayer_pump`].
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_press(id: u32) {
    with_session(|session| {
        session.press(NodeId::from(id));
        -1.0
    });
}

/// The whole new text of input `id`. Applied at the next [`nlayer_pump`].
///
/// # Safety
///
/// `ptr..ptr + len` is memory from [`nlayer_alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlayer_input(id: u32, ptr: *const u8, len: usize) {
    // SAFETY: the caller passes a range from [`nlayer_alloc`].
    let text = unsafe { string(ptr, len) };
    with_session(|session| {
        session.input(NodeId::from(id), text);
        -1.0
    });
}

/// Scroll view `id` scrolled (`onScroll`). Applied at the next [`nlayer_pump`].
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_scroll(id: u32) {
    with_session(|session| {
        session.scroll(NodeId::from(id));
        -1.0
    });
}

/// The surface changed size (CSS pixels). Applied at the next [`nlayer_pump`].
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_resize(width: f32, height: f32) {
    with_session(|session| {
        session.resize(Size::new(px(width), px(height)));
        -1.0
    });
}

/// Safe-area insets and the keyboard height (CSS pixels). Applied at the
/// next [`nlayer_pump`].
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_insets(top: f32, right: f32, bottom: f32, left: f32, keyboard: f32) {
    with_session(|session| {
        session.set_insets(
            Edges {
                top: px(top),
                right: px(right),
                bottom: px(bottom),
                left: px(left),
            },
            px(keyboard),
        );
        -1.0
    });
}

/// Answers fetch `id`. `ok` 0: `status_text` is the error and the rest is
/// ignored. Otherwise `headers` is a JSON array of `[name, value]` and
/// `body` is the whole response body. Wakes the page; it pumps.
///
/// # Safety
///
/// Each range is memory from [`nlayer_alloc`], or its length is 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlayer_fetch_done(
    id: u32,
    ok: u32,
    status: u32,
    status_text: *const u8,
    status_text_len: usize,
    url: *const u8,
    url_len: usize,
    redirected: u32,
    headers: *const u8,
    headers_len: usize,
    body: *const u8,
    body_len: usize,
) {
    // SAFETY: the caller passes ranges from [`nlayer_alloc`].
    let status_text = unsafe { string(status_text, status_text_len) };
    let result = if ok == 0 {
        Err(status_text)
    } else {
        let header_json = unsafe { string(headers, headers_len) };
        match serde_json::from_str::<Vec<(String, String)>>(&header_json) {
            Ok(pairs) => Ok(HttpResponse {
                status: status as u16,
                status_text,
                url: unsafe { string(url, url_len) },
                redirected: redirected != 0,
                headers: pairs,
                body: unsafe { bytes(body, body_len) },
            }),
            Err(error) => Err(error.to_string()),
        }
    };
    RUNNING.with(|slot| {
        if let Some(session) = slot.borrow().as_ref() {
            session.complete_fetch(id, result);
        }
    });
}
