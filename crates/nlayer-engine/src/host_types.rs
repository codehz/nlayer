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
    /// The selection or caret of an input, in UTF-16 units
    /// (`onSelectionChange`).
    Selection {
        /// Start of the range.
        start: u32,
        /// End of the range (the caret when equal to `start`).
        end: u32,
    },
    /// An input method's composition (`onCompositionChange`).
    Composition {
        /// A composition is in progress; `text` is what is being composed.
        composing: bool,
        /// The composed text, empty once the composition ended.
        text: String,
    },
    /// Safe-area insets in points reported by the platform for a screen or
    /// modal (`onInsets`).
    Insets {
        /// Top inset.
        top: f32,
        /// Right inset.
        right: f32,
        /// Bottom inset.
        bottom: f32,
        /// Left inset.
        left: f32,
    },
    /// Why the user asked a modal to close (`onRequestClose`): `"cancel"`,
    /// `"swipe"` or `"backdropPress"`.
    CloseReason(String),
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
