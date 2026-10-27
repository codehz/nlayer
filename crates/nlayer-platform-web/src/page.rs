//! One surface running on the caller's thread: the page's engine.
//!
//! [`PageSession`] loads the bundle, lays out with the headless text
//! stand-in and hands each frame to the page as the JSON [`Encoder`]
//! writes. The page applies it (`web/nlayer.js`), answers `fetch` and
//! reports presses, typing, scrolling, resizes and insets. Measurement
//! is [`ApproxTextEngine`] (the same stand-in as `nlayer test`); the
//! browser wraps the attributed spans in the box it produced. Glyph
//! measurement, hot reload and bytecode are not here. The wasm exports
//! (`nlayer_browser_start`, `nlayer_pump`) call this module. The bundle
//! string is evaluated only by the native QuickJS host; on wasm the page
//! owns the script and the string is empty.
//!
//! Extracted from `nsh-platform-web`'s page loop (`src/lib.rs`, the
//! `nsh_start` / `nsh_pump` / `nsh_event` exports) and cut to that set.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use nlayer_core::{
    EventKind, NodeId,
    geometry::{Edges, Points, Size},
    text::ApproxTextEngine,
};
use nlayer_engine::{
    EngineConfig, EngineError, EventPayload, HttpResponse, LocalEngine, MountSink, NetworkRequest,
    Wake,
};

use crate::Encoder;

/// What the page does for the running surface.
///
/// Called on the engine's thread, which on wasm is the page's. `submit`
/// and `fetch` run inside a pump or [`PageSession::start`] and must not
/// call back into the session: the DOM mounter applies a batch and the
/// page starts a `fetch`, then returns. [`Page::request_pump`] only
/// schedules a later [`PageSession::pump`].
pub trait Page: Send + Sync + 'static {
    /// One encoded mount batch (a JSON array).
    fn submit(&self, json: &str);

    /// A fetch finished outside a pump, or a pump left a message queued.
    /// Schedule [`PageSession::pump`]; do not call it from here.
    fn request_pump(&self);

    /// The app failed outside a handler, or a frame could not be applied.
    fn report_error(&self, message: &str);

    /// A `fetch` the app started. Copy what is needed and return; answer
    /// later with [`PageSession::complete_fetch`].
    fn fetch(&self, request: &NetworkRequest);
}

/// The engine of one page, driven by the page.
pub struct PageSession {
    engine: LocalEngine<ApproxTextEngine>,
    page: Arc<dyn Page>,
}

impl PageSession {
    /// Loads `bundle` and lays out the first frame before returning.
    /// Fetches the bundle started are handed to [`Page::fetch`] before
    /// this returns.
    pub fn start(
        bundle: impl Into<String>,
        viewport: Size<Points>,
        page: impl Page,
    ) -> Result<Self, EngineError> {
        let page: Arc<dyn Page> = Arc::new(page);
        let sink = Arc::new(Sink {
            page: Arc::clone(&page),
            encoder: Mutex::new(Encoder::new()),
        });
        let wake_page = Arc::clone(&page);
        let wake: Wake = Arc::new(move || wake_page.request_pump());
        let engine = LocalEngine::start_with_host_fetch(
            EngineConfig::new(bundle, viewport).with_os(nlayer_engine::Os::Web),
            ApproxTextEngine,
            sink,
            wake,
        )?;
        let mut session = PageSession { engine, page };
        session.dispatch_fetches();
        Ok(session)
    }

    /// How long until the next timer (`None`: only input and a finished fetch).
    pub fn deadline(&self) -> Option<Duration> {
        self.engine.deadline()
    }

    /// Applies the input queued since the last call, runs due timers and
    /// hands over fetches the app started. Returns when to call again.
    pub fn pump(&mut self) -> Option<Duration> {
        let next = self.engine.pump();
        self.dispatch_fetches();
        next
    }

    /// A click on view `id` (`onPress`). Applied at the next [`pump`](Self::pump).
    pub fn press(&self, id: NodeId) {
        self.engine
            .control()
            .dispatch_event(id, EventKind::Press, EventPayload::None);
    }

    /// The user typed `text` into input `id` (the whole new value).
    pub fn input(&self, id: NodeId, text: impl Into<String>) {
        self.engine.control().input_changed(id, text);
    }

    /// Scroll view `id` scrolled. The page already moved; this fires
    /// `onScroll` (the payload is empty, as the runtime reports it).
    pub fn scroll(&self, id: NodeId) {
        self.engine
            .control()
            .dispatch_event(id, EventKind::Scroll, EventPayload::None);
    }

    /// The surface changed size (CSS pixels, one point each).
    pub fn resize(&self, size: Size<Points>) {
        self.engine.control().resize(size);
    }

    /// Safe-area insets and the keyboard height over the surface.
    /// `env(safe-area-inset-*)` and `env(keyboard-inset-height)` follow them.
    pub fn set_insets(&self, safe_area: Edges<Points>, keyboard: Points) {
        let control = self.engine.control();
        control.set_safe_area(safe_area);
        control.set_keyboard_inset(keyboard);
    }

    /// Answers a request from [`Page::fetch`]. Wakes the page; pump
    /// outside this call.
    pub fn complete_fetch(&self, id: u32, result: Result<HttpResponse, String>) {
        self.engine.complete_fetch(id, result);
    }

    fn dispatch_fetches(&mut self) {
        for request in self.engine.take_fetches() {
            self.page.fetch(&request);
        }
    }
}

struct Sink {
    page: Arc<dyn Page>,
    encoder: Mutex<Encoder>,
}

impl MountSink for Sink {
    fn submit(&self, batch: nlayer_core::MountBatch) {
        let json = self.encoder.lock().expect("encoder").encode(&batch);
        self.page.submit(&json);
    }

    fn failed(&self, error: EngineError) {
        self.page.report_error(&error.message);
    }
}
