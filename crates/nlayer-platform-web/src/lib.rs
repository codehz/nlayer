//! The web host.
//!
//! Native builds evaluate a bundle with QuickJS (`nlayer-engine` feature
//! `js`) so headless page tests can drive [`PageSession`] directly. The wasm
//! module does not: that target links feature `browser` only, the page
//! evaluates the app (`web/browser/`), and this crate never links QuickJS.
//! [`Encoder`] turns a frame into the JSON `web/nlayer.js` applies:
//! absolutely positioned elements, one generated CSS class per distinct
//! style, a border drawn over the children, text as glyph runs at the
//! positions [`text`] laid out (the browser does not wrap a run), images,
//! one-axis scroll views, virtualized lists and inputs (one line or
//! several, with selection and compositions).
//! [`PageSession`] is that engine on the caller's thread. On wasm the crate
//! exports it (`nlayer_browser_start`, `nlayer_pump`) and `web/boot.js`
//! drives it. Text widths and font metrics come from the page
//! (`measure_text`); off wasm the engine uses a fixed advance so host
//! tests lay the same lines out.
//!
//! Extracted from `nsh-platform-web` (`encode.rs`, `text.rs`, `src/lib.rs`,
//! `web/nsh.js`). Shadows, gradients, filters, blend, outline, glass,
//! transitions and keyframes are not encoded: they are not drawn.

#[cfg(target_arch = "wasm32")]
mod browser;
mod encode;
#[cfg(target_arch = "wasm32")]
mod ffi;
mod page;
mod text;

pub use encode::Encoder;
pub use page::{Page, PageSession};
pub use text::WebTextEngine;

/// The DOM mounter (`web/nlayer.js`). A page loads it and calls
/// `Player`; it is not an app module.
pub const MOUNTER_JS: &str = include_str!("../web/nlayer.js");

/// The base rules the mounter installs once (`nl-base`).
pub const BASE_CSS: &str = include_str!("../web/base.css");

/// The page that instantiates the wasm engine and drives [`MOUNTER_JS`]
/// (`web/boot.js`). Not an app module.
pub const BOOT_JS: &str = include_str!("../web/boot.js");

/// A page that loads [`BOOT_JS`] (`web/index.html`).
pub const PAGE_HTML: &str = include_str!("../web/index.html");
