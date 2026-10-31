//! EngineControl methods for navigation and back routing.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crossbeam_channel::{Receiver, unbounded};
use nlayer_core::{
    MountBatch, NodeId, ROOT_ID,
    geometry::{Edges, Points, Size, px},
    text::ApproxTextEngine,
};
use nlayer_engine::{Engine, EngineConfig, MountSink};

struct ControlSink {
    tx: crossbeam_channel::Sender<MountBatch>,
    back_unhandled_called: Arc<AtomicBool>,
}

impl MountSink for ControlSink {
    fn submit(&self, batch: MountBatch) {
        let _ = self.tx.send(batch);
    }

    fn back_unhandled(&self) {
        self.back_unhandled_called.store(true, Ordering::SeqCst);
    }
}

fn start(bundle: &str) -> (Engine, Receiver<MountBatch>, Arc<AtomicBool>) {
    let (tx, rx) = unbounded();
    let back_flag = Arc::new(AtomicBool::new(false));
    let sink = Arc::new(ControlSink {
        tx,
        back_unhandled_called: Arc::clone(&back_flag),
    });
    let engine = Engine::start(
        EngineConfig::new(bundle, Size::new(px(390.0), px(844.0))),
        || ApproxTextEngine,
        sink,
    )
    .expect("the engine starts");
    (engine, rx, back_flag)
}

fn frame(rx: &Receiver<MountBatch>) -> MountBatch {
    rx.recv_timeout(Duration::from_secs(5)).expect("a frame")
}

#[test]
fn set_root_size_and_screen_header() {
    const BUNDLE: &str = r#"
import { mount } from "@nlayer/runtime";
import { element } from "nlayer:rt";
function App() {
    return element("view");
}
mount(App);
"#;
    let (engine, rx) = {
        let (tx, rx) = unbounded();
        let sink = Arc::new(ControlSink {
            tx,
            back_unhandled_called: Arc::new(AtomicBool::new(false)),
        });
        let engine = Engine::start(
            EngineConfig::new(BUNDLE, Size::new(px(390.0), px(844.0))),
            || ApproxTextEngine,
            sink,
        )
        .expect("the engine starts");
        (engine, rx)
    };

    // Consume initial frame
    let _ = frame(&rx);

    // Test set_root_size
    engine.control().set_root_size(
        100,
        Size::new(px(300.0), px(600.0)),
        Edges {
            top: px(20.0),
            right: px(20.0),
            bottom: px(40.0),
            left: px(20.0),
        },
    );

    // Test set_screen_header
    engine.control().set_screen_header(100, Some(Points(44.0)));

    // Allow processing
    std::thread::sleep(Duration::from_millis(50));

    engine.stop();
}

#[test]
fn back_reaches_platform_when_no_handler() {
    const BUNDLE: &str = r#"
import { mount } from "@nlayer/runtime";
import { element } from "nlayer:rt";
function App() {
    return element("view");
}
mount(App);
"#;
    let (engine, rx, back_flag) = start(BUNDLE);

    // Consume initial frame
    let _ = frame(&rx);

    // Send back with no BackHandler registered
    engine.control().back();

    // Wait for processing
    std::thread::sleep(Duration::from_millis(100));

    // back_unhandled should have been called
    assert!(
        back_flag.load(Ordering::SeqCst),
        "back_unhandled should be called when no handler"
    );

    engine.stop();
}

#[test]
fn back_handled_by_listener() {
    const BUNDLE: &str = r#"
import { mount } from "@nlayer/runtime";
import { BackHandler } from "@nlayer/ui";
import { element } from "nlayer:rt";

let handled = false;
BackHandler.addEventListener(() => {
    handled = true;
    return true;
});

function App() {
    return element("view");
}
mount(App);
"#;
    let (engine, rx, back_flag) = start(BUNDLE);

    // Consume initial frame
    let _ = frame(&rx);

    // Send back with BackHandler registered
    engine.control().back();

    // Wait for processing
    std::thread::sleep(Duration::from_millis(100));

    // back_unhandled should NOT be called since listener handled it
    assert!(
        !back_flag.load(Ordering::SeqCst),
        "back_unhandled should not be called when listener handles it"
    );

    engine.stop();
}

#[test]
fn back_priority_listener_not_handling() {
    const BUNDLE: &str = r#"
import { mount } from "@nlayer/runtime";
import { BackHandler } from "@nlayer/ui";
import { element } from "nlayer:rt";

// Listener that doesn't handle (returns false/undefined)
BackHandler.addEventListener(() => {
    return false;
});

function App() {
    return element("view");
}
mount(App);
"#;
    let (engine, rx, back_flag) = start(BUNDLE);

    // Consume initial frame
    let _ = frame(&rx);

    // Send back with non-handling listener
    engine.control().back();

    // Wait for processing
    std::thread::sleep(Duration::from_millis(100));

    // back_unhandled should be called since listener didn't handle
    assert!(
        back_flag.load(Ordering::SeqCst),
        "back_unhandled should be called when listener returns false"
    );

    engine.stop();
}
