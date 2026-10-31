//! EngineControl methods for navigation and back routing.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crossbeam_channel::{Receiver, unbounded};
use nlayer_core::{
    MountBatch,
    geometry::{Size, px},
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
fn nav_back_pops_second_screen() {
    const BUNDLE: &str = r#"
import { mount } from "@nlayer/runtime";
import { createNavStack, NavStack, NavPage } from "@nlayer/ui";
import { element } from "nlayer:rt";
const state = createNavStack([{ name: "a" }, { name: "b" }]);
function App() {
    return NavStack({ 
        state, 
        children: (route) => NavPage({ 
            title: route.name, 
            children: element("view", { testID: route.name }) 
        }) 
    });
}
mount(App);
"#;
    let (engine, rx, back_flag) = start(BUNDLE);
    let _ = frame(&rx);

    back_flag.store(false, Ordering::SeqCst);
    engine.control().back();

    // Wait for frame with route change
    let batch = frame(&rx);

    // Back should be handled (stack popped), not unhandled
    assert!(
        !back_flag.load(Ordering::SeqCst),
        "back should be handled by nav stack"
    );
    // Verify screen removal in batch (second screen removed)
    assert!(
        batch
            .items
            .iter()
            .any(|item| matches!(item, nlayer_core::MountItem::Remove { .. })),
        "expected screen removal in batch"
    );

    engine.stop();
}

#[test]
fn nav_back_unhandled_at_root() {
    const BUNDLE: &str = r#"
import { mount } from "@nlayer/runtime";
import { createNavStack, NavStack, NavPage } from "@nlayer/ui";
import { element } from "nlayer:rt";
const state = createNavStack([{ name: "a" }]);
function App() {
    return NavStack({ 
        state, 
        children: (route) => NavPage({ 
            title: route.name, 
            children: element("view", { testID: route.name }) 
        }) 
    });
}
mount(App);
"#;
    let (engine, rx, back_flag) = start(BUNDLE);
    let _ = frame(&rx);

    back_flag.store(false, Ordering::SeqCst);
    engine.control().back();

    // Allow processing
    std::thread::sleep(Duration::from_millis(50));

    // At root, back should be unhandled
    assert!(
        back_flag.load(Ordering::SeqCst),
        "back should be unhandled at root"
    );

    engine.stop();
}

#[test]
fn nav_back_blocked_by_pop_on_back_false() {
    const BUNDLE: &str = r#"
import { mount } from "@nlayer/runtime";
import { createNavStack, NavStack, NavPage } from "@nlayer/ui";
import { element } from "nlayer:rt";
const state = createNavStack([{ name: "a" }, { name: "b" }]);
function App() {
    return NavStack({ 
        state, 
        children: (route) => NavPage({ 
            title: route.name,
            popOnBack: route.name === "b" ? false : undefined,
            children: element("view", { testID: route.name }) 
        }) 
    });
}
mount(App);
"#;
    let (engine, rx, back_flag) = start(BUNDLE);
    let _ = frame(&rx);

    back_flag.store(false, Ordering::SeqCst);
    engine.control().back();

    // Wait for processing
    std::thread::sleep(Duration::from_millis(50));

    // Back should be handled but not pop the stack
    assert!(!back_flag.load(Ordering::SeqCst), "back should be handled");

    engine.stop();
}

#[test]
fn dialog_back_closes_modal() {
    const BUNDLE: &str = r#"
import { mount, signal } from "@nlayer/runtime";
import { createNavStack, NavStack, NavPage, Dialog } from "@nlayer/ui";
import { element, insert } from "nlayer:rt";
const state = createNavStack([{ name: "a" }]);
const open = signal(true);
function App() {
    const view = element("view");
    insert(view, [
        NavStack({ 
            state, 
            children: (route) => NavPage({ 
                title: route.name, 
                children: element("view", { testID: route.name }) 
            }) 
        }),
        Dialog({ 
            open: open(), 
            children: element("view", { testID: "dialog" }) 
        })
    ]);
    return view;
}
mount(App);
"#;
    let (engine, rx, back_flag) = start(BUNDLE);
    let _ = frame(&rx);

    back_flag.store(false, Ordering::SeqCst);
    engine.control().back();

    // Wait for modal close
    let batch = frame(&rx);

    // Back should be handled (modal closed), not unhandled
    assert!(
        !back_flag.load(Ordering::SeqCst),
        "back should be handled by dialog"
    );
    // Verify modal removal in batch
    assert!(
        batch
            .items
            .iter()
            .any(|item| matches!(item, nlayer_core::MountItem::Remove { .. })),
        "expected modal removal in batch"
    );

    engine.stop();
}

#[test]
fn back_handler_at_root_prevents_unhandled() {
    const BUNDLE: &str = r#"
import { mount } from "@nlayer/runtime";
import { createNavStack, NavStack, NavPage, BackHandler } from "@nlayer/ui";
import { element } from "nlayer:rt";
const state = createNavStack([{ name: "a" }]);
BackHandler.addEventListener(() => true);
function App() {
    return NavStack({ 
        state, 
        children: (route) => NavPage({ 
            title: route.name, 
            children: element("view", { testID: route.name }) 
        }) 
    });
}
mount(App);
"#;
    let (engine, rx, back_flag) = start(BUNDLE);
    let _ = frame(&rx);

    back_flag.store(false, Ordering::SeqCst);
    engine.control().back();

    // Allow processing
    std::thread::sleep(Duration::from_millis(50));

    // BackHandler listener handled it, not unhandled
    assert!(
        !back_flag.load(Ordering::SeqCst),
        "back should be handled by BackHandler listener"
    );

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
