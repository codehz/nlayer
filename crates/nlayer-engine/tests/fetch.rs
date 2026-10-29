//! `fetch` end to end: a local server, the engine's HTTP, the JS response.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crossbeam_channel::{Receiver, unbounded};
use nlayer_core::{
    MountBatch, MountItem,
    geometry::{Size, px},
    text::ApproxTextEngine,
};
use nlayer_engine::{Engine, EngineConfig, MountSink};

/// A local HTTP server: `handle` gets each connection with its request line
/// and headers already read. Returns the base URL.
fn serve(
    handle: impl Fn(&mut BufReader<TcpStream>, &str, &[(String, String)]) + Send + 'static,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut headers = Vec::new();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).expect("header");
                let header = header.trim_end();
                if header.is_empty() {
                    break;
                }
                let (name, value) = header.split_once(':').expect("header line");
                headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
            }
            handle(&mut reader, line.trim_end(), &headers);
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn reply(stream: &mut BufReader<TcpStream>, status: &str, content_type: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .get_mut()
        .write_all(response.as_bytes())
        .expect("write");
}

/// A bare response, for the ones whose headers matter (`Location`).
fn reply_with(stream: &mut BufReader<TcpStream>, status: &str, headers: &[(&str, &str)]) {
    let mut response = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("Content-Length: 0\r\nConnection: close\r\n\r\n");
    stream
        .get_mut()
        .write_all(response.as_bytes())
        .expect("write");
}

struct Sink {
    tx: crossbeam_channel::Sender<MountBatch>,
}

impl MountSink for Sink {
    fn submit(&self, batch: MountBatch) {
        let _ = self.tx.send(batch);
    }
}

fn run(source: String) -> (Engine, Receiver<MountBatch>) {
    let bundle = nlayer_bundler::compile_module("App.tsx", &source).expect("it compiles");
    let (tx, rx) = unbounded();
    let engine = Engine::start(
        EngineConfig::new(bundle, Size::new(px(390.0), px(844.0))),
        || ApproxTextEngine,
        Arc::new(Sink { tx }),
    )
    .expect("the app starts");
    (engine, rx)
}

fn texts(batch: &MountBatch) -> Vec<String> {
    batch
        .items
        .iter()
        .filter_map(|item| match item {
            MountItem::Text { text, .. } => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

fn wait_for(rx: &Receiver<MountBatch>, text: &str) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        let Ok(batch) = rx.recv_timeout(left) else {
            return false;
        };
        if texts(&batch).iter().any(|t| t == text) {
            return true;
        }
    }
    false
}

#[test]
fn fetch_reads_json_and_headers() {
    let base = serve(|stream, line, _headers| {
        assert!(line.starts_with("GET /data "), "{line}");
        reply(
            stream,
            "200 OK",
            "application/json",
            r#"{"message":"hello","n":3}"#,
        );
    });
    let bundle = format!(
        r#"
import {{ mount, signal }} from "@nlayer/runtime";
import {{ Text }} from "@nlayer/ui";

function App() {{
    const shown = signal("loading");
    fetch("{base}/data")
        .then((response) => {{
            if (!response.ok) throw new Error(String(response.status));
            const type = response.headers.get("content-type");
            return response.json().then((body) => `${{body.message}} ${{body.n}} ${{type}}`);
        }})
        .then((text) => shown.set(text))
        .catch((error) => shown.set(`error ${{error.message}}`));
    return <Text>{{shown()}}</Text>;
}}
mount(App);
"#
    );
    let (engine, rx) = run(bundle);
    let first = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(texts(&first), ["loading"]);
    assert!(
        wait_for(&rx, "hello 3 application/json"),
        "the response reaches the text"
    );
    engine.stop();
}

#[test]
fn fetch_posts_a_body_and_reports_the_status() {
    let base = serve(|stream, line, headers| {
        assert!(line.starts_with("POST /echo "), "{line}");
        let length: usize = headers
            .iter()
            .find(|(name, _)| name == "content-length")
            .and_then(|(_, value)| value.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        let body = String::from_utf8(body).unwrap();
        assert_eq!(body, "ping");
        reply(stream, "201 Created", "text/plain", "pong");
    });
    let bundle = format!(
        r#"
import {{ mount, signal }} from "@nlayer/runtime";
import {{ Text }} from "@nlayer/ui";

function App() {{
    const shown = signal("loading");
    fetch("{base}/echo", {{
        method: "post",
        body: "ping",
        headers: {{ "x-test": "1" }},
    }})
        .then((response) => response.text().then((body) => `${{response.status}} ${{body}}`))
        .then((text) => shown.set(text))
        .catch((error) => shown.set(`error ${{error.message}}`));
    return <Text>{{shown()}}</Text>;
}}
mount(App);
"#
    );
    let (engine, rx) = run(bundle);
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(wait_for(&rx, "201 pong"), "the response reaches the text");
    engine.stop();
}

#[test]
fn aborting_rejects_the_fetch() {
    let base = serve(|stream, line, _headers| {
        // Slow enough for the abort to win.
        std::thread::sleep(Duration::from_millis(300));
        assert!(line.starts_with("GET /slow "), "{line}");
        reply(stream, "200 OK", "text/plain", "late");
    });
    let bundle = format!(
        r#"
import {{ mount, signal }} from "@nlayer/runtime";
import {{ Text }} from "@nlayer/ui";

function App() {{
    const shown = signal("loading");
    const controller = new AbortController();
    fetch("{base}/slow", {{ signal: controller.signal }})
        .then((response) => response.text())
        .then((text) => shown.set(text))
        .catch((error) => shown.set(error.name));
    controller.abort();
    return <Text>{{shown()}}</Text>;
}}
mount(App);
"#
    );
    let (engine, rx) = run(bundle);
    let first = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    // The abort lands before the first frame: the rejection runs as a
    // microtask of the turn that mounted the app.
    assert!(
        texts(&first).iter().any(|text| text == "AbortError") || wait_for(&rx, "AbortError"),
        "{:?}",
        texts(&first)
    );
    engine.stop();
}

#[test]
fn a_failed_connection_rejects_the_fetch() {
    // Nothing listens on port 1.
    let bundle = r#"
import { mount, signal } from "@nlayer/runtime";
import { Text } from "@nlayer/ui";

function App() {
    const shown = signal("loading");
    fetch("http://127.0.0.1:1/none")
        .then(() => shown.set("ok"))
        .catch((error) => shown.set(`error ${error.name} ${error.message}`));
    return <Text>{shown()}</Text>;
}
mount(App);
"#;
    let (engine, rx) = run(bundle.to_owned());
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let got = rx.recv_timeout(Duration::from_secs(5)).expect("a frame");
    let texts = texts(&got);
    assert!(
        texts.iter().any(|t| t.starts_with("error TypeError ")),
        "{texts:?}"
    );
    engine.stop();
}

#[test]
fn fetch_follows_a_redirect() {
    let base = serve(|stream, line, _headers| {
        if line.starts_with("GET /start ") {
            reply_with(stream, "302 Found", &[("Location", "/final")]);
        } else if line.starts_with("GET /final ") {
            reply(stream, "200 OK", "application/json", r#"{"n":5}"#);
        } else {
            panic!("unexpected request: {line}");
        }
    });
    let bundle = format!(
        r#"
import {{ mount, signal }} from "@nlayer/runtime";
import {{ Text }} from "@nlayer/ui";

function App() {{
    const shown = signal("loading");
    fetch("{base}/start")
        .then((response) =>
            response.json().then((body) =>
                `${{response.status}} ${{response.redirected}} ${{response.url.endsWith("/final")}} ${{body.n}}`))
        .then((text) => shown.set(text))
        .catch((error) => shown.set(`error ${{error.message}}`));
    return <Text>{{shown()}}</Text>;
}}
mount(App);
"#
    );
    let (engine, rx) = run(bundle);
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        wait_for(&rx, "200 true true 5"),
        "the redirect is followed and the final URL reported"
    );
    engine.stop();
}

#[test]
fn fetch_reads_bytes_and_blobs() {
    let base = serve(|stream, line, _headers| {
        assert!(line.starts_with("GET /bytes "), "{line}");
        let body = "%PDF-1.4\u{0}binary";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.get_mut().write_all(response.as_bytes()).unwrap();
    });
    let bundle = format!(
        r#"
import {{ mount, signal }} from "@nlayer/runtime";
import {{ Text }} from "@nlayer/ui";

function App() {{
    const shown = signal("loading");
    fetch("{base}/bytes")
        .then(async (response) => {{
            const blob = await response.blob();
            const bytes = await blob.arrayBuffer();
            return `${{bytes.byteLength}} ${{blob.size}} ${{blob.type}} ${{await blob.text()}}`;
        }})
        .then((text) => shown.set(text))
        .catch((error) => shown.set(`error ${{error.message}}`));
    return <Text>{{shown()}}</Text>;
}}
mount(App);
"#
    );
    let (engine, rx) = run(bundle);
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        wait_for(&rx, "15 15 application/octet-stream %PDF-1.4\u{0}binary"),
        "the bytes and the blob"
    );
    engine.stop();
}

/// An abort rejects the promise and stops the request on the network: the
/// connection closes instead of reading the body to its end.
#[test]
fn aborting_stops_the_request_on_the_network() {
    let closed = Arc::new(AtomicBool::new(false));
    let server_closed = Arc::clone(&closed);
    let base = serve(move |stream, line, _headers| {
        assert!(line.starts_with("GET /slow "), "{line}");
        stream
            .get_mut()
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 200000\r\nConnection: close\r\n\r\n")
            .unwrap();
        // The client aborts while this sleeps, waiting for the body.
        std::thread::sleep(Duration::from_millis(400));
        let body = vec![b'x'; 100_000];
        let _ = stream.get_mut().write_all(&body);
        // A closed client reads as the end.
        let mut buf = [0_u8; 1024];
        while let Ok(read) = stream.read(&mut buf) {
            if read == 0 {
                break;
            }
        }
        server_closed.store(true, Ordering::SeqCst);
    });
    let bundle = format!(
        r#"
import {{ mount, signal }} from "@nlayer/runtime";
import {{ Text }} from "@nlayer/ui";

function App() {{
    const shown = signal("loading");
    const controller = new AbortController();
    fetch("{base}/slow", {{ signal: controller.signal }})
        .then((response) => response.text())
        .then((text) => shown.set(`${{text.length}}`))
        .catch((error) => shown.set(error.name));
    setTimeout(() => controller.abort(), 100);
    return <Text>{{shown()}}</Text>;
}}
mount(App);
"#
    );
    let (engine, rx) = run(bundle);
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(wait_for(&rx, "AbortError"), "the promise rejects");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline && !closed.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        closed.load(Ordering::SeqCst),
        "the server's connection stayed open: the request was not cancelled"
    );
    engine.stop();
}
