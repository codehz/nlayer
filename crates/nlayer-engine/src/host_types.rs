//! Event types for a build with no QuickJS.
//!
//! The JS feature uses the same shapes from `nlayer-js`. They live here so
//! the browser engine does not link that crate. `fetch` is not one of them:
//! the page evaluates the app, so the page's `fetch` is the browser's.

/// What a native view reports to the page with an event.
#[derive(Debug, Clone, PartialEq)]
pub enum EventPayload {
    /// Nothing (`onPress`, scroll events).
    None,
    /// The input's text (`onChange`).
    Text(String),
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
