//! The HTTP stack behind `fetch`, on ureq (rustls with the webpki roots):
//! one thread per request, whole bodies (v1 streams none).
//!
//! A request whose `AbortSignal` fired is cancelled through [`Fetch`]: the
//! thread stops before its next chunk and the connection closes without a
//! result handed back (the promise already rejected).

use std::{
    io::Read,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use nlayer_js::{HttpRequest, HttpResponse, NetworkRequest};
use ureq::{Agent, ResponseExt};

/// Bytes read from the response body per chunk, so a cancel is seen within
/// a chunk.
const CHUNK: usize = 64 << 10;

/// Redirects followed before the last response is handed on as it is.
const MAX_REDIRECTS: u32 = 20;

pub(crate) use super::ResultSlot;

fn agents() -> &'static (Agent, Agent) {
    static AGENTS: OnceLock<(Agent, Agent)> = OnceLock::new();
    AGENTS.get_or_init(|| {
        let agent = |redirects| {
            let agent: Agent = Agent::config_builder()
                .http_status_as_error(false)
                .max_redirects(redirects)
                .max_redirects_will_error(false)
                .timeout_connect(Some(Duration::from_secs(30)))
                .timeout_recv_response(Some(Duration::from_secs(60)))
                .build()
                .into();
            agent
        };
        (agent(MAX_REDIRECTS), agent(0))
    })
}

/// The engine's handle on one request: cancelling stops it at its next
/// chunk.
pub(crate) struct Fetch {
    cancelled: Arc<AtomicBool>,
}

impl Fetch {
    /// Stops the request: no result is handed back.
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

impl Drop for Fetch {
    /// The engine stopped or dropped the request: it no longer wants it.
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Runs `request` and keeps its result in `slot`, then wakes the engine
/// with `wake`. A cancelled request pushes nothing.
pub(crate) fn start(
    request: NetworkRequest,
    slot: ResultSlot,
    wake: impl Fn() + Send + 'static,
) -> Fetch {
    let cancelled = Arc::new(AtomicBool::new(false));
    let spawned = std::thread::Builder::new()
        .name("nlayer-fetch".to_owned())
        .spawn({
            let slot = slot.clone();
            let cancelled = Arc::clone(&cancelled);
            move || {
                match run(&request.request, &cancelled) {
                    Ok(Some(response)) => {
                        slot.lock()
                            .expect("network results")
                            .push((request.id, Ok(response)));
                    }
                    Ok(None) => return,
                    Err(error) => {
                        if cancelled.load(Ordering::SeqCst) {
                            return;
                        }
                        slot.lock()
                            .expect("network results")
                            .push((request.id, Err(error)));
                    }
                }
                wake();
            }
        });
    if let Err(error) = spawned {
        slot.lock().expect("network results").push((
            request.id,
            Err(format!("cannot start the request: {error}")),
        ));
    }
    Fetch { cancelled }
}

/// Makes the request, following redirects unless it says not to. `None`:
/// it was cancelled, so its connection is dropped without a result.
fn run(request: &HttpRequest, cancelled: &AtomicBool) -> Result<Option<HttpResponse>, String> {
    if cancelled.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let (follow, manual) = agents();
    let agent = if request.follow_redirects {
        follow
    } else {
        manual
    };
    let requested: ureq::http::Uri = request
        .url
        .parse()
        .map_err(|e| format!("{}: {e}", request.url))?;
    let mut builder = ureq::http::Request::builder()
        .method(request.method.as_str())
        .uri(requested.clone());
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let body = request.body.clone().unwrap_or_default();
    let sent = if body.is_empty() && matches!(request.method.as_str(), "GET" | "HEAD") {
        agent.run(builder.body(()).map_err(|e| e.to_string())?)
    } else {
        agent.run(builder.body(body).map_err(|e| e.to_string())?)
    };
    let mut response = sent.map_err(|e| e.to_string())?;
    let url = response.get_uri().to_string();
    let redirected = response.get_uri() != &requested;
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    // The body arrives in chunks: a cancel between them closes the
    // connection instead of reading to the end.
    let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
    let mut body = Vec::new();
    let mut chunk = [0_u8; CHUNK];
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let read = reader.read(&mut chunk);
        if cancelled.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let read = read.map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Ok(Some(HttpResponse {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or_default().to_owned(),
        url,
        redirected,
        headers,
        body,
    }))
}
