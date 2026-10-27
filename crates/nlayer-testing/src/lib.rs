//! Headless runs of TSX apps: a [`Screen`] models the native views a mount
//! batch would make, so tests assert the mount tree, its text and its
//! layout with no window. [`run`] is `nlayer test`: test files import
//! `@nlayer/test` and open the app on an engine of their own.
//!
//! ```ignore
//! let mut app = nlayer_testing::Headless::start(bundle, viewport)?;
//! assert!(app.wait_for("Count: 0", Duration::from_secs(1)));
//! assert!(app.press_text("Tap"));
//! ```

mod runner;

pub use runner::{CaseResult, FileResult, Report, RunError, run};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, unbounded};
use nlayer_core::{
    EventKind, EventMask, ImageSource, InputConfig, MountBatch, MountItem, NodeId, ResizeMode,
    ViewKind,
    geometry::{Bounds, Points, Size},
    style::ViewStyle,
    text::ApproxTextEngine,
};
use nlayer_engine::{Engine, EngineConfig, EngineError, EventPayload, MountSink};

/// One view of the headless screen, as the platform would hold it.
#[derive(Debug, Clone)]
pub struct View {
    /// What the view is.
    pub kind: ViewKind,
    /// Its parent (`None`: detached).
    pub parent: Option<NodeId>,
    /// Its children, in order.
    pub children: Vec<NodeId>,
    /// Its frame in its parent.
    pub frame: Bounds<Points>,
    /// A text view's content.
    pub text: Option<String>,
    /// The events it listens to.
    pub events: EventMask,
    /// Its style, as last mounted.
    pub style: Option<Box<ViewStyle>>,
    /// An image's source.
    pub source: Option<ImageSource>,
    /// How an image fits its box.
    pub resize_mode: ResizeMode,
    /// A single-line input's configuration and text.
    pub input: Option<InputView>,
    /// The scrollable size of a scroll view.
    pub scroll_content: Option<Size<Points>>,
}

/// A single-line input's configuration and text.
#[derive(Debug, Clone)]
pub struct InputView {
    /// Placeholder, keyboard, security and the text style.
    pub config: Box<InputConfig>,
    /// The text it shows (the app's `value`).
    pub value: String,
}

/// The model of a surface's views.
#[derive(Debug, Default)]
pub struct Screen {
    views: HashMap<NodeId, View>,
}

/// A node of [`Screen::tree`].
#[derive(Debug, Clone)]
pub struct TreeNode {
    /// The node's id.
    pub id: NodeId,
    /// What the view is.
    pub kind: ViewKind,
    /// Its frame in its parent.
    pub frame: Bounds<Points>,
    /// A text view's content.
    pub text: Option<String>,
    /// The events it listens to.
    pub events: EventMask,
    /// Its children, in order.
    pub children: Vec<TreeNode>,
}

impl Screen {
    /// The view `id`, if it exists.
    pub fn view(&self, id: NodeId) -> Option<&View> {
        self.views.get(&id)
    }

    /// Every view, by id.
    pub fn views(&self) -> impl Iterator<Item = (NodeId, &View)> {
        self.views.iter().map(|(&id, view)| (id, view))
    }

    /// The frame of view `id`.
    pub fn frame(&self, id: NodeId) -> Option<Bounds<Points>> {
        self.views.get(&id).map(|v| v.frame)
    }

    /// The text of every text view, in id order (a text block's whole
    /// content is one string).
    pub fn texts(&self) -> Vec<String> {
        let mut ids: Vec<(NodeId, &str)> = self
            .views
            .iter()
            .filter_map(|(&id, v)| v.text.as_deref().map(|t| (id, t)))
            .collect();
        ids.sort_by_key(|(id, _)| *id);
        ids.into_iter().map(|(_, t)| t.to_owned()).collect()
    }

    /// The texts of inputs, in id order.
    pub fn input_values(&self) -> Vec<String> {
        let mut ids: Vec<(NodeId, &str)> = self
            .views
            .iter()
            .filter_map(|(&id, v)| v.input.as_ref().map(|i| (id, i.value.as_str())))
            .collect();
        ids.sort_by_key(|(id, _)| *id);
        ids.into_iter().map(|(_, t)| t.to_owned()).collect()
    }

    /// The tree below `id` (`ROOT_ID` for the surface).
    pub fn tree(&self, id: NodeId) -> Option<TreeNode> {
        let view = self.views.get(&id)?;
        Some(TreeNode {
            id,
            kind: view.kind,
            frame: view.frame,
            text: view.text.clone(),
            events: view.events,
            children: view
                .children
                .iter()
                .filter_map(|child| self.tree(*child))
                .collect(),
        })
    }

    /// The nearest view at or above `id` that listens to `event`.
    pub fn listener(&self, mut id: NodeId, event: EventKind) -> Option<NodeId> {
        loop {
            let view = self.views.get(&id)?;
            if view.events.contains(event) {
                return Some(id);
            }
            id = view.parent?;
        }
    }

    /// The view showing `text` (a text view whose content contains it).
    pub fn find_text(&self, text: &str) -> Option<NodeId> {
        let mut ids: Vec<(NodeId, &str)> = self
            .views
            .iter()
            .filter_map(|(&id, v)| v.text.as_deref().map(|t| (id, t)))
            .filter(|(_, t)| t.contains(text))
            .collect();
        ids.sort_by_key(|(id, _)| *id);
        ids.first().map(|(id, _)| *id)
    }

    /// The parent view of `id`, if attached to one.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.views.get(&id)?.parent
    }

    /// Applies one batch.
    pub fn apply(&mut self, batch: &MountBatch) {
        for item in &batch.items {
            self.apply_item(item);
        }
    }

    fn apply_item(&mut self, item: &MountItem) {
        match item {
            MountItem::Create { id, kind } => {
                let view = self.views.entry(*id).or_insert_with(|| View {
                    kind: *kind,
                    parent: None,
                    children: Vec::new(),
                    frame: Bounds::default(),
                    text: None,
                    events: EventMask::default(),
                    style: None,
                    source: None,
                    resize_mode: ResizeMode::default(),
                    input: None,
                    scroll_content: None,
                });
                view.kind = *kind;
            }
            MountItem::Insert {
                parent,
                child,
                before,
            } => {
                self.ensure(*parent, ViewKind::View);
                self.ensure(*child, ViewKind::View);
                self.detach(*child);
                let children = &mut self.views.get_mut(parent).expect("a view").children;
                let at = match before {
                    Some(before) => children
                        .iter()
                        .position(|c| c == before)
                        .unwrap_or(children.len()),
                    None => children.len(),
                };
                children.insert(at, *child);
                self.views.get_mut(child).expect("a view").parent = Some(*parent);
            }
            MountItem::Remove { id } => self.detach(*id),
            MountItem::Delete { id } => self.delete(*id),
            MountItem::Frame { id, frame } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.frame = *frame;
                }
            }
            MountItem::ViewStyle { id, style } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.style = Some(style.clone());
                }
            }
            MountItem::Text { id, text, .. } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.text = Some(text.text.clone());
                }
            }
            MountItem::Events { id, events } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.events = *events;
                }
            }
            MountItem::Image {
                id,
                source,
                resize_mode,
            } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.source = source.clone();
                    view.resize_mode = *resize_mode;
                }
            }
            MountItem::ScrollConfig { .. } | MountItem::ScrollBy { .. } => {}
            MountItem::ScrollContent { id, size } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.scroll_content = Some(*size);
                }
            }
            MountItem::Input { id, config } => {
                self.ensure(*id, ViewKind::Input);
                if let Some(view) = self.views.get_mut(id) {
                    view.input = Some(InputView {
                        config: config.clone(),
                        value: view
                            .input
                            .as_ref()
                            .map_or_else(String::new, |i| i.value.clone()),
                    });
                }
            }
            MountItem::InputValue { id, text, .. } => {
                if let Some(view) = self.views.get_mut(id)
                    && let Some(input) = &mut view.input
                {
                    input.value = text.clone();
                }
            }
            MountItem::InputContent { .. } | MountItem::Command { .. } => {}
        }
    }

    fn ensure(&mut self, id: NodeId, kind: ViewKind) {
        self.views.entry(id).or_insert_with(|| View {
            kind,
            parent: None,
            children: Vec::new(),
            frame: Bounds::default(),
            text: None,
            events: EventMask::default(),
            style: None,
            source: None,
            resize_mode: ResizeMode::default(),
            input: None,
            scroll_content: None,
        });
    }

    fn detach(&mut self, id: NodeId) {
        let Some(parent) = self.views.get_mut(&id).and_then(|v| v.parent.take()) else {
            return;
        };
        if let Some(parent) = self.views.get_mut(&parent) {
            parent.children.retain(|c| *c != id);
        }
    }

    fn delete(&mut self, id: NodeId) {
        self.detach(id);
        let Some(view) = self.views.remove(&id) else {
            return;
        };
        for child in view.children {
            self.delete(child);
        }
    }
}

/// Hands batches to the screen and to the test waiting on them.
struct Sink {
    tx: crossbeam_channel::Sender<MountBatch>,
}

impl MountSink for Sink {
    fn submit(&self, batch: MountBatch) {
        let _ = self.tx.send(batch);
    }
}

/// An app running on an engine with no window.
pub struct Headless {
    engine: Engine,
    rx: Receiver<MountBatch>,
    screen: Screen,
}

impl Headless {
    /// Starts `bundle` on a headless engine of `viewport`, with the
    /// deterministic text measurer.
    pub fn start(
        bundle: impl Into<String>,
        viewport: Size<Points>,
    ) -> Result<Headless, EngineError> {
        Self::start_with(bundle, viewport, || ApproxTextEngine)
    }

    /// Starts `bundle` with text engine `text_engine`.
    pub fn start_with<M, F>(
        bundle: impl Into<String>,
        viewport: Size<Points>,
        text_engine: F,
    ) -> Result<Headless, EngineError>
    where
        M: nlayer_core::text::TextEngine + 'static,
        F: FnOnce() -> M + Send + 'static,
    {
        let (tx, rx) = unbounded();
        let engine = Engine::start(
            EngineConfig::new(bundle, viewport),
            text_engine,
            Arc::new(Sink { tx }),
        )?;
        let mut app = Headless {
            engine,
            rx,
            screen: Screen::default(),
        };
        app.refresh();
        Ok(app)
    }

    /// The engine's side (events, resizes, input).
    pub fn control(&self) -> nlayer_engine::EngineControl {
        self.engine.control()
    }

    /// Applies the frames that arrived, without waiting: how many.
    pub fn refresh(&mut self) -> usize {
        let mut applied = 0;
        while let Ok(batch) = self.rx.try_recv() {
            self.screen.apply(&batch);
            applied += 1;
        }
        applied
    }

    /// Waits up to `timeout` for a frame, applying what arrives.
    pub fn next_frame(&mut self, timeout: Duration) -> bool {
        match self.rx.recv_timeout(timeout) {
            Ok(batch) => {
                self.screen.apply(&batch);
                self.refresh();
                true
            }
            Err(_) => false,
        }
    }

    /// The screen, after applying frames that arrived.
    pub fn screen(&mut self) -> &Screen {
        self.refresh();
        &self.screen
    }

    /// Every text view's content.
    pub fn texts(&mut self) -> Vec<String> {
        self.screen().texts()
    }

    /// Waits until some text view shows `text`.
    pub fn wait_for(&mut self, text: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            self.refresh();
            if self.screen.texts().iter().any(|t| t.contains(text)) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || !self.next_frame(left) {
                return self.screen.texts().iter().any(|t| t.contains(text));
            }
        }
    }

    /// Calls node `id`'s press handler.
    pub fn press(&mut self, id: NodeId) {
        self.engine
            .control()
            .dispatch_event(id, EventKind::Press, EventPayload::None);
    }

    /// Presses the view showing `text` (its nearest parent listening to
    /// presses): whether one was found.
    pub fn press_text(&mut self, text: &str) -> bool {
        self.refresh();
        let Some(id) = self.screen.find_text(text) else {
            return false;
        };
        let Some(listener) = self.screen.listener(id, EventKind::Press) else {
            return false;
        };
        self.press(listener);
        true
    }

    /// The user typed `text` into input `id`. The native field shows it at
    /// once (as a platform's does); the app's `onChange` decides what the
    /// controlled value becomes.
    pub fn type_text(&mut self, id: NodeId, text: impl Into<String>) {
        let text = text.into();
        self.refresh();
        if let Some(view) = self.screen.views.get_mut(&id)
            && let Some(input) = &mut view.input
        {
            input.value = text.clone();
        }
        self.engine.control().input_changed(id, text);
    }

    /// The surface changed size.
    pub fn resize(&mut self, size: Size<Points>) {
        self.engine.control().resize(size);
    }

    /// Stops the engine and waits for it.
    pub fn stop(self) {
        self.engine.stop();
    }
}
