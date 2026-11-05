//! Steps `nlayer test --target android` runs on the shared dev socket.
//!
//! [`crate::dev::start_test`] connects to `ws://host:port/ws`. This module
//! turns each [`Step`] into the JSON line the view steps already match and
//! parses the answer. A press is a touch the view hierarchy hit-tests.
//! Typing goes through the field's input connection, one character at a
//! time. A scroll sets the scroll view's own offset.
//!
//! The runner is ready once the first bundle has loaded and a step can run.
//! There is no separate ready line: the CLI waits on the first step result.

use std::sync::{Mutex, atomic::Ordering};
use std::thread;
use std::time::{Duration, Instant};

use jni::sys::jint;
use serde::Deserialize;
use serde_json::{Value, json};

use nlayer_engine::protocol::{Step, StepOp, StepResult};

use crate::host::Host;
use crate::java::with_env;
use crate::scene;
use crate::snapshot::scene_of;

const TAP: Duration = Duration::from_millis(60);
const SETTLE: Duration = Duration::from_millis(300);
const KEY: Duration = Duration::from_millis(40);

/// Runs `step` through the view steps and turns the line answer into a
/// [`StepResult`].
pub(crate) fn run_step(host: &Host, step: &Step) -> StepResult {
    let line = json!({
        "op": op_name(step.op),
        "selector": step.selector,
        "text": step.text,
        "distance": step.distance,
        "x": step.x,
        "y": step.y,
        "ms": step.ms,
    })
    .to_string();
    step_result(step.id, &self::step(host, &line))
}

/// `waitFor` is the protocol name; the steps match `query`.
fn op_name(op: StepOp) -> String {
    if op == StepOp::WaitFor {
        return "query".to_owned();
    }
    serde_json::to_value(op)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn step_result(id: u64, line: &str) -> StepResult {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => return StepResult::error(id, error.to_string()),
    };
    let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if !ok {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("step failed");
        return StepResult::error(id, error);
    }
    match value.get("value") {
        Some(value) if !value.is_null() => StepResult::done(id, value.clone()),
        _ => StepResult::ok(id),
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
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    ms: f64,
}

fn step(host: &Host, line: &str) -> String {
    match dispatch(host, line) {
        Ok(value) => ok(value),
        Err(error) => fail(&error),
    }
}

fn dispatch(host: &Host, line: &str) -> Result<Value, String> {
    let request: Request = serde_json::from_str(line).map_err(|error| error.to_string())?;
    match request.op.as_str() {
        "texts" => Ok(json!(scene_of(&snapshot(host)?)?.texts())),
        "inputs" => Ok(json!(scene_of(&snapshot(host)?)?.inputs())),
        "tree" => Ok(scene_of(&snapshot(host)?)?.tree()),
        "query" => {
            let scene = scene_of(&snapshot(host)?)?;
            Ok(match scene.query(&request.selector) {
                scene::Query::Ready => json!({"state": "ready"}),
                scene::Query::Wait => json!({"state": "wait"}),
                scene::Query::Fail(error) => json!({"state": "fail", "error": error}),
            })
        }
        "press" => press(host, request.selector),
        "hover" => hover(host, request.selector),
        "selectionOwner" => {
            let scene = scene_of(&snapshot(host)?)?;
            let owner = scene
                .view_of(&request.selector)
                .ok()
                .and_then(|id| scene.selection_owner(id));
            Ok(json!(owner))
        }
        "openedUrls" => {
            let text = snapshot(host)?;
            let value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
            Ok(value
                .get("openedUrls")
                .cloned()
                .unwrap_or_else(|| json!([])))
        }
        "longPress" => long_press(host, request.selector),
        "type" => type_text(host, request.selector, request.text),
        "scroll" => scroll(host, request.selector, request.distance),
        "advance" => advance(request.ms),
        "drag" => drag(host, request.selector, request.x, request.y),
        "dragAndHold" => drag_and_hold(host, request.selector, request.x, request.y),
        "drop" => drop_touch(host),
        "back" => back(host),
        other => Err(format!("unknown test op `{other}`")),
    }
}

fn press(host: &Host, selector: String) -> Result<Value, String> {
    let scene = scene_of(&snapshot(host)?)?;
    let hit = scene.press(&selector)?;
    let (x, y) = scene.point(hit)?;
    tap(host, x, y, false);
    thread::sleep(TAP);
    // `onPress` runs on the lift. Sample after the down so an active-style
    // batch is not the one this waits for.
    let before = host.commits.load(Ordering::SeqCst);
    tap(host, x, y, true);
    settle(host, before);
    Ok(Value::Null)
}

fn hover(host: &Host, selector: String) -> Result<Value, String> {
    let scene = scene_of(&snapshot(host)?)?;
    let (x, y) = scene.point_for(&selector)?;
    let _ = with_env(|env| {
        host.view.hover(env, x, y)?;
        Ok(())
    });
    thread::sleep(SETTLE);
    Ok(Value::Null)
}

fn long_press(host: &Host, selector: String) -> Result<Value, String> {
    let scene = scene_of(&snapshot(host)?)?;
    let hit = scene.press(&selector)?;
    let (x, y) = scene.point(hit)?;
    tap(host, x, y, false);
    thread::sleep(std::time::Duration::from_millis(600));
    tap(host, x, y, true);
    thread::sleep(SETTLE);
    Ok(Value::Null)
}

fn type_text(host: &Host, selector: String, text: String) -> Result<Value, String> {
    let scene = scene_of(&snapshot(host)?)?;
    let id = scene.type_target(&selector)?;
    for ch in text.chars() {
        type_char(host, id as jint, &ch.to_string());
        thread::sleep(KEY);
    }
    thread::sleep(SETTLE);
    Ok(Value::Null)
}

fn scroll(host: &Host, selector: String, distance: f32) -> Result<Value, String> {
    let scene = scene_of(&snapshot(host)?)?;
    let id = scene.scroll_target(&selector)?;
    let node = scene
        .node(id)
        .and_then(|node| node.scroll)
        .ok_or_else(|| format!("`{selector}` is not in a scroll view"))?;
    let frame = scene
        .node(id)
        .map(|node| node.frame)
        .ok_or_else(|| format!("`{selector}` is not in a scroll view"))?;
    let (x, y) = if node.horizontal {
        (
            scene::next_offset(node.x, distance, node.content_w, frame.w),
            node.y,
        )
    } else {
        (
            node.x,
            scene::next_offset(node.y, distance, node.content_h, frame.h),
        )
    };
    let before = host.commits.load(Ordering::SeqCst);
    scroll_to(host, id as jint, x, y);
    // A scroll moves the view now; the list window and sticky positions are
    // a new mount batch. Read the tree only after it has landed.
    wait_for_commit(host, before, Duration::from_millis(500));
    Ok(Value::Null)
}

fn advance(ms: f64) -> Result<Value, String> {
    // The host's Choreographer drives animation frames with the platform's
    // clock; ticking here from the socket thread would race the UI thread.
    // Wait real time, letting the view's frame callbacks run.
    let deadline = std::time::Instant::now() + Duration::from_secs_f64(ms.max(0.0) / 1000.0);
    while std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(16));
    }
    Ok(Value::Null)
}

/// Where a held finger is, for `drop`.
static DRAG_STATE: Mutex<Option<(f32, f32)>> = Mutex::new(None);

fn drag(host: &Host, selector: String, x: f32, y: f32) -> Result<Value, String> {
    let scene = scene_of(&snapshot(host)?)?;
    let (cx, cy) = scene.point_for(&selector)?;
    tap(host, cx, cy, false);
    thread::sleep(Duration::from_millis(20));
    let steps = 5;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        move_to(host, cx + x * t, cy + y * t);
        thread::sleep(Duration::from_millis(10));
    }
    // Moves commit as the finger travels. The lift's `onPanEnd` is the
    // batch after that.
    let before = host.commits.load(Ordering::SeqCst);
    tap(host, cx + x, cy + y, true);
    settle(host, before);
    Ok(Value::Null)
}

fn drag_and_hold(host: &Host, selector: String, x: f32, y: f32) -> Result<Value, String> {
    let scene = scene_of(&snapshot(host)?)?;
    let (cx, cy) = scene.point_for(&selector)?;
    tap(host, cx, cy, false);
    thread::sleep(Duration::from_millis(20));
    let steps = 5;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        move_to(host, cx + x * t, cy + y * t);
        thread::sleep(Duration::from_millis(10));
    }
    *DRAG_STATE.lock().unwrap_or_else(|err| err.into_inner()) = Some((cx + x, cy + y));
    Ok(Value::Null)
}

fn drop_touch(host: &Host) -> Result<Value, String> {
    let (x, y) = DRAG_STATE
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .take()
        .ok_or_else(|| "no drag in progress".to_string())?;
    tap(host, x, y, true);
    thread::sleep(SETTLE);
    Ok(Value::Null)
}

fn back(host: &Host) -> Result<Value, String> {
    let before = host.commits.load(Ordering::SeqCst);
    let _ = with_env(|env| {
        host.view.back(env)?;
        Ok(())
    });
    settle(host, before);
    Ok(Value::Null)
}

fn move_to(host: &Host, x: f32, y: f32) {
    let _ = with_env(|env| {
        host.view.move_view(env, x, y)?;
        Ok(())
    });
}

/// Waits until the engine has posted a mount batch after `before`, then
/// joins the UI thread so the batch is applied. `false` on timeout.
pub(crate) fn wait_for_commit(host: &Host, before: u64, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if host.commits.load(Ordering::SeqCst) > before {
            let _ = snapshot(host);
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    false
}

/// Waits for the mount batch `before` did not include, then the usual
/// settle. A step that changes nothing runs the wait out.
fn settle(host: &Host, before: u64) {
    wait_for_commit(host, before, Duration::from_secs(1));
    thread::sleep(SETTLE);
}

pub(crate) fn snapshot(host: &Host) -> Result<String, String> {
    let text = with_env(|env| {
        let string = host.view.snapshot(env)?;
        if string.is_null() {
            return Ok(String::new());
        }
        string.try_to_string(env)
    });
    if text.is_empty() {
        Err("the app did not answer".into())
    } else {
        Ok(text)
    }
}

fn tap(host: &Host, x: f32, y: f32, up: bool) {
    let _ = with_env(|env| {
        host.view.tap(env, x, y, up)?;
        Ok(())
    });
}

fn type_char(host: &Host, id: jint, text: &str) {
    let _ = with_env(|env| {
        let text = env.new_string(text)?;
        host.view.type_char(env, id, &text)?;
        Ok(())
    });
}

fn scroll_to(host: &Host, id: jint, x: f32, y: f32) {
    let _ = with_env(|env| {
        host.view.scroll_to(env, id, x, y)?;
        Ok(())
    });
}

fn ok(value: Value) -> String {
    json!({"ok": true, "value": value}).to_string()
}

fn fail(error: &str) -> String {
    json!({"ok": false, "error": error}).to_string()
}
