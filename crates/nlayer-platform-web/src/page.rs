//! One surface running on the caller's thread: the page's engine.
//!
//! [`PageSession`] loads the bundle, lays out with the headless text
//! stand-in and hands each frame to the page as the JSON [`Encoder`]
//! writes. The page applies it (`web/nlayer.js`) and reports presses,
//! typing, scrolling, resizes and insets. The app's `fetch` is the page's.
//! Measurement
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
    EventKind, NO_NATIVE_HEADER, NodeId,
    geometry::{Edges, Points, Size},
    text::ApproxTextEngine,
};
use nlayer_engine::{EngineConfig, EngineError, EventPayload, LocalEngine, MountSink, Wake};

use crate::Encoder;

/// What the page does for the running surface.
///
/// Called on the engine's thread, which on wasm is the page's. `submit`
/// runs inside a pump or [`PageSession::start`] and must not call back into
/// the session: the DOM mounter applies a batch, then returns.
/// [`Page::request_pump`] only schedules a later [`PageSession::pump`].
pub trait Page: Send + Sync + 'static {
    /// One encoded mount batch (a JSON array).
    fn submit(&self, json: &str);

    /// A fetch finished outside a pump (QuickJS host), or a pump left a
    /// message queued.
    /// Schedule [`PageSession::pump`]; do not call it from here.
    fn request_pump(&self);

    /// Asks the page for the next display frame (it calls [`PageSession::pump`]
    /// when it begins); `false` when the page has none.
    fn request_frame(&self) -> bool {
        false
    }

    /// The app failed outside a handler, or a frame could not be applied.
    fn report_error(&self, message: &str);
}

/// The engine of one page, driven by the page.
pub struct PageSession {
    engine: LocalEngine<ApproxTextEngine>,
}

impl PageSession {
    /// Loads `bundle` and lays out the first frame before returning.
    /// On wasm `bundle` is empty: the page evaluates the app.
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
        let engine = LocalEngine::start(
            EngineConfig::new(bundle, viewport)
                .with_os(nlayer_engine::Os::Web)
                .with_header_heights(NO_NATIVE_HEADER),
            ApproxTextEngine,
            sink,
            wake,
        )?;
        Ok(PageSession { engine })
    }

    /// How long until the next timer (`None`: only input, and a finished fetch on the QuickJS host).
    pub fn deadline(&self) -> Option<Duration> {
        self.engine.deadline()
    }

    /// Applies the input queued since the last call and runs due timers.
    /// Returns when to call again.
    pub fn pump(&mut self) -> Option<Duration> {
        self.engine.pump()
    }

    /// A click on view `id` (`onPress`). Applied at the next [`pump`](Self::pump).
    pub fn press(&self, id: NodeId) {
        self.engine
            .control()
            .dispatch_event(id, EventKind::Press, EventPayload::None);
    }

    /// A long press on view `id` (`onLongPress`). Applied at the next
    /// [`pump`](Self::pump).
    pub fn long_press(&self, id: NodeId) {
        self.engine
            .control()
            .dispatch_event(id, EventKind::LongPress, EventPayload::None);
    }

    /// The user typed `text` into input `id` (the whole new value).
    pub fn input(&self, id: NodeId, text: impl Into<String>) {
        self.engine.control().input_changed(id, text);
    }

    /// The selection of input `id`, in UTF-16 units.
    pub fn selection(&self, id: NodeId, start: u32, end: u32) {
        self.engine.control().selection_changed(id, start, end);
    }

    /// The composition of input `id` (`text` is empty once it ended).
    pub fn composition(&self, id: NodeId, composing: bool, text: impl Into<String>) {
        self.engine
            .control()
            .composition_changed(id, composing, text);
    }

    /// Scroll view `id` reported `kind`. The page already moved. The payload
    /// is the content offset, content size and visible size, in points.
    /// Kinds other than the six scroll events are ignored.
    #[allow(clippy::too_many_arguments)]
    pub fn scroll(
        &self,
        id: NodeId,
        kind: EventKind,
        x: f32,
        y: f32,
        content_w: f32,
        content_h: f32,
        width: f32,
        height: f32,
    ) {
        if !matches!(
            kind,
            EventKind::Scroll
                | EventKind::ScrollEnd
                | EventKind::ScrollDragStart
                | EventKind::ScrollDragEnd
                | EventKind::MomentumStart
                | EventKind::MomentumEnd
        ) {
            return;
        }
        self.engine.control().dispatch_event(
            id,
            kind,
            EventPayload::Scroll {
                x,
                y,
                content_w,
                content_h,
                width,
                height,
            },
        );
    }

    /// The browser's back button was pressed (popstate event). The browser has
    /// already navigated. Queues the back event for engine processing at the
    /// next pump: modals dismiss, stacks pop, then BackHandler listeners run.
    pub fn back(&self) {
        self.engine.control().back();
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

    /// The platform presents root `id` (a modal) at `size`, with `insets`
    /// of it covered by system UI.
    pub fn set_root_size(&self, id: NodeId, size: Size<Points>, insets: Edges<Points>) {
        self.engine.control().set_root_size(id, size, insets);
    }

    /// The platform measured where the content of screen `id` starts below
    /// its header (`None`: let the core estimate it).
    pub fn set_screen_header(&self, id: NodeId, height: Option<Points>) {
        self.engine.control().set_screen_header(id, height);
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

    fn request_frame(&self) -> bool {
        self.page.request_frame()
    }

    fn failed(&self, error: EngineError) {
        self.page.report_error(&error.message);
    }
}
