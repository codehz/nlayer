//! The same-thread engine: the page's loop, with no engine thread.

use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, atomic::AtomicUsize},
    time::Duration,
};

use crossbeam_channel::{Receiver, unbounded};
use nlayer_core::{
    EventKind, MountBatch, MountItem,
    geometry::{Points, Size, px},
    text::ApproxTextEngine,
};
use nlayer_engine::{EngineConfig, EventPayload, LocalEngine, MountSink, Wake};

/// The counter: a press increments a signal a text shows.
const BUNDLE: &str = r#"
import { mount, signal } from "@nlayer/runtime";
import { element, prop, append, insert, text } from "nlayer:rt";

const count = signal(0);
function App() {
    const view = element("view");
    prop(view, "style", { width: 100, height: 100 });
    prop(view, "onPress", () => count.set(count() + 1));
    const block = element("text");
    append(block, text("count: "));
    insert(block, () => String(count()));
    append(view, block);
    return view;
}
mount(App);
"#;

struct Sink {
    tx: crossbeam_channel::Sender<MountBatch>,
}

impl MountSink for Sink {
    fn submit(&self, batch: MountBatch) {
        let _ = self.tx.send(batch);
    }
}

fn start(bundle: &str) -> (LocalEngine<ApproxTextEngine>, Receiver<MountBatch>) {
    start_with(bundle, Arc::new(|| {}))
}

fn start_with(bundle: &str, wake: Wake) -> (LocalEngine<ApproxTextEngine>, Receiver<MountBatch>) {
    let (tx, rx) = unbounded();
    let engine = LocalEngine::start(
        EngineConfig::new(bundle, Size::new(px(390.0), px(844.0))),
        ApproxTextEngine,
        Arc::new(Sink { tx }),
        wake,
    )
    .expect("the engine starts");
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

#[test]
fn a_press_updates_the_frame_on_pump() {
    let (mut engine, rx) = start(BUNDLE);
    let first = rx.try_recv().expect("the first frame is ready");
    assert_eq!(texts(&first), ["count: 0"]);
    let id = first
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Events { id, events } if events.contains(EventKind::Press) => Some(*id),
            _ => None,
        })
        .expect("a press listener");
    engine
        .control()
        .dispatch_event(id, EventKind::Press, EventPayload::None);
    assert!(rx.try_recv().is_err(), "input waits for pump");
    engine.pump();
    let second = rx.try_recv().expect("the press's frame");
    assert_eq!(texts(&second), ["count: 1"]);
    assert!(
        !second
            .items
            .iter()
            .any(|item| matches!(item, MountItem::Create { .. })),
        "the text node is updated, not made again"
    );
}

/// A view that fills the surface.
const FILL: &str = r#"
import { mount } from "@nlayer/runtime";
import { element, prop } from "nlayer:rt";

function App() {
    const view = element("view");
    prop(view, "style", { width: "100%", height: 100 });
    return view;
}
mount(App);
"#;

#[test]
fn a_resize_lays_out_again_on_pump() {
    let (mut engine, rx) = start(FILL);
    let first = rx.try_recv().expect("the first frame");
    let view = first
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Frame { id, frame } if frame.size.width == Points(390.0) => Some(*id),
            _ => None,
        })
        .expect("the app's view");
    engine.control().resize(Size::new(px(200.0), px(400.0)));
    engine.pump();
    let second = rx.try_recv().expect("the resized frame");
    assert!(
        second.items.iter().any(|item| matches!(
            item,
            MountItem::Frame { id, frame }
                if *id == view && frame.size.width == Points(200.0)
        )),
        "{:?}",
        second.items
    );
}

#[test]
fn a_bundle_that_throws_fails_to_start() {
    let (tx, _rx) = unbounded::<MountBatch>();
    let error = LocalEngine::start(
        EngineConfig::new("throw new Error('boom');", Size::new(px(10.0), px(10.0))),
        ApproxTextEngine,
        Arc::new(Sink { tx }),
        Arc::new(|| {}),
    )
    .map(|_| ())
    .expect_err("the app throws");
    assert!(error.message.contains("boom"), "{error}");
}

const TIMER: &str = r#"
import { mount, signal } from "@nlayer/runtime";
import { element, insert } from "nlayer:rt";

const label = signal("before");
setTimeout(() => label.set("after"), 40);
function App() {
    const block = element("text");
    insert(block, () => label());
    return block;
}
mount(App);
"#;

#[test]
fn a_timer_waits_for_the_deadline() {
    let (mut engine, rx) = start(TIMER);
    let first = rx.try_recv().expect("the first frame");
    assert_eq!(texts(&first), ["before"]);
    let wait = engine.deadline().expect("the timeout");
    engine.pump();
    assert!(rx.try_recv().is_err(), "the timer has not fired");
    std::thread::sleep(wait + Duration::from_millis(15));
    engine.pump();
    let second = rx.try_recv().expect("the timer's frame");
    assert_eq!(texts(&second), ["after"]);
}

/// A local HTTP server. `handle` gets each connection with its request line
/// and headers already read. Returns the base URL.
fn serve(handle: impl Fn(&mut BufReader<TcpStream>, &str) + Send + 'static) -> String {
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
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).expect("header");
                if header.trim_end().is_empty() {
                    break;
                }
            }
            handle(&mut reader, line.trim_end());
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn a_fetch_wakes_the_caller() {
    let base = serve(|stream, line| {
        assert!(line.starts_with("GET /data "), "{line}");
        let body = r#"{"message":"hello"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .get_mut()
            .write_all(response.as_bytes())
            .expect("write");
    });
    let bundle = format!(
        r#"
import {{ mount, signal }} from "@nlayer/runtime";
import {{ element, insert }} from "nlayer:rt";

const shown = signal("loading");
fetch("{base}/data")
    .then((response) => response.json())
    .then((body) => shown.set(body.message));
function App() {{
    const block = element("text");
    insert(block, () => shown());
    return block;
}}
mount(App);
"#
    );
    let (wake_tx, wake_rx) = unbounded();
    let (mut engine, rx) = start_with(
        &bundle,
        Arc::new(move || {
            let _ = wake_tx.send(());
        }),
    );
    let first = rx.try_recv().expect("the first frame");
    assert_eq!(texts(&first), ["loading"]);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut saw = false;
    while std::time::Instant::now() < deadline {
        while let Ok(batch) = rx.try_recv() {
            if texts(&batch).iter().any(|text| text == "hello") {
                saw = true;
            }
        }
        if saw {
            break;
        }
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        wake_rx
            .recv_timeout(left)
            .expect("a finished fetch wakes the page");
        engine.pump();
    }
    assert!(saw, "the response reaches the text");
}

struct RequestSink {
    tx: crossbeam_channel::Sender<MountBatch>,
    requests: Arc<AtomicUsize>,
}

impl MountSink for RequestSink {
    fn submit(&self, batch: MountBatch) {
        let _ = self.tx.send(batch);
    }

    fn request_frame(&self) -> bool {
        self.requests
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    }
}

fn start_with_requests(
    bundle: &str,
) -> (
    LocalEngine<ApproxTextEngine>,
    Receiver<MountBatch>,
    Arc<AtomicUsize>,
) {
    let (tx, rx) = unbounded();
    let requests = Arc::new(AtomicUsize::new(0));
    let engine = LocalEngine::start(
        EngineConfig::new(bundle, Size::new(px(390.0), px(844.0))),
        ApproxTextEngine,
        Arc::new(RequestSink {
            tx,
            requests: Arc::clone(&requests),
        }),
        Arc::new(|| {}),
    )
    .expect("the engine starts");
    (engine, rx, requests)
}

/// A local host binds overscan a frame at a time: each pump is one frame.
#[test]
fn a_local_host_binds_overscan_a_frame_at_a_time() {
    const APP: &str = r#"
import { mount } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";
const rows = Array.from({ length: 200 }, (_, i) => i);
mount(() => List({
    each: rows,
    estimatedItemSize: 50,
    style: { flex: 1 },
    children: (item) => View({
        style: { height: 50 },
        children: Array.from({ length: 8 }, (_, k) => Text({ children: "row " + item() + " " + k })),
    }),
}));
"#;
    let (mut engine, rx, requests) = start_with_requests(APP);
    // After start, a frame was requested.
    assert!(
        requests.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "frame requested after start"
    );
    // The engine waits for the page's frame, not an immediate pump.
    assert!(
        engine.deadline().is_none(),
        "waiting for a frame, not a timer"
    );
    // Pump in a loop: each pump is one frame. The start frame is in `rx`
    // already; the pumps must add the overscan.
    let first = texts(&rx.try_recv().expect("the start frame")).len();
    let mut total = first;
    for _ in 0..100 {
        engine.pump();
        if let Ok(batch) = rx.try_recv() {
            total += texts(&batch).len();
        } else {
            // No batch: done.
            break;
        }
    }
    assert!(total > first, "{first} first, {total} in all");
}
