//! The HTTP stack behind `fetch`, on ureq (rustls with the webpki roots):
//! one thread per request, whole bodies (v1 streams none).

use std::{
    io::Read,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use nlayer_js::{HttpRequest, HttpResponse, NetworkRequest};
use ureq::{Agent, ResponseExt};

/// Redirects followed before the last response is handed on as it is.
const MAX_REDIRECTS: u32 = 20;

/// A request's result, waiting for the engine thread.
pub(crate) type ResultSlot = Arc<Mutex<Vec<(u32, Result<HttpResponse, String>)>>>;

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

/// Runs `request` and keeps its result in `slot`, then wakes the engine
/// with `wake`.
pub(crate) fn start(request: NetworkRequest, slot: ResultSlot, wake: impl Fn() + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("nlayer-fetch".to_owned())
        .spawn({
            let slot = slot.clone();
            move || {
                let result = run(&request.request);
                slot.lock()
                    .expect("network results")
                    .push((request.id, result));
                wake();
            }
        });
    if let Err(error) = spawned {
        slot.lock().expect("network results").push((
            request.id,
            Err(format!("cannot start the request: {error}")),
        ));
    }
}

/// Makes the request, following redirects unless it says not to.
fn run(request: &HttpRequest) -> Result<HttpResponse, String> {
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
    let mut body = Vec::new();
    response
        .body_mut()
        .with_config()
        .limit(u64::MAX)
        .reader()
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    Ok(HttpResponse {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or_default().to_owned(),
        url,
        redirected,
        headers,
        body,
    })
}
