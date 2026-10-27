//! The engine of one surface.
//!
//! [`Engine`] runs it on a dedicated thread (native targets). [`LocalEngine`]
//! runs it on the caller's thread, which is the only engine on wasm: the page
//! has no thread of its own, so it sends input and calls [`LocalEngine::pump`]
//! when a timer is due or, on the QuickJS host, a fetch finishes. The browser
//! host does not run `fetch` (the page's does). ureq is that host's HTTP stack
//! and does not build for wasm. Either way one [`nlayer_core::Core`] applies what the app
//! committed, lays out, and hands one [`MountBatch`] to the platform through
//! a [`MountSink`]. Feature `js` commits from QuickJS. Feature `browser`
//! commits ops the page queued and links no QuickJS. The platform thread never
//! blocks on [`Engine`].
//!
//! [`LocalEngine`] follows `nsh_engine::LocalEngine`, cut to this loop: no
//! component tasks, display frames or posted JS calls.

#[cfg(not(any(feature = "js", feature = "browser")))]
compile_error!("nlayer-engine requires feature `js` or `browser`");
#[cfg(all(feature = "js", feature = "browser"))]
compile_error!("nlayer-engine features `js` and `browser` cannot be combined");

#[cfg(feature = "browser")]
mod browser;
#[cfg(feature = "browser")]
mod host_types;
#[cfg(feature = "browser")]
pub use browser::{Bridge, clear_registration, register};
#[cfg(feature = "browser")]
pub use host_types::{EventPayload, Os};

#[cfg(all(not(target_arch = "wasm32"), feature = "js"))]
mod net;

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[cfg(not(target_arch = "wasm32"))]
use std::thread::JoinHandle;

#[cfg(feature = "browser")]
use browser::Script as Vm;
use crossbeam_channel::{Receiver, Sender, unbounded};
use nlayer_core::{
    Core, EventKind, MountBatch, NodeId,
    geometry::{Edges, Points, Size},
    text::TextEngine,
};
#[cfg(feature = "js")]
use nlayer_js::Host as Vm;
#[cfg(feature = "js")]
pub use nlayer_js::{EventPayload, HttpRequest, HttpResponse, NetworkRequest, Os};

/// A platform that applies mount batches.
pub trait MountSink: Send + Sync + 'static {
    /// Applies one frame.
    fn submit(&self, batch: MountBatch);

    /// The app failed (it threw outside a handler, or the VM did not
    /// start).
    fn failed(&self, error: EngineError) {
        log::error!("{error}");
    }
}

/// A failure to start or run the app.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct EngineError {
    /// What failed, as JS reported it.
    pub message: String,
}

impl EngineError {
    pub(crate) fn new(message: String) -> Self {
        Self { message }
    }
}

/// What a surface starts with.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// The app bundle (an ES module).
    pub bundle: String,
    /// The surface's size in points.
    pub viewport: Size<Points>,
    /// What `platform.os` reports. [`EngineConfig::new`] is [`Os::Web`].
    pub os: Os,
}

impl EngineConfig {
    /// A config for `bundle` on a `viewport`.
    pub fn new(bundle: impl Into<String>, viewport: Size<Points>) -> Self {
        Self {
            bundle: bundle.into(),
            viewport,
            os: Os::Web,
        }
    }

    /// `platform.os` on this surface.
    pub fn with_os(mut self, os: Os) -> Self {
        self.os = os;
        self
    }
}

/// One instruction for the engine thread.
#[derive(Debug, Clone)]
enum Message {
    Event {
        id: NodeId,
        event: EventKind,
        payload: EventPayload,
    },
    Input {
        id: NodeId,
        text: String,
    },
    Resize(Size<Points>),
    SafeArea(Edges<Points>),
    KeyboardInset(Points),
    /// A request finished (its result is in the engine's slot).
    NetworkDone,
    /// The engine thread was asked to exit.
    #[cfg(not(target_arch = "wasm32"))]
    Stop,
}

/// The surface's side of the engine.
#[derive(Clone)]
pub struct EngineControl {
    tx: Sender<Message>,
}

impl EngineControl {
    /// Calls node `id`'s handler of `event` on the engine thread.
    pub fn dispatch_event(&self, id: NodeId, event: EventKind, payload: EventPayload) {
        let _ = self.tx.send(Message::Event { id, event, payload });
    }

    /// The user typed `text` into input `id` (the whole new text): the
    /// app's `onChange` gets it and the input shows it.
    pub fn input_changed(&self, id: NodeId, text: impl Into<String>) {
        let _ = self.tx.send(Message::Input {
            id,
            text: text.into(),
        });
    }

    /// The surface changed size (points).
    pub fn resize(&self, size: Size<Points>) {
        let _ = self.tx.send(Message::Resize(size));
    }

    /// The safe area changed: `env(safe-area-inset-*)` follows it.
    pub fn set_safe_area(&self, insets: Edges<Points>) {
        let _ = self.tx.send(Message::SafeArea(insets));
    }

    /// The software keyboard's height over the surface: `env(keyboard-inset-height)`
    /// follows it.
    pub fn set_keyboard_inset(&self, height: Points) {
        let _ = self.tx.send(Message::KeyboardInset(height));
    }
}

/// A running surface. Native only: wasm drives [`LocalEngine`].
#[cfg(not(target_arch = "wasm32"))]
pub struct Engine {
    control: EngineControl,
    thread: Option<JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Engine {
    /// Starts the engine thread, loads the app and lays out its first
    /// frame before returning.
    pub fn start<M, F>(
        config: EngineConfig,
        text_engine: F,
        sink: Arc<dyn MountSink>,
    ) -> Result<Engine, EngineError>
    where
        M: TextEngine + 'static,
        F: FnOnce() -> M + Send + 'static,
    {
        let (tx, rx) = unbounded();
        let (ready_tx, ready_rx) = unbounded();
        let wake = tx.clone();
        let thread = std::thread::Builder::new()
            .name("nlayer-engine".to_owned())
            .spawn(move || run(config, text_engine, sink, rx, wake, ready_tx))
            .map_err(|e| EngineError::new(format!("could not start the engine thread: {e}")))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Engine {
                control: EngineControl { tx },
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => Err(EngineError::new("the engine thread stopped".to_owned())),
        }
    }

    /// The surface's side of the engine.
    pub fn control(&self) -> EngineControl {
        self.control.clone()
    }

    /// Stops the engine thread and waits for it.
    pub fn stop(mut self) {
        let _ = self.control.tx.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.control.tx.send(Message::Stop);
    }
}

/// Called when a fetch finishes outside [`LocalEngine::pump`]. It may run
/// on the fetch thread: schedule a pump, do not call [`LocalEngine::pump`]
/// from it (the VM is not `Send`, and a pump must not re-enter).
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// An engine driven by its host's event loop instead of a thread of its
/// own. Input sent through [`EngineControl`] is handled at the next
/// [`pump`](Self::pump). A burst of input is one frame.
pub struct LocalEngine<M: TextEngine> {
    session: Session<M>,
    rx: Receiver<Message>,
    control: EngineControl,
    /// Fetches finished outside a pump send here and call [`Wake`].
    notify: Arc<LocalNotify>,
    stopped: bool,
}

struct LocalNotify {
    tx: Sender<Message>,
    pumping: AtomicBool,
    host: Mutex<Wake>,
}

impl LocalNotify {
    fn new(tx: Sender<Message>, host: Wake) -> Self {
        Self {
            tx,
            pumping: AtomicBool::new(false),
            host: Mutex::new(host),
        }
    }

    /// Queues the finished fetch, then tells the host unless a pump is
    /// already draining the queue (it will see the message).
    fn wake(&self) {
        let _ = self.tx.send(Message::NetworkDone);
        if !self.pumping.load(Ordering::SeqCst) {
            (self.host.lock().expect("wake"))();
        }
    }

    fn call_host(&self) {
        (self.host.lock().expect("wake"))();
    }
}

impl<M: TextEngine> LocalEngine<M> {
    /// Loads the app and lays out its first frame before returning.
    /// `wake` runs when a fetch finishes and no pump is draining the queue.
    /// On the QuickJS host, fetches run on ureq. The browser host queues
    /// none: the page's `fetch` is the browser's.
    pub fn start(
        config: EngineConfig,
        text_engine: M,
        sink: Arc<dyn MountSink>,
        wake: Wake,
    ) -> Result<Self, EngineError> {
        Self::open(config, text_engine, sink, wake)
    }

    fn open(
        config: EngineConfig,
        text_engine: M,
        sink: Arc<dyn MountSink>,
        wake: Wake,
    ) -> Result<Self, EngineError> {
        let (tx, rx) = unbounded();
        let notify = Arc::new(LocalNotify::new(tx.clone(), wake));
        let hook: Wake = {
            let notify = Arc::clone(&notify);
            Arc::new(move || notify.wake())
        };
        let mut session = Session::open(config, text_engine, sink, hook)?;
        session.commit();
        Ok(LocalEngine {
            session,
            rx,
            control: EngineControl { tx },
            notify,
            stopped: false,
        })
    }

    /// A handle for sending input.
    pub fn control(&self) -> EngineControl {
        self.control.clone()
    }

    /// How long until the next timer (`None`: only input and [`Wake`]).
    pub fn deadline(&self) -> Option<Duration> {
        self.session.next_timer()
    }

    /// Handles the input sent since the last call, runs the timers that are
    /// due and commits. Returns when to call again at the latest (`None`:
    /// only for input or [`Wake`]).
    pub fn pump(&mut self) -> Option<Duration> {
        if self.stopped {
            return None;
        }
        self.notify.pumping.store(true, Ordering::SeqCst);
        // Bounded: a fetch that finishes while this pump runs is handled
        // here too. A loop that never settles is not.
        for _ in 0..4 {
            let mut batch = Vec::new();
            while let Ok(message) = self.rx.try_recv() {
                batch.push(message);
            }
            let stop = if batch.is_empty() {
                self.session.on_timer();
                false
            } else {
                !self.session.on_burst(batch)
            };
            if stop {
                self.stopped = true;
                break;
            }
            if self.rx.is_empty() {
                break;
            }
        }
        // After the flag: a fetch that lands as the pump ends either is
        // still in the queue (the host is called here) or sees the flag
        // clear and calls the host itself.
        self.notify.pumping.store(false, Ordering::SeqCst);
        if !self.rx.is_empty() {
            self.notify.call_host();
        }
        self.session.next_timer()
    }
}

#[cfg(not(target_arch = "wasm32"))]
type Ready = Sender<Result<(), EngineError>>;

#[cfg(not(target_arch = "wasm32"))]
fn run<M, F>(
    config: EngineConfig,
    text_engine: F,
    sink: Arc<dyn MountSink>,
    rx: Receiver<Message>,
    wake: Sender<Message>,
    ready: Ready,
) where
    M: TextEngine + 'static,
    F: FnOnce() -> M,
{
    let notify: Wake = Arc::new(move || {
        let _ = wake.send(Message::NetworkDone);
    });
    let mut session = match Session::open(config, text_engine(), sink, notify) {
        Ok(session) => session,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    session.commit();
    if ready.send(Ok(())).is_err() {
        return;
    }
    loop {
        let wait = session.next_timer().unwrap_or(Duration::from_secs(3600));
        match rx.recv_timeout(wait) {
            Ok(message) => {
                if !session.on_message(message) {
                    break;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => session.on_timer(),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// A request's result, waiting until the engine applies it.
#[cfg(all(not(target_arch = "wasm32"), feature = "js"))]
type ResultSlot = Arc<Mutex<Vec<(u32, Result<HttpResponse, String>)>>>;

/// One engine's state, whoever drives it.
struct Session<M: TextEngine> {
    host: Vm,
    core: Core<M>,
    sink: Arc<dyn MountSink>,
    #[cfg(all(not(target_arch = "wasm32"), feature = "js"))]
    results: ResultSlot,
    edits: HashMap<NodeId, u32>,
    /// Wakes the engine's HTTP stack. The browser host has none.
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(feature = "browser", allow(dead_code))]
    notify: Wake,
}

impl<M: TextEngine> Session<M> {
    fn open(
        config: EngineConfig,
        text_engine: M,
        sink: Arc<dyn MountSink>,
        notify: Wake,
    ) -> Result<Self, EngineError> {
        let core = Core::new(text_engine, config.viewport);
        #[cfg(feature = "js")]
        let host = {
            let host = nlayer_js::Host::new().map_err(|error| EngineError::new(error.message))?;
            host.set_os(config.os);
            host.eval_source(&config.bundle)
                .map_err(|error| EngineError::new(error.message))?;
            host
        };
        #[cfg(feature = "browser")]
        let host = browser::Script::start(config.os).map_err(EngineError::new)?;
        #[cfg(target_arch = "wasm32")]
        let _ = notify;
        Ok(Session {
            host,
            core,
            sink,
            #[cfg(all(not(target_arch = "wasm32"), feature = "js"))]
            results: Arc::default(),
            edits: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            notify,
        })
    }

    fn next_timer(&self) -> Option<Duration> {
        self.host.next_timer()
    }

    /// One message, then one frame. `false`: the engine was stopped.
    /// The thread engine's path; wasm commits through [`on_burst`](Self::on_burst).
    #[cfg(not(target_arch = "wasm32"))]
    fn on_message(&mut self, message: Message) -> bool {
        #[cfg(feature = "js")]
        complete_network(&self.host, &self.results);
        if !self.apply(message) {
            return false;
        }
        self.host.run_due_timers();
        self.commit();
        true
    }

    /// A timer fired and nothing else is waiting.
    fn on_timer(&mut self) {
        self.host.run_due_timers();
        self.commit();
    }

    /// One burst of input, then one frame. `false`: the engine was stopped.
    fn on_burst(&mut self, messages: Vec<Message>) -> bool {
        #[cfg(all(not(target_arch = "wasm32"), feature = "js"))]
        complete_network(&self.host, &self.results);
        for message in messages {
            if !self.apply(message) {
                return false;
            }
        }
        self.host.run_due_timers();
        self.commit();
        true
    }

    fn apply(&mut self, message: Message) -> bool {
        match message {
            Message::Event { id, event, payload } => {
                self.host.dispatch_event(id, event, &payload);
            }
            Message::Input { id, text } => {
                let count = self.edits.entry(id).or_insert(0);
                *count += 1;
                self.core.input_changed(id, &text, *count);
                self.host
                    .dispatch_event(id, EventKind::Change, &EventPayload::Text(text));
            }
            Message::Resize(size) => self.core.set_viewport(size),
            Message::SafeArea(insets) => self.core.set_safe_area(insets),
            Message::KeyboardInset(height) => self.core.set_keyboard_inset(height),
            Message::NetworkDone => {}
            #[cfg(not(target_arch = "wasm32"))]
            Message::Stop => return false,
        }
        true
    }

    /// Applies what JS queued, lays out, and hands the frame to the platform.
    fn commit(&mut self) {
        let mut ops = Vec::new();
        self.host.take_ops(&mut ops);
        for op in ops {
            if let Err(error) = self.core.apply(op) {
                if error.is_fatal() {
                    self.sink.failed(EngineError::new(format!("{error:?}")));
                    return;
                }
                log::error!("{error:?}");
            }
        }
        if let Some(batch) = self.core.commit() {
            self.sink.submit(batch);
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "js"))]
        {
            let mut requests = Vec::new();
            self.host.take_network(&mut requests);
            for request in requests {
                let notify = Arc::clone(&self.notify);
                net::start(request, Arc::clone(&self.results), move || notify());
            }
        }
    }
}

/// Hands JS the responses that arrived.
#[cfg(all(not(target_arch = "wasm32"), feature = "js"))]
fn complete_network(host: &Vm, results: &ResultSlot) {
    let done = std::mem::take(&mut *results.lock().expect("network results"));
    for (id, result) in done {
        host.complete_network(id, result);
    }
}
