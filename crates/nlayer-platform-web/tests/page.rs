//! The page's engine: frames as the mounter's JSON, input and insets.

use std::{
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use nlayer_core::{
    EventKind,
    geometry::{Edges, Size, px},
};
use nlayer_platform_web::{Page, PageSession};

struct Record {
    batches: std::sync::Mutex<Vec<String>>,
    pumps: AtomicUsize,
}

struct Share(Arc<Record>);

impl Page for Share {
    fn submit(&self, json: &str) {
        self.0
            .batches
            .lock()
            .expect("batches")
            .push(json.to_owned());
    }

    fn request_pump(&self) {
        self.0.pumps.fetch_add(1, Ordering::SeqCst);
    }

    fn report_error(&self, message: &str) {
        panic!("the page reported {message}");
    }
}

fn session(bundle: &str, record: &Arc<Record>) -> PageSession {
    PageSession::start(
        bundle,
        Size::new(px(390.0), px(844.0)),
        Share(Arc::clone(record)),
    )
    .expect("the page starts")
}

fn items(json: &str) -> Vec<serde_json::Value> {
    serde_json::from_str(json).expect("a batch is JSON")
}

fn texts_of(json: &str) -> Vec<String> {
    items(json)
        .iter()
        .filter(|item| item[0] == "t")
        .map(|item| {
            item[8]
                .as_array()
                .unwrap()
                .iter()
                .map(|run| run[0].as_str().unwrap())
                .collect()
        })
        .collect()
}

fn last_batch(record: &Record) -> String {
    record
        .batches
        .lock()
        .expect("batches")
        .last()
        .expect("a frame")
        .clone()
}

#[test]
fn a_press_updates_the_json_frame() {
    let bundle = r#"
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
    let record = Arc::new(Record {
        batches: Default::default(),
        pumps: AtomicUsize::new(0),
    });
    let mut page = session(bundle, &record);
    let first = last_batch(&record);
    assert_eq!(texts_of(&first), ["count: 0"]);
    let id = items(&first)
        .iter()
        .find_map(|item| {
            (item[0] == "e"
                && item[2]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|event| event == "onPress"))
            .then(|| item[1].as_u64().unwrap() as u32)
        })
        .expect("a press listener");
    page.press(id);
    page.pump();
    assert_eq!(texts_of(&last_batch(&record)), ["count: 1"]);
}

#[test]
fn a_scroll_end_updates_the_json_frame() {
    let bundle = r#"
import { mount, signal } from "@nlayer/runtime";
import { element, prop, append, insert, text } from "nlayer:rt";

const y = signal(0);
function App() {
    const view = element("scroll");
    prop(view, "style", { width: 100, height: 40 });
    prop(view, "onScrollEnd", (e) => y.set(e.offset.y));
    const block = element("text");
    append(block, text("y: "));
    insert(block, () => String(y()));
    append(view, block);
    return view;
}
mount(App);
"#;
    let record = Arc::new(Record {
        batches: Default::default(),
        pumps: AtomicUsize::new(0),
    });
    let mut page = session(bundle, &record);
    let first = last_batch(&record);
    assert_eq!(texts_of(&first), ["y: 0"]);
    let id = items(&first)
        .iter()
        .find_map(|item| {
            (item[0] == "e"
                && item[2]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|event| event == "onScrollEnd"))
            .then(|| item[1].as_u64().unwrap() as u32)
        })
        .expect("a scroll end listener");
    page.scroll(
        id,
        EventKind::ScrollEnd,
        0.0,
        24.0,
        100.0,
        400.0,
        100.0,
        40.0,
    );
    page.pump();
    assert_eq!(texts_of(&last_batch(&record)), ["y: 24"]);
    page.scroll(id, EventKind::Press, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
    page.pump();
    assert_eq!(texts_of(&last_batch(&record)), ["y: 24"]);
}

#[test]
fn a_resize_and_safe_area_lay_out_again() {
    let bundle = r#"
import { mount } from "@nlayer/runtime";
import { element, prop, append } from "nlayer:rt";

function App() {
    const view = element("view");
    prop(view, "style", { width: "100%", height: 80, paddingTop: "env(safe-area-inset-top)" });
    const child = element("view");
    prop(child, "style", { width: 10, height: 10 });
    append(view, child);
    return view;
}
mount(App);
"#;
    let record = Arc::new(Record {
        batches: Default::default(),
        pumps: AtomicUsize::new(0),
    });
    let mut page = session(bundle, &record);
    let child_y = |json: &str| {
        items(json)
            .iter()
            .rev()
            .find_map(|item| {
                (item[0] == "f" && item[5].as_f64() == Some(10.0))
                    .then(|| item[3].as_f64().unwrap())
            })
            .expect("the child")
    };
    let wide = |json: &str| {
        items(json).iter().rev().any(|item| {
            item[0] == "f" && item[4].as_f64() == Some(200.0) && item[5].as_f64() == Some(80.0)
        })
    };
    assert_eq!(child_y(&last_batch(&record)), 0.0);
    page.resize(Size::new(px(200.0), px(400.0)));
    page.set_insets(
        Edges {
            top: px(24.0),
            ..Edges::default()
        },
        px(0.0),
    );
    page.pump();
    let next = last_batch(&record);
    assert!(wide(&next), "{next}");
    assert_eq!(child_y(&next), 24.0, "env(safe-area-inset-top)");
}

#[test]
fn a_bundle_that_throws_fails_to_start() {
    let record = Arc::new(Record {
        batches: Default::default(),
        pumps: AtomicUsize::new(0),
    });
    let error = PageSession::start(
        "throw new Error('boom');",
        Size::new(px(10.0), px(10.0)),
        Share(Arc::clone(&record)),
    )
    .map(|_| ())
    .expect_err("the app throws");
    assert!(error.to_string().contains("boom"), "{error}");
}

#[test]
fn a_timer_is_due_on_a_later_pump() {
    let bundle = r#"
import { mount, signal } from "@nlayer/runtime";
import { element, insert } from "nlayer:rt";

const shown = signal("a");
setTimeout(() => shown.set("b"), 0);
function App() {
    const block = element("text");
    insert(block, () => shown());
    return block;
}
mount(App);
"#;
    let record = Arc::new(Record {
        batches: Default::default(),
        pumps: AtomicUsize::new(0),
    });
    let mut page = session(bundle, &record);
    assert_eq!(texts_of(&last_batch(&record)), ["a"]);
    assert!(page.deadline().is_some(), "the timer is scheduled");
    page.pump();
    assert_eq!(texts_of(&last_batch(&record)), ["b"]);
}

#[test]
fn the_page_bootstrap_loads() {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/web/boot.test.mjs");
    let status = Command::new("node")
        .args(["--test", script])
        .status()
        .expect("node runs the bootstrap test");
    assert!(status.success(), "node --test {script}");
}
