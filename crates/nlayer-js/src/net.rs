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

/// What the engine does with a request: start it, or stop one whose
/// promise was rejected by its `AbortSignal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkOp {
    /// Fetch this request.
    Start(NetworkRequest),
    /// The promise of request `id` was aborted: stop it on the network.
    Cancel(u32),
}

/// The network work JS queued since the engine last took it.
#[derive(Debug, Default)]
pub(crate) struct Queue {
    next: u32,
    pending: Vec<NetworkOp>,
}

impl Queue {
    /// Queues `request` for the engine and returns its id.
    pub(crate) fn push(&mut self, request: HttpRequest) -> u32 {
        let id = self.next;
        self.next += 1;
        self.pending
            .push(NetworkOp::Start(NetworkRequest { id, request }));
        id
    }

    /// Stops request `id` (an abort the promise already rejected with).
    pub(crate) fn cancel(&mut self, id: u32) {
        self.pending.push(NetworkOp::Cancel(id));
    }

    /// Moves the queued work into `out`.
    pub(crate) fn take(&mut self, out: &mut Vec<NetworkOp>) {
        out.append(&mut self.pending);
    }
}
