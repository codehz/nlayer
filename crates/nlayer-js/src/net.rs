//! The `fetch` handoff: JS builds a request, the host queues it for the
//! engine, and the engine hands the response back. No sockets here: the
//! engine (or a platform) makes the request.

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

/// A request waiting for the engine, with the id JS knows it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRequest {
    /// The id the promise was made under.
    pub id: u32,
    /// The request.
    pub request: HttpRequest,
}

/// The requests JS queued since the engine last took them.
#[derive(Debug, Default)]
pub(crate) struct Queue {
    next: u32,
    pending: Vec<NetworkRequest>,
}

impl Queue {
    /// Queues `request` for the engine and returns its id.
    pub(crate) fn push(&mut self, request: HttpRequest) -> u32 {
        let id = self.next;
        self.next += 1;
        self.pending.push(NetworkRequest { id, request });
        id
    }

    /// Moves the queued requests into `out`.
    pub(crate) fn take(&mut self, out: &mut Vec<NetworkRequest>) {
        out.append(&mut self.pending);
    }
}
