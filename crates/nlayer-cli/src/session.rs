//! One `nlayer test --target` session.
//!
//! `start` bundles the entry once and listens on `127.0.0.1:0`. The first
//! client on [`WS_PATH`] is sent [`Hello`] (`test`), then [`Bundle`] version
//! 1 and its script. [`TestSession::step`] sends a [`Step`] and waits for
//! the matching [`StepResult`].
//!
//! The wait is `nsh-dev`'s `DevServer::step`: an id, the host's result, or
//! 30 seconds. A closed socket is "the app stopped". A test session does
//! not send [`Patch`].
//!
//! `select` puts `start,end` (UTF-16) in [`Step::text`]. `compose` puts the
//! composition in `text` and `1.0` or `0.0` in `x` (composing or not).
//! `query` is [`StepOp::WaitFor`].

use std::{
    io::ErrorKind,
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
        mpsc::{Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use nlayer_bundler::bundle_hot;
use nlayer_engine::protocol::{self, StepOp, StepResult, WS_PATH};
use serde_json::Value;
use tungstenite::{Message, WebSocket};

/// How long one step may take, including the wait for the app to connect.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// A closed socket: the app is gone.
const STOPPED: &str = "the app stopped";

/// The app never opened the socket.
const NOT_CONNECTED: &str = "the app did not connect";

type Socket = WebSocket<TcpStream>;

/// A test session serving one app. Dropping it closes the socket and the
/// accept thread.
pub(crate) struct TestSession {
    port: u16,
    stop: Arc<AtomicBool>,
    incoming: Receiver<Result<Socket, String>>,
    socket: Option<Socket>,
    thread: Option<JoinHandle<()>>,
    next_id: u64,
}

impl TestSession {
    /// Bundles `entry` for a test session and listens for the host.
    pub(crate) fn start(entry: &Path, fixture_origin: bool) -> Result<Self, String> {
        let bundled = bundle_hot(entry, true).map_err(|error| error.colored)?;
        let (script, source_map) = bundled.script();
        let script = if fixture_origin {
            nlayer_testing::with_fixture_origin(&script, entry.parent().unwrap_or(entry))
        } else {
            script
        };
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, incoming) = mpsc::channel();
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("nlayer-test".to_owned())
            .spawn(move || serve(listener, script, source_map, thread_stop, tx))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            port,
            stop,
            incoming,
            socket: None,
            thread: Some(thread),
            next_id: 1,
        })
    }

    /// The port the host connects to (`ws://127.0.0.1:<port>/ws`).
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// Runs `op` and returns what the host read, or why it failed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step(
        &mut self,
        op: StepOp,
        selector: &str,
        text: &str,
        distance: f32,
        x: f32,
        y: f32,
        ms: f64,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + STEP_TIMEOUT;
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let mut frame = protocol::Step::new(id, op);
        frame.selector = selector.to_owned();
        frame.text = text.to_owned();
        frame.distance = distance;
        frame.x = x;
        frame.y = y;
        frame.ms = ms;
        let text = serde_json::to_string(&frame).map_err(|error| error.to_string())?;
        let socket = self.connected(deadline)?;
        send_frame(socket, Message::text(text), deadline, None)?;
        read_result(socket, id, deadline)
    }

    /// The host's socket, waiting out the rest of `deadline` for it to connect.
    fn connected(&mut self, deadline: Instant) -> Result<&mut Socket, String> {
        if self.socket.is_none() {
            let wait = deadline.saturating_duration_since(Instant::now());
            if wait.is_zero() {
                return Err(NOT_CONNECTED.to_owned());
            }
            match self.incoming.recv_timeout(wait) {
                Ok(Ok(socket)) => self.socket = Some(socket),
                Ok(Err(error)) => return Err(error),
                Err(RecvTimeoutError::Timeout) => return Err(NOT_CONNECTED.to_owned()),
                Err(RecvTimeoutError::Disconnected) => return Err(STOPPED.to_owned()),
            }
        }
        self.socket.as_mut().ok_or_else(|| STOPPED.to_owned())
    }
}

impl Drop for TestSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(socket) = self.socket.take() {
            let _ = close_socket(socket);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The wire name `call` already uses. `query` is [`StepOp::WaitFor`].
pub(crate) fn step_op(name: &str) -> Result<StepOp, String> {
    let wire = if name == "query" { "waitFor" } else { name };
    serde_json::from_value(Value::String(wire.to_owned()))
        .map_err(|_| format!("unknown test op `{name}`"))
}

fn serve(
    listener: TcpListener,
    script: String,
    source_map: Option<String>,
    stop: Arc<AtomicBool>,
    tx: Sender<Result<Socket, String>>,
) {
    let _ = listener.set_nonblocking(true);
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(error) => {
                let _ = tx.send(Err(error.to_string()));
                return;
            }
        };
        let _ = stream.set_nodelay(true);
        // accept() on macOS keeps the listener's O_NONBLOCK. The handshake
        // reads the request; a timeout is how a silent peer is dropped.
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut socket = match handshake(stream) {
            Ok(socket) => socket,
            Err(error) if rejected(&error) => continue,
            Err(error) => {
                let _ = tx.send(Err(error));
                return;
            }
        };
        if let Err(error) = arm(&mut socket) {
            let _ = tx.send(Err(error));
            return;
        }
        if let Err(error) = send_open(&mut socket, &script, source_map.as_deref(), &stop) {
            let _ = tx.send(Err(error));
            return;
        }
        let _ = tx.send(Ok(socket));
        return;
    }
}

fn handshake(stream: TcpStream) -> Result<Socket, String> {
    tungstenite::accept_hdr(stream, require_ws).map_err(|error| error.to_string())
}

fn require_ws(
    request: &tungstenite::handshake::server::Request,
    response: tungstenite::handshake::server::Response,
) -> Result<tungstenite::handshake::server::Response, tungstenite::handshake::server::ErrorResponse>
{
    if request.uri().path() == WS_PATH {
        Ok(response)
    } else {
        Err(tungstenite::http::Response::builder()
            .status(tungstenite::http::StatusCode::NOT_FOUND)
            .body(Some("not found".to_owned()))
            .unwrap_or_else(|_| tungstenite::http::Response::new(Some("not found".to_owned()))))
    }
}

fn rejected(error: &impl std::fmt::Display) -> bool {
    error.to_string().contains("404")
}

/// Reads are non-blocking after this: tungstenite treats a read timeout as
/// fatal, and [`ErrorKind::WouldBlock`] is the error it expects when the
/// peer has not written a frame.
fn arm(socket: &mut Socket) -> Result<(), String> {
    let stream = socket.get_mut();
    stream
        .set_read_timeout(None)
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(None)
        .map_err(|error| error.to_string())?;
    stream
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn send_open(
    socket: &mut Socket,
    script: &str,
    source_map: Option<&str>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let deadline = Instant::now() + STEP_TIMEOUT;
    send_json(socket, &protocol::Hello::new(true), deadline, Some(stop))?;
    send_json(
        socket,
        &protocol::Bundle::new(1, source_map.is_some()),
        deadline,
        Some(stop),
    )?;
    send_frame(
        socket,
        Message::binary(script.as_bytes().to_vec()),
        deadline,
        Some(stop),
    )?;
    if let Some(map) = source_map {
        send_frame(
            socket,
            Message::binary(map.as_bytes().to_vec()),
            deadline,
            Some(stop),
        )?;
    }
    Ok(())
}

fn send_json(
    socket: &mut Socket,
    value: &impl serde::Serialize,
    deadline: Instant,
    stop: Option<&AtomicBool>,
) -> Result<(), String> {
    let text = serde_json::to_string(value).map_err(|error| error.to_string())?;
    send_frame(socket, Message::text(text), deadline, stop)
}

fn send_frame(
    socket: &mut Socket,
    message: Message,
    deadline: Instant,
    stop: Option<&AtomicBool>,
) -> Result<(), String> {
    let mut pending = Some(message);
    loop {
        if stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
            return Err(STOPPED.to_owned());
        }
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        if let Some(message) = pending.take() {
            match socket.send(message) {
                Ok(()) => {}
                Err(tungstenite::Error::WriteBufferFull(message)) => pending = Some(message),
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => return Err(explain(error)),
            }
        }
        match socket.flush() {
            Ok(()) if pending.is_none() => return Ok(()),
            Ok(()) => {}
            Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(explain(error)),
        }
    }
}

fn read_result(socket: &mut Socket, id: u64, deadline: Instant) -> Result<Value, String> {
    loop {
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Some(result) = serde_json::from_str::<StepResult>(&text).ok()
                    && result.id == id
                {
                    return outcome(result);
                }
            }
            Ok(Message::Ping(data)) => {
                send_frame(socket, Message::Pong(data), deadline, None)?;
            }
            Ok(Message::Close(_)) => return Err(STOPPED.to_owned()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(explain(error)),
        }
    }
}

fn outcome(result: StepResult) -> Result<Value, String> {
    if result.ok {
        Ok(result.value.unwrap_or(Value::Null))
    } else {
        Err(result.error.unwrap_or_else(|| "the step failed".to_owned()))
    }
}

fn explain(error: tungstenite::Error) -> String {
    match error {
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
            STOPPED.to_owned()
        }
        tungstenite::Error::Io(error) if disconnected(&error) => STOPPED.to_owned(),
        tungstenite::Error::Io(error) if error.kind() == ErrorKind::TimedOut => timed_out(),
        other => other.to_string(),
    }
}

fn disconnected(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::UnexpectedEof
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::NotConnected
    )
}

fn timed_out() -> String {
    format!("the app did not answer in {} s", STEP_TIMEOUT.as_secs())
}

fn close_socket(mut socket: Socket) -> Result<(), ()> {
    let _ = socket.get_mut().set_nonblocking(false);
    let _ = socket
        .get_mut()
        .set_write_timeout(Some(Duration::from_secs(1)));
    socket.close(None).map_err(|_| ())?;
    socket.flush().map_err(|_| ())
}
