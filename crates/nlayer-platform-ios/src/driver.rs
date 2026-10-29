//! The socket `nlayer test --target ios` drives.
//!
//! The test runner listens; this process connects and answers one JSON line
//! per step. A press is a touch UIKit hit-tests. Typing goes through the
//! software keyboard into the focused field. A scroll sets the scroll
//! view's own offset.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use objc2::{MainThreadMarker, msg_send};
use objc2_core_foundation::{CGFloat, CGPoint};
use objc2_ui_kit::{UIView, UIWindow};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mount::Mounter;
use crate::pin::Pin;
use crate::scene::{self, Press, Query, Scene};
use crate::synth::{self, Phase};

const TAP: Duration = Duration::from_millis(60);
const SETTLE: Duration = Duration::from_millis(300);
const KEY: Duration = Duration::from_millis(40);

pub(crate) fn start(pin: Pin) {
    if pin.port == 0 || pin.host.is_empty() {
        return;
    }
    thread::Builder::new()
        .name("nlayer-test".into())
        .spawn(move || serve(pin))
        .ok();
}

fn serve(pin: Pin) {
    let address = format!("{}:{}", pin.host, pin.port);
    let Some(stream) = connect(&address) else {
        eprintln!("nlayer test: could not reach {address}");
        return;
    };
    let mut writer = stream;
    let mut reader = match writer.try_clone() {
        Ok(stream) => BufReader::new(stream),
        Err(error) => {
            eprintln!("nlayer test: {error}");
            return;
        }
    };
    if let Err(error) = wait_until_mounted() {
        let _ = writeln!(writer, "{}", fail(&error));
        return;
    }
    if writeln!(writer, "{}", ok(json!("ready"))).is_err() {
        return;
    }
    let _ = writer.flush();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let response = step(line.trim());
        if writeln!(writer, "{response}").is_err() {
            return;
        }
        let _ = writer.flush();
    }
}

fn connect(address: &str) -> Option<TcpStream> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(stream) = TcpStream::connect(address) {
            let _ = stream.set_nodelay(true);
            return Some(stream);
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

fn wait_until_mounted() -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match on_ui(mounted)? {
            Ok(()) => return Ok(()),
            Err(error) if error == "wait" => {}
            Err(error) => return Err(error),
        }
        if Instant::now() >= deadline {
            return Err("the app did not mount".into());
        }
        thread::sleep(Duration::from_millis(30));
    }
}

fn mounted() -> Result<(), String> {
    if let Some(error) = crate::host::boot_error() {
        return Err(error);
    }
    let mounter = crate::host::mounter().ok_or_else(|| "wait".to_owned())?;
    let scene = mounter.borrow().scene();
    let children = scene
        .tree()
        .get("children")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if children == 0 {
        Err("wait".into())
    } else {
        Ok(())
    }
}

#[derive(Deserialize)]
struct Request {
    op: String,
    #[serde(default)]
    selector: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    distance: f32,
}

fn step(line: &str) -> String {
    match dispatch(line) {
        Ok(value) => ok(value),
        Err(error) => fail(&error),
    }
}

fn dispatch(line: &str) -> Result<Value, String> {
    let request: Request = serde_json::from_str(line).map_err(|error| error.to_string())?;
    match request.op.as_str() {
        "texts" => on_ui(|| scene().map(|scene| json!(scene.texts())))?,
        "inputs" => on_ui(|| scene().map(|scene| json!(scene.inputs())))?,
        "tree" => on_ui(|| scene().map(|scene| scene.tree()))?,
        "query" => on_ui(move || {
            scene().map(|scene| match scene.query(&request.selector) {
                Query::Ready => json!({"state": "ready"}),
                Query::Wait => json!({"state": "wait"}),
                Query::Fail(error) => json!({"state": "fail", "error": error}),
            })
        })?,
        "press" => press(request.selector),
        "type" => type_text(request.selector, request.text),
        "scroll" => scroll(request.selector, request.distance),
        other => Err(format!("unknown test op `{other}`")),
    }
}

fn scene() -> Result<Scene, String> {
    let mounter = crate::host::mounter().ok_or_else(|| "the app is not up".to_owned())?;
    Ok(mounter.borrow().scene())
}

fn press(selector: String) -> Result<Value, String> {
    let point = on_ui(move || -> Result<Point, String> {
        let mounter = crate::host::mounter().ok_or_else(|| "the app is not up".to_owned())?;
        let mounter = mounter.borrow();
        let scene = mounter.scene();
        let press = scene.press(&selector)?;
        let (window, point) = point_of(&mounter, press)?;
        if !synth::touch(&window, point.x, point.y, Phase::Down) {
            return Err("the simulator did not take the touch".into());
        }
        Ok(point)
    })??;
    thread::sleep(TAP);
    on_ui(move || -> Result<Value, String> {
        let mounter = crate::host::mounter().ok_or_else(|| "the app is not up".to_owned())?;
        let mounter = mounter.borrow();
        let window = window_of(&mounter)?;
        if !synth::touch(&window, point.x, point.y, Phase::Up) {
            return Err("the simulator did not take the touch".into());
        }
        Ok(json!(null))
    })??;
    thread::sleep(SETTLE);
    Ok(Value::Null)
}

fn type_text(selector: String, text: String) -> Result<Value, String> {
    on_ui({
        let selector = selector.clone();
        move || -> Result<(), String> {
            let field = {
                let mounter =
                    crate::host::mounter().ok_or_else(|| "the app is not up".to_owned())?;
                let mounter = mounter.borrow();
                let id = mounter.scene().type_target(&selector)?;
                mounter
                    .field(id)
                    .ok_or_else(|| format!("`{selector}` is not an input"))?
            };
            let _: bool = unsafe { msg_send![&*field, becomeFirstResponder] };
            Ok(())
        }
    })??;
    thread::sleep(KEY);
    for ch in text.chars() {
        let piece = ch.to_string();
        on_ui(move || -> Result<(), String> {
            let mounter = crate::host::mounter().ok_or_else(|| "the app is not up".to_owned())?;
            let window = window_of(&mounter.borrow())?;
            synth::insert_text(&window, &piece);
            Ok(())
        })??;
        thread::sleep(KEY);
    }
    thread::sleep(SETTLE);
    Ok(Value::Null)
}

fn scroll(selector: String, distance: f32) -> Result<Value, String> {
    on_ui(move || -> Result<Value, String> {
        let (scroll, x, y) = {
            let mounter = crate::host::mounter().ok_or_else(|| "the app is not up".to_owned())?;
            let mounter = mounter.borrow();
            let scene = mounter.scene();
            let id = scene.scroll_target(&selector)?;
            let scroll = mounter
                .scroll(id)
                .ok_or_else(|| format!("`{selector}` is not in a scroll view"))?;
            let offset = scroll.contentOffset();
            let content = scroll.contentSize();
            let bounds = scroll.bounds().size;
            let horizontal = scroll.alwaysBounceHorizontal();
            let (x, y) = if horizontal {
                (
                    scene::next_offset(
                        offset.x as f32,
                        distance,
                        content.width as f32,
                        bounds.width as f32,
                    ),
                    offset.y as f32,
                )
            } else {
                (
                    offset.x as f32,
                    scene::next_offset(
                        offset.y as f32,
                        distance,
                        content.height as f32,
                        bounds.height as f32,
                    ),
                )
            };
            (scroll, x, y)
        };
        scroll.setContentOffset(CGPoint::new(x as CGFloat, y as CGFloat));
        // The offset is the end of a drag that glides. The delegate reports
        // the same callbacks a finger would. The mounter borrow is gone, so
        // the delegate's `try_borrow` succeeds.
        if let Some(delegate) = scroll.delegate() {
            let scroll = &*scroll;
            let _: () = unsafe { msg_send![&*delegate, scrollViewWillBeginDragging: scroll] };
            let _: () = unsafe {
                msg_send![&*delegate, scrollViewDidEndDragging: scroll, willDecelerate: true]
            };
            let _: () = unsafe { msg_send![&*delegate, scrollViewDidEndDecelerating: scroll] };
        }
        Ok(json!(null))
    })?
}

struct Point {
    x: f64,
    y: f64,
}

fn point_of(
    mounter: &Mounter,
    press: Press,
) -> Result<(objc2::rc::Retained<UIWindow>, Point), String> {
    let window = window_of(mounter)?;
    let point = match press {
        Press::View(id) => {
            let view = mounter
                .view_of(id)
                .ok_or_else(|| "the view is gone".to_owned())?;
            let bounds = view.bounds();
            let mid = CGPoint::new(
                bounds.origin.x + bounds.size.width / 2.0,
                bounds.origin.y + bounds.size.height / 2.0,
            );
            to_window(&view, mid)
        }
        Press::Span { view, start, end } => {
            let text = mounter
                .text(view)
                .ok_or_else(|| "the text is gone".to_owned())?;
            let local = text.span_point(start, end).unwrap_or_else(|| {
                let bounds = text.bounds();
                CGPoint::new(bounds.size.width / 2.0, bounds.size.height / 2.0)
            });
            to_window(&text, local)
        }
    };
    Ok((
        window,
        Point {
            x: point.x,
            y: point.y,
        },
    ))
}

fn to_window(view: &UIView, point: CGPoint) -> CGPoint {
    // SAFETY: `convertPoint:toView:` with a nil view is the window's space.
    unsafe { msg_send![view, convertPoint: point, toView: Option::<&UIView>::None] }
}

fn window_of(mounter: &Mounter) -> Result<objc2::rc::Retained<UIWindow>, String> {
    mounter
        .view_of(nlayer_core::ROOT_ID)
        .and_then(|view| view.window())
        .ok_or_else(|| "the window is not up".to_owned())
}

fn on_ui<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    // The socket thread hops to the main queue. A caller already on the main
    // thread (the host's unit tests) runs the step here; waiting on the main
    // queue from it would deadlock.
    if MainThreadMarker::new().is_some() {
        return Ok(work());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    DispatchQueue::main().exec_async(move || {
        let _ = tx.send(work());
    });
    rx.recv()
        .map_err(|_| "the simulator app stopped".to_owned())
}

fn ok(value: Value) -> String {
    json!({"ok": true, "value": value}).to_string()
}

fn fail(error: &str) -> String {
    json!({"ok": false, "error": error}).to_string()
}

#[cfg(feature = "host-tests")]
pub(crate) mod tests {
    use super::*;
    use nlayer_core::geometry::{Bounds, point, px, size};
    use nlayer_core::{MountBatch, MountItem, ViewKind};
    use objc2::MainThreadMarker;
    use objc2_core_foundation::{CGRect, CGSize};

    /// The host's globals are per-process; each test starts its own surface.
    fn surface(mtm: MainThreadMarker) -> crate::host::Session {
        let container = UIView::new(mtm);
        container.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 640.0)));
        let assets = std::env::temp_dir().join(format!(
            "nlayer-driver-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&assets);
        crate::host::start(mtm, &container, "", assets)
    }

    fn scroll_batch() -> MountBatch {
        MountBatch {
            items: vec![
                MountItem::Create {
                    id: 2,
                    kind: ViewKind::Scroll,
                },
                MountItem::Insert {
                    parent: nlayer_core::ROOT_ID,
                    child: 2,
                    before: None,
                },
                MountItem::Frame {
                    id: 2,
                    frame: Bounds::new(point(px(0.0), px(0.0)), size(px(300.0), px(200.0))),
                },
                MountItem::ScrollConfig {
                    id: 2,
                    horizontal: true,
                    taps: nlayer_core::KeyboardTaps::Never,
                    dismiss: nlayer_core::KeyboardDismiss::None,
                },
                MountItem::ScrollContent {
                    id: 2,
                    size: size(px(600.0), px(200.0)),
                },
                MountItem::TestId {
                    id: 2,
                    test_id: Some("list".into()),
                },
            ],
        }
    }

    fn value(line: &str) -> Value {
        let response: Value = serde_json::from_str(line).expect("a JSON response");
        assert_eq!(response["ok"], true, "{line}");
        response["value"].clone()
    }

    pub(crate) fn tests() -> Vec<(&'static str, fn())> {
        vec![
            (
                "driver::a_scroll_step_moves_the_scroll_views_offset",
                a_scroll_step_moves_the_scroll_views_offset as fn(),
            ),
            (
                "driver::a_step_that_cannot_run_reports_why",
                a_step_that_cannot_run_reports_why as fn(),
            ),
        ]
    }

    fn a_scroll_step_moves_the_scroll_views_offset() {
        let mtm =
            MainThreadMarker::new().expect("the iOS host test runner runs on the main thread");
        let session = surface(mtm);
        let mounter = crate::host::mounter().expect("mounter");
        mounter.borrow_mut().apply(scroll_batch());

        let tree = value(&step(r#"{"op":"tree"}"#));
        assert_eq!(tree["children"][0]["kind"], "scroll");
        assert_eq!(tree["children"][0]["horizontal"], true);

        value(&step(
            r#"{"op":"scroll","selector":"id:list","distance":40}"#,
        ));
        let scroll = mounter.borrow().scroll(2).expect("scroll view");
        assert_eq!(scroll.contentOffset().x, 40.0);
        assert_eq!(scroll.contentOffset().y, 0.0);
        assert_eq!(
            value(&step(r#"{"op":"query","selector":"id:list"}"#))["state"],
            "ready"
        );
        assert_eq!(
            value(&step(
                r#"{"op":"scroll","selector":"id:list","distance":-20}"#
            ))
            .as_null(),
            Some(())
        );
        assert_eq!(scroll.contentOffset().x, 20.0);
        drop(session);
    }

    fn a_step_that_cannot_run_reports_why() {
        let mtm =
            MainThreadMarker::new().expect("the iOS host test runner runs on the main thread");
        let session = surface(mtm);
        let unknown: Value =
            serde_json::from_str(&step(r#"{"op":"frobnicate"}"#)).expect("a JSON response");
        assert_eq!(unknown["ok"], false);
        assert_eq!(unknown["error"], "unknown test op `frobnicate`");
        let bad: Value = serde_json::from_str(&step("not json")).expect("a JSON response");
        assert_eq!(bad["ok"], false);
        // The empty bundle's boot error is what the runner would receive
        // instead of a ready app.
        assert_eq!(mounted().unwrap_err(), "no app.js next to the runner");
        drop(session);
    }
}
