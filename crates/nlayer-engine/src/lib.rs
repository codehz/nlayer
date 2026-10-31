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
#[cfg(not(target_arch = "wasm32"))]
mod storage;

pub use nlayer_core::storage::{StorageBackend, StorageChange, Store};
#[cfg(not(target_arch = "wasm32"))]
pub use storage::{FileStorage, data_dir};

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
    ColorScheme, Core, EventKind, ListBind, MountBatch, NodeId,
    animation::Finished,
    geometry::{Edges, Point, Points, Size},
    text::TextEngine,
};
#[cfg(feature = "js")]
use nlayer_js::Host as Vm;
#[cfg(feature = "js")]
pub use nlayer_js::{EventPayload, HttpRequest, HttpResponse, NetworkOp, NetworkRequest, Os};

/// A platform that applies mount batches.
pub trait MountSink: Send + Sync + 'static {
    /// Applies one frame.
    fn submit(&self, batch: MountBatch);

    /// The app failed (it threw outside a handler, or the VM did not
    /// start).
    fn failed(&self, error: EngineError) {
        log::error!("{error}");
    }

    /// The back button reached the app and no `BackHandler` listener
    /// handled it: the platform's default (Android: send the task to the
    /// background; a page: nothing, the browser already went back).
    fn back_unhandled(&self) {}
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
    /// Safe area applied before the first frame. [`EngineConfig::new`] is zero.
    pub safe_area: Edges<Points>,
    /// `env(keyboard-inset-height)` before the first frame.
    pub keyboard: Points,
    /// Where the app keeps its data: `storage` of `@nlayer/runtime` is
    /// `storage.json` there. [`EngineConfig::new`] is none (in memory).
    pub data_dir: Option<std::path::PathBuf>,
    /// The system color scheme `platform.colorScheme()` reports before the
    /// first frame. [`EngineConfig::new`] is [`ColorScheme::Light`].
    pub color_scheme: ColorScheme,
    /// The app's storage, overriding the file [`EngineConfig::data_dir`]
    /// names.
    pub storage: Option<Arc<dyn StorageBackend>>,
}

impl EngineConfig {
    /// A config for `bundle` on a `viewport`.
    pub fn new(bundle: impl Into<String>, viewport: Size<Points>) -> Self {
        Self {
            bundle: bundle.into(),
            viewport,
            os: Os::Web,
            safe_area: Edges::default(),
            keyboard: Points::ZERO,
            data_dir: None,
            storage: None,
            color_scheme: ColorScheme::Light,
        }
    }

    /// `platform.os` on this surface.
    pub fn with_os(mut self, os: Os) -> Self {
        self.os = os;
        self
    }

    /// Safe area and keyboard height for the first frame (`open`'s pin).
    pub fn with_insets(mut self, safe_area: Edges<Points>, keyboard: Points) -> Self {
        self.safe_area = safe_area;
        self.keyboard = keyboard;
        self
    }

    /// The directory the app keeps its storage in.
    pub fn with_data_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.data_dir = Some(dir.into());
        self
    }

    /// The app's storage backend, overriding [`EngineConfig::data_dir`].
    pub fn with_storage(mut self, storage: Arc<dyn StorageBackend>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// The system color scheme before the first frame.
    pub fn with_color_scheme(mut self, scheme: ColorScheme) -> Self {
        self.color_scheme = scheme;
        self
    }
}

/// The storage of the app `config` runs: [`EngineConfig::storage`], or a
/// file in its directory of [`EngineConfig::data_dir`].
#[cfg(feature = "js")]
fn app_storage(config: &EngineConfig) -> Option<Arc<dyn StorageBackend>> {
    if let Some(storage) = &config.storage {
        return Some(storage.clone());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(root) = &config.data_dir {
        return Some(FileStorage::open(root));
    }
    None
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
    /// The system's color scheme changed.
    ColorScheme(ColorScheme),
    /// The platform's back button, or history back.
    Back,
    /// A request finished (its result is in the engine's slot).
    NetworkDone,
    /// Native animations finished or were interrupted.
    AnimationFinished(Vec<Finished>),
    /// Acknowledge after earlier messages have committed.
    #[cfg(not(target_arch = "wasm32"))]
    Barrier(Sender<()>),
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

    /// Waits until earlier messages have been committed by the engine thread.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn synchronize(&self) -> Result<(), EngineError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.tx
            .send(Message::Barrier(tx))
            .map_err(|_| EngineError::new("engine stopped".to_owned()))?;
        rx.recv()
            .map_err(|_| EngineError::new("engine stopped".to_owned()))
    }

    /// The selection or caret of input `id` moved (UTF-16 units): the
    /// app's `onSelectionChange` gets it.
    pub fn selection_changed(&self, id: NodeId, start: u32, end: u32) {
        self.dispatch_event(
            id,
            EventKind::SelectionChange,
            EventPayload::Selection { start, end },
        );
    }

    /// The composition of input `id` changed: the app's
    /// `onCompositionChange` gets it (`text` is empty once it ended).
    pub fn composition_changed(&self, id: NodeId, composing: bool, text: impl Into<String>) {
        self.dispatch_event(
            id,
            EventKind::CompositionChange,
            EventPayload::Composition {
                composing,
                text: text.into(),
            },
        );
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

    /// The system's color scheme changed: `platform.colorScheme()` follows
    /// it, and what read it lays out again.
    pub fn set_color_scheme(&self, scheme: ColorScheme) {
        let _ = self.tx.send(Message::ColorScheme(scheme));
    }

    /// The platform's back button (Android) or history back (a page that
    /// pushed an entry). `BackHandler` listeners run newest first; when
    /// none handles it, [`MountSink::back_unhandled`] runs.
    pub fn back(&self) {
        let _ = self.tx.send(Message::Back);
    }

    /// Reports native animations that finished or were interrupted, so
    /// the promises JS made settle. Call it on the platform's UI thread.
    pub fn animations_finished(&self, finished: Vec<Finished>) {
        let _ = self.tx.send(Message::AnimationFinished(finished));
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
    /// The requests running on the network, to cancel one whose promise
    /// was aborted.
    #[cfg(all(not(target_arch = "wasm32"), feature = "js"))]
    fetches: HashMap<u32, net::Fetch>,
    edits: HashMap<NodeId, u32>,
    /// Wakes the engine's HTTP stack. The browser host has none.
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(feature = "browser", allow(dead_code))]
    notify: Wake,
    /// Cells a list asked for that this frame's budget did not reach.
    pending_binds: Vec<ListBind>,
    /// JS ops the last round's paced binds queued; the next turn applies
    /// them, so none are lost when the rounds end.
    queued_ops: Vec<nlayer_core::Op>,
    /// [`keep_last_sets`]'s buffer, reused across turns.
    sets_seen: HashMap<(u8, NodeId), usize>,
}

/// Rounds of "list asks for items -> JS renders them -> layout" folded into
/// one frame. Bounded so a list that never settles cannot stall the engine.
const MAX_LIST_ROUNDS: usize = 8;
/// The overscan a turn binds: cells until their ops reach this many or their
/// JS takes [`PACED_BIND_TIME`], at least one. A fast scroll or a jump asks
/// for a screen of cells at once and each bind rebuilds a cell's subtree;
/// what shows (the viewport, the leading edge, cells whose item moved) binds
/// regardless, the rest in later turns.
const PACED_BIND_OPS: usize = 150;
/// See [`PACED_BIND_OPS`].
const PACED_BIND_TIME: Duration = Duration::from_millis(2);

impl<M: TextEngine> Session<M> {
    fn open(
        config: EngineConfig,
        text_engine: M,
        sink: Arc<dyn MountSink>,
        notify: Wake,
    ) -> Result<Self, EngineError> {
        let mut core = Core::new(text_engine, config.viewport);
        core.set_safe_area(config.safe_area);
        core.set_keyboard_inset(config.keyboard);
        #[cfg(feature = "js")]
        let host = {
            let host = nlayer_js::Host::new().map_err(|error| EngineError::new(error.message))?;
            host.set_os(config.os);
            host.set_color_scheme(config.color_scheme);
            host.set_storage(Store::new(app_storage(&config)));
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
            #[cfg(all(not(target_arch = "wasm32"), feature = "js"))]
            fetches: HashMap::new(),
            edits: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            notify,
            pending_binds: Vec::new(),
            queued_ops: Vec::new(),
            sets_seen: HashMap::new(),
        })
    }

    fn next_timer(&self) -> Option<Duration> {
        if !self.pending_binds.is_empty()
            || !self.queued_ops.is_empty()
            || self.core.has_paced_list_binds()
        {
            return Some(Duration::ZERO);
        }
        self.host.next_timer()
    }

    /// One message, then one frame. `false`: the engine was stopped.
    /// The thread engine's path; wasm commits through [`on_burst`](Self::on_burst).
    #[cfg(not(target_arch = "wasm32"))]
    fn on_message(&mut self, message: Message) -> bool {
        if let Message::Barrier(reply) = message {
            let _ = reply.send(());
            return true;
        }
        #[cfg(feature = "js")]
        complete_network(&self.host, &self.results, &mut self.fetches);
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
        complete_network(&self.host, &self.results, &mut self.fetches);
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
                if event == EventKind::Scroll
                    && let EventPayload::Scroll { x, y, .. } = &payload
                {
                    self.core
                        .set_scroll_offset(id, Point::new(Points(*x), Points(*y)));
                }
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
            Message::ColorScheme(scheme) => {
                #[cfg(feature = "js")]
                self.host.set_color_scheme(scheme);
                #[cfg(feature = "browser")]
                let _ = scheme;
            }
            Message::Back => {
                let handled = {
                    #[cfg(feature = "js")]
                    {
                        self.host.dispatch_back()
                    }
                    #[cfg(feature = "browser")]
                    {
                        false
                    }
                };
                if !handled {
                    self.sink.back_unhandled();
                }
            }
            Message::AnimationFinished(finished) => {
                self.host.animations_finished(&finished);
            }
            Message::NetworkDone => {}
            #[cfg(not(target_arch = "wasm32"))]
            Message::Barrier(_) => unreachable!("barriers are handled before apply"),
            #[cfg(not(target_arch = "wasm32"))]
            Message::Stop => return false,
        }
        true
    }

    /// Applies what JS queued, lays out, and hands the frame to the platform.
    /// A list's cells are rendered across rounds (up to
    /// [`MAX_LIST_ROUNDS`]); every round's mount items go into one batch.
    /// What the viewport shows binds in the round that assigns it; a list's
    /// overscan waits in the core and binds within this turn's budget
    /// ([`PACED_BIND_OPS`]), the rest in later turns.
    fn commit(&mut self) {
        let mut merged: Vec<nlayer_core::MountItem> = Vec::new();
        let mut paced = PacedBinds::default();
        let mut commits = 0;
        let mut failed = false;
        for _ in 0..MAX_LIST_ROUNDS {
            // Requests from the previous round first: they must show.
            for bind in std::mem::take(&mut self.pending_binds) {
                self.bind_list(bind.list, &bind.binds);
            }
            let mut ops = std::mem::take(&mut self.queued_ops);
            self.host.take_ops(&mut ops);
            for op in ops {
                if let Err(error) = self.core.apply(op) {
                    if error.is_fatal() {
                        self.sink.failed(EngineError::new(format!("{error:?}")));
                        failed = true;
                        break;
                    }
                    log::error!("{error:?}");
                }
            }
            if failed {
                break;
            }
            if let Some(batch) = self.core.commit() {
                commits += 1;
                extend_frame(&mut merged, batch.items);
            }
            let mut events = Vec::new();
            self.core.take_events(&mut events);
            for (id, event) in &events {
                self.host.dispatch_event(*id, *event, &EventPayload::None);
            }
            // The window's own binds: the next round renders them.
            let mut binds: Vec<ListBind> = Vec::new();
            self.core.take_list_binds(&mut binds);
            self.pending_binds.extend(binds);
            // The overscan, this turn's budget allowing.
            let mut one: Vec<ListBind> = Vec::new();
            while paced.left() && self.core.has_paced_list_binds() {
                let start = std::time::Instant::now();
                let ops = self.queued_ops.len();
                one.clear();
                self.core.take_paced_list_binds(1, &mut one);
                for bind in &one {
                    paced.cells += bind.binds.len();
                    self.bind_list(bind.list, &bind.binds);
                }
                self.host.take_ops(&mut self.queued_ops);
                paced.ops += self.queued_ops.len() - ops;
                paced.time += start.elapsed();
            }
            // New cells queued JS ops; another round applies them. With the
            // budget spent, the rest waits for the next turn.
            if self.pending_binds.is_empty()
                && events.is_empty()
                && self.queued_ops.is_empty()
                && !(paced.left() && self.core.has_paced_list_binds())
            {
                break;
            }
        }
        if commits > 1 {
            keep_last_sets(&mut merged, &mut self.sets_seen);
        }
        if !merged.is_empty() {
            self.sink.submit(MountBatch { items: merged });
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "js"))]
        {
            let mut work = Vec::new();
            self.host.take_network(&mut work);
            for op in work {
                match op {
                    NetworkOp::Start(request) => {
                        let id = request.id;
                        let notify = Arc::clone(&self.notify);
                        let fetch =
                            net::start(request, Arc::clone(&self.results), move || notify());
                        self.fetches.insert(id, fetch);
                    }
                    NetworkOp::Cancel(id) => {
                        if let Some(fetch) = self.fetches.remove(&id) {
                            fetch.cancel();
                        }
                    }
                }
            }
        }
    }

    fn bind_list(&mut self, id: NodeId, binds: &[(u32, u32)]) {
        #[cfg(feature = "js")]
        {
            self.host.bind_list(id, binds);
        }
        #[cfg(feature = "browser")]
        {
            self.host.bind_list(id, binds);
        }
    }
}

/// What a turn spent binding overscan cells.
#[derive(Default)]
struct PacedBinds {
    cells: usize,
    ops: usize,
    time: Duration,
}

impl PacedBinds {
    /// Whether the turn may bind another: at least one, then within the
    /// budget.
    fn left(&self) -> bool {
        self.cells == 0 || (self.ops < PACED_BIND_OPS && self.time < PACED_BIND_TIME)
    }
}

/// Appends a commit's items to the frame's. A scroll view's content size
/// that the frame already sets is updated to the later size (unless the
/// view scrolls in between): a row relaid over two commits must not send
/// the platform a transient size, which clamps the offset before the
/// content grows back (a list jumping when a row expands).
fn extend_frame(frame: &mut Vec<nlayer_core::MountItem>, items: Vec<nlayer_core::MountItem>) {
    use nlayer_core::MountItem;
    let start = frame.len();
    frame.extend(items);
    for k in start..frame.len() {
        let MountItem::ScrollContent { id, size } = frame[k] else {
            continue;
        };
        let earlier = frame[..k].iter().rposition(|m| {
            matches!(m, MountItem::ScrollContent { id: e, .. } | MountItem::ScrollBy { id: e, .. } if *e == id)
        });
        if let Some(j) = earlier
            && let MountItem::ScrollContent { size: s, .. } = &mut frame[j]
        {
            *s = size;
        }
    }
}

/// Of the items of a frame merged from several commits that set one part
/// of a view whole (its frame, style, text, ...), keeps the last: the
/// platform would apply each in one pass, and a list's rounds (binds, then
/// the paced ones) relay and restyle the same cells. Once per turn, with
/// `last` the reused buffer.
fn keep_last_sets(
    frame: &mut Vec<nlayer_core::MountItem>,
    last: &mut HashMap<(u8, NodeId), usize>,
) {
    last.clear();
    let mut repeated = false;
    for (k, item) in frame.iter().enumerate() {
        if let Some(key) = sets_whole(item) {
            repeated |= last.insert(key, k).is_some();
        }
    }
    if !repeated {
        return;
    }
    let mut k = 0;
    frame.retain(|item| {
        k += 1;
        sets_whole(item).is_none_or(|key| last[&key] == k - 1)
    });
}

/// The part of a view an item sets whole, and the view: of two such items
/// the later is all the view ends up with.
fn sets_whole(item: &nlayer_core::MountItem) -> Option<(u8, NodeId)> {
    use nlayer_core::MountItem;
    Some(match *item {
        MountItem::Frame { id, .. } => (0, id),
        MountItem::ViewStyle { id, .. } => (1, id),
        MountItem::Text { id, .. } => (2, id),
        MountItem::Events { id, .. } => (3, id),
        MountItem::Image { id, .. } => (4, id),
        _ => return None,
    })
}

/// Hands JS the responses that arrived.
#[cfg(all(not(target_arch = "wasm32"), feature = "js"))]
fn complete_network(host: &Vm, results: &ResultSlot, fetches: &mut HashMap<u32, net::Fetch>) {
    let done = std::mem::take(&mut *results.lock().expect("network results"));
    for (id, result) in done {
        fetches.remove(&id);
        host.complete_network(id, result);
    }
}
