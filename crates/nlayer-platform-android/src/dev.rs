//! Applies `nlayer run`'s updates to the running app, and drives a test
//! session on the same socket.
//!
//! Extracted from `nsh-platform-android/src/dev.rs` and cut to the v6
//! protocol (`nlayer_engine::protocol`): one socket through `adb reverse`,
//! a patch applied in place, a bundle reloaded in the same JS context when
//! its version differs, and a failed build shown as an overlay. There is no
//! log forwarding or status line. A test pin ([`start_test`]) sends
//! `Hello` `test: true`, loads the first bundle, and answers each step.
//! No overlay and no patch on that session.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use nlayer_engine::HotUpdate;
use nlayer_engine::protocol::{self, PROTOCOL_VERSION, WS_PATH};

use crate::host::Host;

/// Connects to the dev server at `address:port` and applies its updates.
///
/// `test` is the dev pin's flag: a test session shows no overlay and applies
/// no patch. An error is logged once and ends the thread; the app keeps
/// running.
pub(crate) fn start(host: Arc<Host>, address: String, port: u16, test: bool) {
    let _ = thread::Builder::new()
        .name("nlayer-dev".to_owned())
        .spawn(move || {
            if let Err(error) = run(&host, &address, port, test) {
                log::error!("nlayer: dev server: {error}");
            }
        });
}

fn run(host: &Arc<Host>, address: &str, port: u16, pin_test: bool) -> Result<(), String> {
    let url = format!("ws://{address}:{port}{WS_PATH}");
    let mut socket = tungstenite::connect(&url)
        .map_err(|error| format!("cannot connect to {url}: {error}"))?
        .0;
    let control = host
        .control
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    // The app started from `app.js`, the first bundle this connection gets.
    let mut running: Option<u64> = None;
    let mut first = true;
    let mut test = pin_test;
    loop {
        let message = socket.read().map_err(|error| error.to_string())?;
        let text = match message {
            tungstenite::Message::Text(text) => text,
            tungstenite::Message::Close(_) => return Ok(()),
            _ => continue,
        };
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|error| format!("invalid frame: {error}"))?;
        match value.get("type").and_then(|tag| tag.as_str()) {
            Some("hello") => {
                let hello: protocol::Hello = serde_json::from_value(value)
                    .map_err(|error| format!("invalid hello: {error}"))?;
                if hello.protocol != PROTOCOL_VERSION {
                    return Err(format!(
                        "the dev server speaks protocol {} and this host {PROTOCOL_VERSION}",
                        hello.protocol
                    ));
                }
                test |= hello.test;
            }
            Some("bundle") => {
                let bundle: protocol::Bundle = serde_json::from_value(value)
                    .map_err(|error| format!("invalid bundle: {error}"))?;
                let (code, _) = read_script(&mut socket, bundle.source_map)?;
                if test {
                    continue;
                }
                let Some(control) = &control else {
                    return Err("the engine is not running".to_owned());
                };
                if first {
                    // `app.js` is this build; the app is already running it.
                    running = Some(bundle.version);
                    first = false;
                } else if running != Some(bundle.version) {
                    control.replace_bundle(code);
                    running = Some(bundle.version);
                }
                overlay(host, None);
            }
            Some("patch") => {
                let patch: protocol::Patch = serde_json::from_value(value)
                    .map_err(|error| format!("invalid patch: {error}"))?;
                let (code, source_map) = read_script(&mut socket, patch.source_map)?;
                if test || running != Some(patch.base) {
                    continue;
                }
                let Some(control) = &control else {
                    return Err("the engine is not running".to_owned());
                };
                control.hot_update(HotUpdate {
                    name: format!("hot-{}.js", patch.version),
                    code,
                    source_map,
                });
                running = Some(patch.version);
                overlay(host, None);
            }
            Some("build_failed") => {
                let failed: protocol::BuildFailed = serde_json::from_value(value)
                    .map_err(|error| format!("invalid build_failed: {error}"))?;
                if !test {
                    overlay(host, Some(&failed.message));
                }
            }
            // `step` and `step_result` belong to the test channel (M6).
            _ => {}
        }
    }
}

/// The binary frames after a control frame: the script, then its source map
/// when `source_map` says one follows.
fn read_script<S: std::io::Read + std::io::Write>(
    socket: &mut tungstenite::WebSocket<S>,
    source_map: bool,
) -> Result<(String, Option<String>), String> {
    let code = read_binary(socket)?;
    let code = String::from_utf8(code).map_err(|_| "the script is not UTF-8".to_owned())?;
    let map = if source_map {
        let map = read_binary(socket)?;
        Some(String::from_utf8(map).map_err(|_| "the source map is not UTF-8".to_owned())?)
    } else {
        None
    };
    Ok((code, map))
}

fn read_binary<S: std::io::Read + std::io::Write>(
    socket: &mut tungstenite::WebSocket<S>,
) -> Result<Vec<u8>, String> {
    loop {
        match socket.read().map_err(|error| error.to_string())? {
            tungstenite::Message::Binary(bytes) => return Ok(bytes.to_vec()),
            tungstenite::Message::Close(_) => return Err("the server closed the socket".to_owned()),
            _ => {}
        }
    }
}

/// `Some`: shows the message over the app. `None`: takes it away.
fn overlay(host: &Host, message: Option<&str>) {
    crate::host::overlay_view(&host.view, message);
}

/// Connects a test pin to the dev server until the socket ends.
///
/// Sends [`protocol::Hello`] with `test: true`, loads the first bundle, then
/// answers each [`protocol::Step`]. No overlay and no patch. The runner is
/// ready once that bundle is loaded and a step can run.
pub(crate) fn start_test(host: Arc<Host>, address: String, port: u16) {
    let _ = thread::Builder::new()
        .name("nlayer-test".to_owned())
        .spawn(move || {
            if let Err(error) = test_session(&host, &address, port) {
                log::error!("nlayer test: {error}");
            }
        });
}

fn test_session(host: &Arc<Host>, address: &str, port: u16) -> Result<(), String> {
    let url = format!("ws://{address}:{port}{WS_PATH}");
    let mut socket = connect_test(&url)?;
    let hello =
        serde_json::to_string(&protocol::Hello::new(true)).map_err(|error| error.to_string())?;
    send(&mut socket, tungstenite::Message::text(hello))?;

    loop {
        let message = socket.read().map_err(|error| error.to_string())?;
        let text = match message {
            tungstenite::Message::Text(text) => text,
            tungstenite::Message::Ping(payload) => {
                send(&mut socket, tungstenite::Message::Pong(payload))?;
                continue;
            }
            tungstenite::Message::Close(_) => return Ok(()),
            _ => continue,
        };
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|error| format!("invalid frame: {error}"))?;
        match value.get("type").and_then(|tag| tag.as_str()) {
            Some("hello") => {
                let hello: protocol::Hello = serde_json::from_value(value)
                    .map_err(|error| format!("invalid hello: {error}"))?;
                if hello.protocol != PROTOCOL_VERSION {
                    return Err(format!(
                        "the dev server speaks protocol {} and this host {PROTOCOL_VERSION}",
                        hello.protocol
                    ));
                }
            }
            Some("bundle") => {
                let bundle: protocol::Bundle = serde_json::from_value(value)
                    .map_err(|error| format!("invalid bundle: {error}"))?;
                let (code, _) = read_script(&mut socket, bundle.source_map)?;
                load_test_bundle(host, code);
            }
            // The spec forbids patches mid-test. Consume the binary frames
            // so the socket stays aligned, and do not apply them.
            Some("patch") => {
                let patch: protocol::Patch = serde_json::from_value(value)
                    .map_err(|error| format!("invalid patch: {error}"))?;
                let _ = read_script(&mut socket, patch.source_map)?;
            }
            Some("build_failed") => {
                let failed: protocol::BuildFailed = serde_json::from_value(value)
                    .map_err(|error| format!("invalid build_failed: {error}"))?;
                return Err(failed.message);
            }
            Some("step") => {
                let step: protocol::Step = serde_json::from_value(value)
                    .map_err(|error| format!("invalid step: {error}"))?;
                let result = crate::driver::run_step(host, &step);
                let text = serde_json::to_string(&result).map_err(|error| error.to_string())?;
                send(&mut socket, tungstenite::Message::text(text))?;
            }
            _ => {}
        }
    }
}

fn load_test_bundle(host: &Host, code: String) {
    let control = host
        .control
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let boot = host
        .boot_script
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    if let Some(control) = control {
        if boot == code {
            return;
        }
        control.replace_bundle(code.clone());
        *host
            .boot_script
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = code;
        return;
    }
    if crate::host::boot(host, &code).is_some() {
        *host
            .boot_script
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = code;
    }
}

fn connect_test(
    url: &str,
) -> Result<tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>, String>
{
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last = format!("cannot connect to {url}");
    while Instant::now() < deadline {
        match tungstenite::connect(url) {
            Ok((socket, _)) => return Ok(socket),
            Err(error) => last = error.to_string(),
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!("test server: {last}"))
}

fn send<S: std::io::Read + std::io::Write>(
    socket: &mut tungstenite::WebSocket<S>,
    message: tungstenite::Message,
) -> Result<(), String> {
    socket
        .send(message)
        .map_err(|error| format!("test server: {error}"))?;
    socket
        .flush()
        .map_err(|error| format!("test server: {error}"))
}
