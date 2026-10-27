//! The engine thread of one surface.
//!
//! The engine owns one [`nlayer_core::Core`] and one [`nlayer_js::Host`] on
//! a dedicated thread: each wake-up (an input event, a resize, new insets)
//! runs the app's code, applies what JS committed, lays out, and hands one
//! [`MountBatch`] to the platform through a [`MountSink`]. The platform
//! thread never blocks on the engine.

use std::{collections::HashMap, sync::Arc, thread::JoinHandle, time::Duration};

use crossbeam_channel::{Receiver, Sender, unbounded};
use nlayer_core::{
    Core, EventKind, MountBatch, NodeId,
    geometry::{Edges, Points, Size},
    text::TextEngine,
};
pub use nlayer_js::EventPayload;
use nlayer_js::Host;

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
}

impl EngineConfig {
    /// A config for `bundle` on a `viewport`.
    pub fn new(bundle: impl Into<String>, viewport: Size<Points>) -> Self {
        Self {
            bundle: bundle.into(),
            viewport,
        }
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

/// A running surface.
pub struct Engine {
    control: EngineControl,
    thread: Option<JoinHandle<()>>,
}

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
        let thread = std::thread::Builder::new()
            .name("nlayer-engine".to_owned())
            .spawn(move || run(config, text_engine, sink, rx, ready_tx))
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

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.control.tx.send(Message::Stop);
    }
}

type Ready = Sender<Result<(), EngineError>>;

fn run<M, F>(
    config: EngineConfig,
    text_engine: F,
    sink: Arc<dyn MountSink>,
    rx: Receiver<Message>,
    ready: Ready,
) where
    M: TextEngine + 'static,
    F: FnOnce() -> M,
{
    let mut core = Core::new(text_engine(), config.viewport);
    let host = match Host::new() {
        Ok(host) => host,
        Err(e) => {
            let _ = ready.send(Err(EngineError::new(e.message)));
            return;
        }
    };
    if let Err(e) = host.eval_source(&config.bundle) {
        let _ = ready.send(Err(EngineError::new(e.message)));
        return;
    }
    pump(&host, &mut core, &*sink);
    if ready.send(Ok(())).is_err() {
        return;
    }
    let mut edits: HashMap<NodeId, u32> = HashMap::new();
    loop {
        let wait = host.next_timer().unwrap_or(Duration::from_secs(3600));
        let message = match rx.recv_timeout(wait) {
            Ok(message) => message,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                host.run_due_timers();
                pump(&host, &mut core, &*sink);
                continue;
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };
        match message {
            Message::Event { id, event, payload } => host.dispatch_event(id, event, &payload),
            Message::Input { id, text } => {
                let count = edits.entry(id).or_insert(0);
                *count += 1;
                core.input_changed(id, &text, *count);
                host.dispatch_event(id, EventKind::Change, &EventPayload::Text(text));
            }
            Message::Resize(size) => core.set_viewport(size),
            Message::SafeArea(insets) => core.set_safe_area(insets),
            Message::KeyboardInset(height) => core.set_keyboard_inset(height),
            Message::Stop => break,
        }
        host.run_due_timers();
        pump(&host, &mut core, &*sink);
    }
}

/// Applies what JS queued, lays out, and hands the frame to the platform.
fn pump<M: TextEngine>(host: &Host, core: &mut Core<M>, sink: &dyn MountSink) {
    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    for op in ops {
        if let Err(error) = core.apply(op) {
            if error.is_fatal() {
                sink.failed(EngineError::new(format!("{error:?}")));
                return;
            }
            log::error!("{error:?}");
        }
    }
    if let Some(batch) = core.commit() {
        sink.submit(batch);
    }
}
