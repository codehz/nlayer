//! Development client for `nlayer run --target ios` and `nlayer test`.
//!
//! Connects to the dev server at `ws://host:port/ws`, sends [`Hello`], and
//! applies [`Patch`] / [`Bundle`] frames. A [`BuildFailed`] frame covers the
//! app with the host overlay and leaves the running script in place. A test
//! pin ([`start_test`]) uses the same socket: [`Hello`] `test` is true, the
//! first [`Bundle`] loads the script, and each [`Step`] is answered with a
//! [`StepResult`]. No overlay, and no patch, on that session.
//!
//! The client is cut from `nsh-dev`'s socket client and `nsh-platform-ios`'s
//! dev session: no log forwarding, no reconnect loop.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use nlayer_engine::protocol::{self, BuildFailed, Bundle, Hello, PROTOCOL_VERSION, Patch, Step};
use nlayer_engine::{EngineControl, HotUpdate};
use tungstenite::{Message, WebSocket};

use crate::pin::DevPin;

/// Shared with [`note_failure`]: the engine reports a patch that could not
/// be applied on its own thread, after this client has already read on.
struct Live {
    control: Option<EngineControl>,
    /// Build the engine is running. `None` until the first bundle is identified.
    running: Option<u64>,
    /// Patch version sent to the engine, waiting for its bundle frame.
    pending: Option<u64>,
    /// Newest full script from the socket, with its version.
    latest: Option<(u64, String)>,
    /// A patch failed and the bundle of that version has not been applied yet.
    failed: bool,
    saw_bundle: bool,
    /// `app.js` the process already evaluated. Empty when the engine has not started.
    boot_script: String,
}

static LIVE: Mutex<Option<Arc<Mutex<Live>>>> = Mutex::new(None);

/// A patch could not be applied. Hides nothing by itself: the host calls
/// this from the main thread after showing the reload line. When the full
/// script of that version is already here, it is loaded in place; otherwise
/// the next [`Bundle`] is.
pub(crate) fn note_failure() {
    let live = LIVE.lock().expect("dev client").clone();
    let Some(live) = live else { return };
    let replace = {
        let mut live = live.lock().expect("dev client");
        live.failed = true;
        match live.latest.clone() {
            Some((version, code))
                if live.running == Some(version) || live.pending == Some(version) =>
            {
                live.running = Some(version);
                live.pending = None;
                live.failed = false;
                live.control.clone().map(|control| (control, code))
            }
            _ => None,
        }
    };
    if let Some((control, code)) = replace {
        control.replace_bundle(code);
    }
}

/// Connects until the socket ends. `test` sessions are not connected.
/// `boot_script` is the source already evaluated, when the engine started.
pub(crate) fn start(pin: DevPin, boot_script: String, control: Option<EngineControl>) {
    if pin.test || pin.port == 0 || pin.host.is_empty() {
        return;
    }
    let _ = thread::Builder::new()
        .name("nlayer-dev".to_owned())
        .spawn(move || {
            let live = Arc::new(Mutex::new(Live {
                control,
                running: None,
                pending: None,
                latest: None,
                failed: false,
                saw_bundle: false,
                boot_script,
            }));
            *LIVE.lock().expect("dev client") = Some(Arc::clone(&live));
            if let Err(error) = session(&pin, &live) {
                eprintln!("nlayer: {error}");
            }
            let mut slot = LIVE.lock().expect("dev client");
            if slot
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &live))
            {
                *slot = None;
            }
        });
}

fn session(pin: &DevPin, live: &Arc<Mutex<Live>>) -> Result<(), String> {
    let address: SocketAddr = format!("{}:{}", pin.host, pin.port)
        .parse()
        .map_err(|_| format!("dev server address {}:{}", pin.host, pin.port))?;
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|error| format!("dev server: {error}"))?;
    let _ = stream.set_nodelay(true);
    let url = format!("ws://{address}{}", protocol::WS_PATH);
    let (mut socket, _) =
        tungstenite::client(url, stream).map_err(|error| format!("dev server: {error}"))?;
    send(
        &mut socket,
        Message::text(
            serde_json::to_string(&Hello::new(false)).map_err(|error| error.to_string())?,
        ),
    )?;

    let mut pending = Pending::None;
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                if !matches!(pending, Pending::None) {
                    return Err("dev server: a script frame was missing".to_owned());
                }
                if on_text(&text, &mut pending)? {
                    return Ok(());
                }
            }
            Ok(Message::Binary(bytes)) => on_binary(&bytes, &mut pending, live)?,
            Ok(Message::Ping(payload)) => send(&mut socket, Message::Pong(payload))?,
            Ok(Message::Close(_)) => return Ok(()),
            Ok(Message::Pong(_) | Message::Frame(_)) => {}
            Err(error) => return Err(format!("dev server: {error}")),
        }
    }
}

fn send<S: Read + Write>(socket: &mut WebSocket<S>, message: Message) -> Result<(), String> {
    socket
        .send(message)
        .map_err(|error| format!("dev server: {error}"))?;
    socket
        .flush()
        .map_err(|error| format!("dev server: {error}"))
}

/// `true`: the hello marked a test session. Stop without applying anything.
fn on_text(text: &str, pending: &mut Pending) -> Result<bool, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("dev server: {error}"))?;
    let kind = value
        .get("type")
        .and_then(|kind| kind.as_str())
        .unwrap_or("");
    match kind {
        "hello" => {
            let hello: Hello =
                serde_json::from_value(value).map_err(|error| format!("dev server: {error}"))?;
            if hello.protocol != PROTOCOL_VERSION {
                return Err(format!(
                    "dev server protocol {} is not {PROTOCOL_VERSION}",
                    hello.protocol
                ));
            }
            Ok(hello.test)
        }
        "patch" => {
            let patch: Patch =
                serde_json::from_value(value).map_err(|error| format!("dev server: {error}"))?;
            *pending = Pending::Script {
                kind: Frame::Patch {
                    version: patch.version,
                    base: patch.base,
                },
                source_map: patch.source_map,
            };
            Ok(false)
        }
        "bundle" => {
            let bundle: Bundle =
                serde_json::from_value(value).map_err(|error| format!("dev server: {error}"))?;
            *pending = Pending::Script {
                kind: Frame::Bundle {
                    version: bundle.version,
                },
                source_map: bundle.source_map,
            };
            Ok(false)
        }
        "build_failed" => {
            let failed: BuildFailed =
                serde_json::from_value(value).map_err(|error| format!("dev server: {error}"))?;
            show(failed.message);
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn on_binary(bytes: &[u8], pending: &mut Pending, live: &Arc<Mutex<Live>>) -> Result<(), String> {
    let text =
        String::from_utf8(bytes.to_vec()).map_err(|_| "dev server sent non-UTF-8".to_owned())?;
    match std::mem::replace(pending, Pending::None) {
        Pending::Script {
            kind,
            source_map: true,
        } => {
            *pending = Pending::Map { kind, code: text };
            Ok(())
        }
        Pending::Script {
            kind,
            source_map: false,
        } => {
            apply(kind, text, None, live);
            Ok(())
        }
        Pending::Map { kind, code } => {
            apply(kind, code, Some(text), live);
            Ok(())
        }
        Pending::None => Err("dev server: unexpected binary frame".to_owned()),
    }
}

fn apply(kind: Frame, code: String, map: Option<String>, live: &Arc<Mutex<Live>>) {
    match kind {
        Frame::Patch { version, base } => on_patch(live, version, base, code, map),
        Frame::Bundle { version } => on_bundle(live, version, code),
    }
}

fn on_patch(
    live: &Arc<Mutex<Live>>,
    version: u64,
    base: u64,
    code: String,
    source_map: Option<String>,
) {
    let control = {
        let mut live = live.lock().expect("dev client");
        if live.running != Some(base) {
            return;
        }
        live.pending = Some(version);
        live.control.clone()
    };
    let Some(control) = control else { return };
    control.hot_update(HotUpdate {
        name: format!("hot-{version}.js"),
        code,
        source_map,
    });
    hide();
}

fn on_bundle(live: &Arc<Mutex<Live>>, version: u64, code: String) {
    enum Step {
        Done,
        Replace(EngineControl, String),
        Boot(String),
    }
    let step = {
        let mut live = live.lock().expect("dev client");
        live.latest = Some((version, code.clone()));
        let failed = live.failed;
        if failed {
            live.failed = false;
            live.pending = None;
        }
        let pending = live.pending == Some(version);
        if pending {
            live.pending = None;
        }
        let first = !live.saw_bundle;
        live.saw_bundle = true;
        if !failed && pending {
            live.running = Some(version);
            Step::Done
        } else if !failed && first && !live.boot_script.is_empty() && live.boot_script == code {
            // Already evaluated from `app.js`. The socket copy is the same script.
            live.running = Some(version);
            Step::Done
        } else if !failed && live.running == Some(version) {
            Step::Done
        } else if let Some(control) = live.control.clone() {
            live.running = Some(version);
            Step::Replace(control, code)
        } else {
            Step::Boot(code)
        }
    };
    match step {
        Step::Done => hide(),
        Step::Replace(control, code) => {
            control.replace_bundle(code);
            hide();
        }
        Step::Boot(code) => {
            let started = on_main(move || crate::host::boot_engine(&code));
            if let Some(control) = started.flatten() {
                let mut live = live.lock().expect("dev client");
                live.control = Some(control);
                live.running = Some(version);
                hide();
            }
        }
    }
}

fn on_main<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    dispatch2::DispatchQueue::main().exec_async(move || {
        let _ = tx.send(work());
    });
    rx.recv().ok()
}

fn show(message: String) {
    dispatch2::DispatchQueue::main().exec_async(move || crate::host::show_dev_overlay(&message));
}

fn hide() {
    dispatch2::DispatchQueue::main().exec_async(crate::host::hide_dev_overlay);
}

enum Pending {
    None,
    Script { kind: Frame, source_map: bool },
    Map { kind: Frame, code: String },
}

enum Frame {
    Patch { version: u64, base: u64 },
    Bundle { version: u64 },
}

/// Connects a test pin to the dev server until the socket ends.
///
/// Sends [`Hello`] with `test: true`, loads the first [`Bundle`], then
/// answers each [`Step`]. No overlay and no patch. The runner is ready once
/// that bundle is loaded and a step can run; there is no ready line.
pub(crate) fn start_test(
    pin: crate::pin::Pin,
    boot_script: String,
    control: Option<EngineControl>,
) {
    if pin.port == 0 || pin.host.is_empty() {
        return;
    }
    let _ = thread::Builder::new()
        .name("nlayer-test".to_owned())
        .spawn(move || {
            if let Err(error) = test_session(&pin, boot_script, control) {
                eprintln!("nlayer test: {error}");
            }
        });
}

struct TestLive {
    control: Option<EngineControl>,
    /// `app.js` already evaluated. Empty when the engine has not started.
    boot_script: String,
}

fn test_session(
    pin: &crate::pin::Pin,
    boot_script: String,
    control: Option<EngineControl>,
) -> Result<(), String> {
    let mut socket = connect_test(&pin.host, pin.port, pin.test)?;
    send(
        &mut socket,
        Message::text(serde_json::to_string(&Hello::new(true)).map_err(|error| error.to_string())?),
    )?;

    let mut live = TestLive {
        control,
        boot_script,
    };
    let mut pending = TestPending::None;
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                if !matches!(pending, TestPending::None) {
                    return Err("test server: a script frame was missing".to_owned());
                }
                on_test_text(&text, &mut pending, &mut socket)?;
            }
            Ok(Message::Binary(bytes)) => on_test_binary(&bytes, &mut pending, &mut live)?,
            Ok(Message::Ping(payload)) => send(&mut socket, Message::Pong(payload))?,
            Ok(Message::Close(_)) => return Ok(()),
            Ok(Message::Pong(_) | Message::Frame(_)) => {}
            Err(error) => return Err(format!("test server: {error}")),
        }
    }
}

fn connect_test(host: &str, port: u16, test: bool) -> Result<WebSocket<TcpStream>, String> {
    let address: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|_| format!("test server address {host}:{port} (test={test})"))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last = format!("could not reach {host}:{port} (test={test})");
    while Instant::now() < deadline {
        match TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                let url = format!("ws://{address}{}", protocol::WS_PATH);
                match tungstenite::client(url, stream) {
                    Ok((socket, _)) => return Ok(socket),
                    Err(error) => last = error.to_string(),
                }
            }
            Err(error) => last = error.to_string(),
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!("test server: {last}"))
}

fn on_test_text<S: Read + Write>(
    text: &str,
    pending: &mut TestPending,
    socket: &mut WebSocket<S>,
) -> Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("test server: {error}"))?;
    let kind = value
        .get("type")
        .and_then(|kind| kind.as_str())
        .unwrap_or("");
    match kind {
        "hello" => {
            let hello: Hello =
                serde_json::from_value(value).map_err(|error| format!("test server: {error}"))?;
            if hello.protocol != PROTOCOL_VERSION {
                return Err(format!(
                    "dev server protocol {} is not {PROTOCOL_VERSION}",
                    hello.protocol
                ));
            }
            Ok(())
        }
        "bundle" => {
            let bundle: Bundle =
                serde_json::from_value(value).map_err(|error| format!("test server: {error}"))?;
            *pending = TestPending::Script {
                discard: false,
                source_map: bundle.source_map,
            };
            Ok(())
        }
        // The spec forbids patches mid-test. Consume the binary frames so
        // the socket stays aligned, and do not apply them.
        "patch" => {
            let patch: Patch =
                serde_json::from_value(value).map_err(|error| format!("test server: {error}"))?;
            *pending = TestPending::Script {
                discard: true,
                source_map: patch.source_map,
            };
            Ok(())
        }
        "build_failed" => {
            let failed: BuildFailed =
                serde_json::from_value(value).map_err(|error| format!("test server: {error}"))?;
            Err(failed.message)
        }
        "step" => {
            let step: Step =
                serde_json::from_value(value).map_err(|error| format!("test server: {error}"))?;
            let result = crate::driver::run_step(&step);
            let text = serde_json::to_string(&result).map_err(|error| error.to_string())?;
            send(socket, Message::text(text))
        }
        _ => Ok(()),
    }
}

fn on_test_binary(
    bytes: &[u8],
    pending: &mut TestPending,
    live: &mut TestLive,
) -> Result<(), String> {
    let text =
        String::from_utf8(bytes.to_vec()).map_err(|_| "test server sent non-UTF-8".to_owned())?;
    match std::mem::replace(pending, TestPending::None) {
        TestPending::Script {
            discard,
            source_map: true,
        } => {
            *pending = TestPending::Map {
                discard,
                code: text,
            };
            Ok(())
        }
        TestPending::Script {
            discard: true,
            source_map: false,
        }
        | TestPending::Map { discard: true, .. } => Ok(()),
        TestPending::Script {
            discard: false,
            source_map: false,
        } => {
            apply_test_bundle(live, text);
            Ok(())
        }
        TestPending::Map {
            discard: false,
            code,
        } => {
            apply_test_bundle(live, code);
            Ok(())
        }
        TestPending::None => Err("test server: unexpected binary frame".to_owned()),
    }
}

fn apply_test_bundle(live: &mut TestLive, code: String) {
    if let Some(control) = live.control.clone() {
        if !live.boot_script.is_empty() && live.boot_script == code {
            return;
        }
        control.replace_bundle(code.clone());
        live.boot_script = code;
        return;
    }
    let started = {
        let code = code.clone();
        on_main(move || crate::host::boot_engine_quiet(&code))
    };
    if let Some(control) = started.flatten() {
        live.control = Some(control);
        live.boot_script = code;
    }
}

enum TestPending {
    None,
    Script { discard: bool, source_map: bool },
    Map { discard: bool, code: String },
}
