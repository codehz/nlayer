//! Applies `nlayer run`'s updates to the running app.
//!
//! Extracted from `nsh-platform-android/src/dev.rs` and cut to the v6
//! protocol (`nlayer_engine::protocol`): one socket through `adb reverse`,
//! a patch applied in place, a bundle reloaded in the same JS context when
//! its version differs, and a failed build shown as an overlay. There is no
//! log forwarding, status line or test step here (the steps are M6).

use std::sync::Arc;
use std::thread;

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
