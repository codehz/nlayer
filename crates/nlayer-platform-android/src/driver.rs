//! The socket `nlayer test --target android` drives.
//!
//! The test runner listens; this process connects and answers one JSON line
//! per step. A press is a touch the view hierarchy hit-tests. Typing goes
//! through the field's input connection, one character at a time. A scroll
//! sets the scroll view's own offset.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use jni::objects::{JString, JValue};
use jni::sys::{jboolean, jint};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::host::Host;
use crate::java::{java, with_env};
use crate::scene;
use crate::snapshot::scene_of;

const TAP: Duration = Duration::from_millis(60);
const SETTLE: Duration = Duration::from_millis(300);
const KEY: Duration = Duration::from_millis(40);

pub(crate) fn start(host: Arc<Host>, address: String) {
    thread::Builder::new()
        .name("nlayer-test".into())
        .spawn(move || serve(host, address))
        .ok();
}

fn serve(host: Arc<Host>, address: String) {
    let Some(stream) = connect(&address) else {
        log::error!("nlayer test: could not reach {address}");
        return;
    };
    let mut writer = stream;
    let mut reader = match writer.try_clone() {
        Ok(stream) => BufReader::new(stream),
        Err(error) => {
            log::error!("nlayer test: {error}");
            return;
        }
    };
    if let Err(error) = wait_until_mounted(&host) {
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
        let response = step(&host, line.trim());
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

fn wait_until_mounted(host: &Host) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let scene = scene_of(&snapshot(host)?)?;
        let children = scene
            .tree()
            .get("children")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        if children > 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("the app did not mount".into());
        }
        thread::sleep(Duration::from_millis(30));
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
        "longPress" => long_press(host, request.selector),
        "type" => type_text(host, request.selector, request.text),
        "scroll" => scroll(host, request.selector, request.distance),
        other => Err(format!("unknown test op `{other}`")),
    }
}

fn press(host: &Host, selector: String) -> Result<Value, String> {
    let scene = scene_of(&snapshot(host)?)?;
    let hit = scene.press(&selector)?;
    let (x, y) = scene.point(hit)?;
    tap(host, x, y, false);
    thread::sleep(TAP);
    tap(host, x, y, true);
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
    scroll_to(host, id as jint, x, y);
    Ok(Value::Null)
}

fn snapshot(host: &Host) -> Result<String, String> {
    let text = with_env(|env| {
        let value = unsafe {
            env.call_method_unchecked(
                host.view.as_obj(),
                java().view_snapshot,
                jni::signature::ReturnType::Object,
                &[],
            )?
        };
        let object = value.l()?;
        if object.is_null() {
            return Ok(String::new());
        }
        let string = unsafe { JString::from_raw(object.as_raw()) };
        Ok(env.get_string(&string)?.into())
    });
    if text.is_empty() {
        Err("the app did not answer".into())
    } else {
        Ok(text)
    }
}

fn tap(host: &Host, x: f32, y: f32, up: bool) {
    let _ = with_env(|env| {
        unsafe {
            env.call_method_unchecked(
                host.view.as_obj(),
                java().view_tap,
                jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
                &[
                    JValue::Float(x).as_jni(),
                    JValue::Float(y).as_jni(),
                    JValue::Bool(jboolean::from(up)).as_jni(),
                ],
            )?;
        }
        Ok(())
    });
}

fn type_char(host: &Host, id: jint, text: &str) {
    let _ = with_env(|env| {
        let text = env.new_string(text)?;
        unsafe {
            env.call_method_unchecked(
                host.view.as_obj(),
                java().view_type_char,
                jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
                &[JValue::Int(id).as_jni(), JValue::Object(&text).as_jni()],
            )?;
        }
        Ok(())
    });
}

fn scroll_to(host: &Host, id: jint, x: f32, y: f32) {
    let _ = with_env(|env| {
        unsafe {
            env.call_method_unchecked(
                host.view.as_obj(),
                java().view_scroll_to,
                jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
                &[
                    JValue::Int(id).as_jni(),
                    JValue::Float(x).as_jni(),
                    JValue::Float(y).as_jni(),
                ],
            )?;
        }
        Ok(())
    });
}

fn ok(value: Value) -> String {
    json!({"ok": true, "value": value}).to_string()
}

fn fail(error: &str) -> String {
    json!({"ok": false, "error": error}).to_string()
}
