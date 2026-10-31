//! The C ABI `web/boot.js` calls. Wasm only: one running [`PageSession`],
//! strings and bytes as pointer and length into this module's memory.
//! JS copies them before returning.
//!
//! Imports are the module `nlayer` (`nlayer_submit`, `nlayer_request_pump`,
//! `nlayer_request_frame`, `nlayer_report_error`, `nlayer_log`). The module does not
//! link QuickJS, so it imports no `env` clock or entropy. The page evaluates
//! the app and starts it with [`nlayer_browser_start`].
#![allow(unreachable_pub)]
use std::cell::RefCell;

use nlayer_core::{
    EventKind, NodeId,
    geometry::{Edges, Size, px},
};

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

    fn request_frame(&self) -> bool {
        // SAFETY: no pointers. JS schedules a frame pump; it does not call one here.
        unsafe { nlayer_request_frame() };
        true
    }

    fn report_error(&self, message: &str) {
        // SAFETY: as [`Page::submit`].
        unsafe { nlayer_report_error(message.as_ptr(), message.len()) }
    }
}

#[link(wasm_import_module = "nlayer")]
unsafe extern "C" {
    fn nlayer_submit(ptr: *const u8, len: usize);
    fn nlayer_request_pump();
    fn nlayer_request_frame();
    fn nlayer_report_error(ptr: *const u8, len: usize);
    fn nlayer_log(level: u32, ptr: *const u8, len: usize);
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

/// Starts the page-owned script. The page registered it with
/// [`nlayer_browser_initialize`] first. Returns the delay (ms) until
/// [`nlayer_pump`] is due, or -1.
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_browser_start(width: f32, height: f32) -> f64 {
    install_logger();
    match PageSession::start(String::new(), Size::new(px(width), px(height)), HostPage) {
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

/// Stops the app. A later [`nlayer_browser_start`] runs another.
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

/// A long press on view `id`. Applied at the next [`nlayer_pump`].
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_long_press(id: u32) {
    with_session(|session| {
        session.long_press(NodeId::from(id));
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

/// The selection of input `id`, in UTF-16 units (CSS pixels). Applied at
/// the next [`nlayer_pump`].
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_selection(id: u32, start: u32, end: u32) {
    with_session(|session| {
        session.selection(NodeId::from(id), start, end);
        -1.0
    });
}

/// A change of input `id`'s composition (`text` is pointer and length into
/// this module's memory, empty once it ended). Applied at the next
/// [`nlayer_pump`].
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_composition(id: u32, composing: u32, ptr: *const u8, len: usize) {
    let text = unsafe { string(ptr, len) };
    with_session(|session| {
        session.composition(NodeId::from(id), composing != 0, text);
        -1.0
    });
}

/// Scroll view `id` reported `kind` ([`EventKind`]: `onScroll` is 1,
/// `onScrollEnd` 2, drag 3 and 4, momentum 5 and 6). Lengths are CSS
/// pixels. Applied at the next [`nlayer_pump`].
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_scroll(
    id: u32,
    kind: u32,
    x: f32,
    y: f32,
    content_w: f32,
    content_h: f32,
    width: f32,
    height: f32,
) {
    let Some(kind) = EventKind::from_raw(kind as u8) else {
        return;
    };
    with_session(|session| {
        session.scroll(
            NodeId::from(id),
            kind,
            x,
            y,
            content_w,
            content_h,
            width,
            height,
        );
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

/// The browser's back button was pressed (popstate event). The browser has
/// already navigated back. Queues the back event for processing at the next
/// pump.
#[unsafe(no_mangle)]
pub extern "C" fn nlayer_back() {
    RUNNING.with_borrow(|session| {
        if let Some(s) = session {
            s.back();
        }
    });
}
