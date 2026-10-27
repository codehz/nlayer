//! The web host.
//!
//! The engine (the same QuickJS runtime as the other targets, compiled for
//! the page) lays a frame out and [`Encoder`] turns it into the JSON
//! `web/nlayer.js` applies: absolutely positioned elements, one generated
//! CSS class per distinct style, a border drawn over the children, text as
//! the core's attributed spans, images, one-axis scroll views and
//! single-line inputs. [`PageSession`] is that engine on the caller's
//! thread. On wasm the crate exports it (`nlayer_start`, `nlayer_pump`) and
//! `web/boot.js` is the page that drives it.
//!
//! Extracted from `nsh-platform-web` (`encode.rs`, `text/css.rs`,
//! `src/lib.rs`, `web/nsh.js`) and cut to that set. Shadows, gradients,
//! filters, blend, outline, glass, transitions and keyframes are not
//! encoded: they are not in the v1 style whitelist, so they do not reach
//! the page. There is no glyph-positioned text here; the browser wraps
//! the attributed spans inside the content box the core measured.
//! Measurement on the page is the headless stand-in.

mod encode;
#[cfg(target_arch = "wasm32")]
mod ffi;
mod page;

pub use encode::Encoder;
pub use page::{Page, PageSession};

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
