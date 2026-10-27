//! Event and fetch types for a build with no QuickJS.
//!
//! The JS feature uses the same shapes from `nlayer-js`. They live here so
//! the browser engine does not link that crate.

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

/// An HTTP request as `fetch` sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Upper-case method.
    pub method: String,
    /// Absolute URL.
    pub url: String,
    /// Header names and values, in order; names may repeat.
    pub headers: Vec<(String, String)>,
    /// The request body.
    pub body: Option<Vec<u8>>,
    /// Follow redirects (the default).
    pub follow_redirects: bool,
}

/// An HTTP response, whole (v1 streams no bodies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Status code.
    pub status: u16,
    /// Reason phrase.
    pub status_text: String,
    /// Final URL, after redirects.
    pub url: String,
    /// Whether a redirect was followed.
    pub redirected: bool,
    /// Header names and values, in order.
    pub headers: Vec<(String, String)>,
    /// The body's bytes.
    pub body: Vec<u8>,
}

/// A request waiting for the engine, with the id the page knows it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRequest {
    /// The id the promise was made under.
    pub id: u32,
    /// The request.
    pub request: HttpRequest,
}
