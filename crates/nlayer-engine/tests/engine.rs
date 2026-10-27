//! The engine thread: a JS app's frames, events and resizes.

use std::{sync::Arc, time::Duration};

use crossbeam_channel::{Receiver, unbounded};
use nlayer_core::{
    EventKind, MountBatch, MountItem,
    geometry::{Points, Size, px},
    text::ApproxTextEngine,
};
use nlayer_engine::{Engine, EngineConfig, MountSink};
use nlayer_js::EventPayload;

/// The counter: a press increments a signal a text shows.
const BUNDLE: &str = r#"
import { mount, signal } from "@nlayer";
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

fn start() -> (Engine, Receiver<MountBatch>) {
    let (tx, rx) = unbounded();
    let engine = Engine::start(
        EngineConfig::new(BUNDLE, Size::new(px(390.0), px(844.0))),
        || ApproxTextEngine,
        Arc::new(Sink { tx }),
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

fn frame(rx: &Receiver<MountBatch>) -> MountBatch {
    rx.recv_timeout(Duration::from_secs(5)).expect("a frame")
}

#[test]
fn a_press_updates_the_frame() {
    let (engine, rx) = start();
    let first = frame(&rx);
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
    let second = frame(&rx);
    assert_eq!(texts(&second), ["count: 1"]);
    assert!(
        !second
            .items
            .iter()
            .any(|item| matches!(item, MountItem::Create { .. })),
        "the text node is updated, not made again"
    );
    engine.stop();
}

/// A view that fills the surface.
const FILL: &str = r#"
import { mount } from "@nlayer";
import { element, prop } from "nlayer:rt";

function App() {
    const view = element("view");
    prop(view, "style", { width: "100%", height: 100 });
    return view;
}
mount(App);
"#;

#[test]
fn a_resize_lays_out_again() {
    let (tx, rx) = unbounded();
    let engine = Engine::start(
        EngineConfig::new(FILL, Size::new(px(390.0), px(844.0))),
        || ApproxTextEngine,
        Arc::new(Sink { tx }),
    )
    .unwrap();
    let first = frame(&rx);
    let view = first
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Frame { id, frame } if frame.size.width == Points(390.0) => Some(*id),
            _ => None,
        })
        .expect("the app's view");
    engine.control().resize(Size::new(px(200.0), px(400.0)));
    let second = frame(&rx);
    assert!(
        second.items.iter().any(|item| matches!(
            item,
            MountItem::Frame { id, frame }
                if *id == view && frame.size.width == Points(200.0)
        )),
        "{:?}",
        second.items
    );
    engine.stop();
}

#[test]
fn a_bundle_that_throws_fails_to_start() {
    let (tx, _rx) = unbounded::<MountBatch>();
    let error = Engine::start(
        EngineConfig::new("throw new Error('boom');", Size::new(px(10.0), px(10.0))),
        || ApproxTextEngine,
        Arc::new(Sink { tx }),
    )
    .map(|_| ())
    .expect_err("the app throws");
    assert!(error.message.contains("boom"), "{error}");
}
