//! UIKit host for one nlayer surface (iOS and the iOS simulator).
//!
//! [`run_app`] builds a window and runs `bundle` in it. The engine thread
//! lays the app out; each [`nlayer_core::MountBatch`] is applied on the main
//! queue. Text is measured on the engine thread with the same CoreText lines
//! the views draw.
//!
//! Extracted from `nsh-platform-ios` (`app.rs`, `surface.rs`, `mount.rs`,
//! `text.rs`, `text_view.rs`) and cut to views, text and one-axis scroll
//! views: frames, fill, radius, opacity, overflow, a static transform,
//! solid or dashed uniform borders, per-side solid borders, `onPress`,
//! `activeStyle` / `hoverStyle` and safe area. Images, inputs, refresh,
//! zoom, paging and text selection are not mounted.

#![cfg_attr(target_os = "ios", deny(unsafe_op_in_unsafe_fn))]

#[cfg(target_os = "ios")]
mod app;
#[cfg(target_os = "ios")]
mod host;
#[cfg(target_os = "ios")]
mod mount;
#[cfg(target_os = "ios")]
mod text;
#[cfg(target_os = "ios")]
mod text_view;

/// Runs `bundle` (an ES module) until the process exits.
///
/// An empty bundle shows the read error and does not start the engine.
/// Must be called on the main thread.
#[cfg(target_os = "ios")]
pub fn run_app(bundle: String) -> ! {
    app::run_app(bundle)
}

/// UIKit is not on this target.
#[cfg(not(target_os = "ios"))]
pub fn run_app(_bundle: String) -> ! {
    eprintln!("nlayer-platform-ios runs on iOS");
    std::process::exit(1);
}
