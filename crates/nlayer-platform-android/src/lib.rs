//! Android host for one nlayer surface.
//!
//! The engine thread lays the app out and measures text with
//! `StaticLayout`. Each [`nlayer_core::MountBatch`] is [`encode`]d into one
//! little-endian word stream plus an object table. The UI thread applies
//! that stream.
//!
//! Extracted from `nsh-platform-android` (`encode.rs`, `host.rs`, `text.rs`,
//! `java.rs`) and cut to views, text, images, one-axis scroll, virtualized lists and
//! a text field (one line or several, with selection and compositions). `onPress` is the view's own touch: a nested span wins
//! over its text view, and a view that does not take one leaves the touch
//! for the nearest ancestor that does. A scroll view reports `onScroll` (at
//! most once per frame), drag and momentum, with the content offset, content
//! size and visible size. `nlayer test` drives the mounted tree through the
//! view system's own touch, input connection and scroll view.

mod encode;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
mod scene;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
mod snapshot;

#[cfg(target_os = "android")]
mod driver;
#[cfg(target_os = "android")]
mod host;
#[cfg(target_os = "android")]
mod java;
#[cfg(target_os = "android")]
mod text;

pub use encode::{Encoded, Object, encode};

#[cfg(target_os = "android")]
use std::ffi::c_void;

#[cfg(target_os = "android")]
use jni::{JavaVM, sys::jint};

/// Called by the VM when `System.loadLibrary("nlayer")` runs.
///
/// # Safety
///
/// The VM calls this once, with a live `JavaVM`.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "system" fn JNI_OnLoad(vm: JavaVM, _: *mut c_void) -> jint {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
    if let Err(err) = java::init(vm) {
        log::error!("nlayer: JNI setup failed: {err}");
    }
    jni::sys::JNI_VERSION_1_6
}

#[cfg(target_os = "android")]
struct Logger;

#[cfg(target_os = "android")]
static LOGGER: Logger = Logger;

#[cfg(target_os = "android")]
unsafe extern "C" {
    fn __android_log_write(priority: i32, tag: *const i8, text: *const i8) -> i32;
}

#[cfg(target_os = "android")]
impl log::Log for Logger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        let message = std::ffi::CString::new(record.args().to_string()).unwrap_or_default();
        let priority = match record.level() {
            log::Level::Error => 6,
            log::Level::Warn => 5,
            _ => 4,
        };
        unsafe {
            __android_log_write(priority, c"nlayer".as_ptr().cast(), message.as_ptr().cast());
        }
    }

    fn flush(&self) {}
}
