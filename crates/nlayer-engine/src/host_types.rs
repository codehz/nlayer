//! Event types for a build with no QuickJS.
//!
//! The JS feature uses the same shapes from `nlayer-js`. They live here so
//! the browser engine does not link that crate. `fetch` is not one of them:
//! the page evaluates the app, so the page's `fetch` is the browser's.

/// What a native view reports to the page with an event.
#[derive(Debug, Clone, PartialEq)]
pub enum EventPayload {
    /// Nothing (`onPress`).
    None,
    /// The input's text (`onChange`).
    Text(String),
    /// Where a scroll view is, in points. `x` and `y` are the content
    /// offset (negative while iOS bounces). `content_w` and `content_h` are
    /// the content size. `width` and `height` are the visible size.
    Scroll {
        /// Content offset x.
        x: f32,
        /// Content offset y.
        y: f32,
        /// Content width.
        content_w: f32,
        /// Content height.
        content_h: f32,
        /// Visible width.
        width: f32,
        /// Visible height.
        height: f32,
    },
}

/// The operating system a surface runs on (`platform.os`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Os {
    /// A browser page.
    #[default]
    Web,
    /// iOS.
    Ios,
    /// Android.
    Android,
}

impl Os {
    /// `platform.os`: `"web"`, `"ios"` or `"android"`.
    pub const fn name(self) -> &'static str {
        match self {
            Os::Web => "web",
            Os::Ios => "ios",
            Os::Android => "android",
        }
    }
}
