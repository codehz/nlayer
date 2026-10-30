//! Headless runs of TSX apps: a [`Screen`] models the native views a mount
//! batch would make, so tests assert the mount tree, its text and its
//! layout with no window. [`run`] is `nlayer test`: test files import
//! `@nlayer/test` and open the app on an engine of their own. An app entry
//! instead of a test file is opened, then the steps from standard input run.
//! [`run_with`] opens that app on another host instead, still through the
//! same steps.
//!
//! ```ignore
//! let mut app = nlayer_testing::Headless::start(bundle, viewport)?;
//! assert!(app.wait_for("Count: 0", Duration::from_secs(1)));
//! assert!(app.press_text("Tap"));
//! ```

mod runner;
mod selector;

pub use runner::{
    AppHost, CaseResult, FileResult, HostInsets, HostOpen, HostedApp, Report, RunError,
    fixture_file_port, run, run_with, with_fixture_origin,
};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, unbounded};
use nlayer_core::{
    Accessibility, Command, EventKind, EventMask, ImageSource, InputConfig, MountBatch, MountItem,
    NodeId, ResizeMode, ViewKind,
    geometry::{Bounds, Points, Size},
    style::ViewStyle,
    text::{ApproxTextEngine, SpanTestId, TextLayout, TextLink},
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
    /// `true` when the scroll view scrolls horizontally.
    pub scroll_horizontal: Option<bool>,
    /// Style runs of a text view, covering [`Self::text`].
    pub spans: Vec<TextRun>,
    /// `testID`. The screen keeps it; nothing is drawn from it.
    pub test_id: Option<Arc<str>>,
    /// `role`, label, hint, state, value and whether the subtree is hidden
    /// from assistive technologies.
    pub accessibility: Accessibility,
    /// Nested spans named by `testID`, on the text view that draws them.
    pub span_ids: Vec<SpanTestId>,
    /// Nested spans that take presses, on the text view that draws them.
    pub links: Vec<TextLink>,
    /// Content offset of a scroll view, toward the end of the content.
    pub scroll_x: f32,
    /// Content offset of a scroll view.
    pub scroll_y: f32,
    /// Where a text view's layout sits inside the view.
    pub text_content: Bounds<Points>,
    /// The text view's layout, so a span has a box.
    pub text_layout: Option<TextLayout>,
}

/// A single-line input's configuration and text.
#[derive(Debug, Clone)]
pub struct InputView {
    /// Placeholder, keyboard, security and the text style.
    pub config: Box<InputConfig>,
    /// The text it shows (the app's `value`).
    pub value: String,
    /// The field is the one editing. `Keyboard.dismiss` clears it.
    pub focused: bool,
    /// The controlled selection (`selectedRange`), in UTF-16 units.
    pub selection: Option<(u32, u32)>,
}

/// One style run of a text view.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// The run's characters.
    pub text: String,
    /// Font size, in points.
    pub font_size: f32,
    /// Font weight, `100..=900`.
    pub font_weight: u16,
    /// Italic.
    pub italic: bool,
    /// Packed `0xRRGGBBAA`.
    pub color: u32,
}

/// The model of a surface's views.
#[derive(Debug, Default)]
pub struct Screen {
    views: HashMap<NodeId, View>,
    /// The viewport `open` pinned. Visibility is judged against it.
    viewport: Size<Points>,
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

    /// The viewport the screen was opened at.
    pub fn viewport(&self) -> Size<Points> {
        self.viewport
    }

    /// Pins the viewport selectors judge visibility against.
    pub fn set_viewport(&mut self, viewport: Size<Points>) {
        self.viewport = viewport;
    }

    /// Sets the content offset of scroll view `id`.
    pub fn set_scroll(&mut self, id: NodeId, x: f32, y: f32) {
        if let Some(view) = self.views.get_mut(&id) {
            view.scroll_x = x;
            view.scroll_y = y;
        }
    }

    /// `Keyboard.dismiss`: no field is editing.
    pub fn blur(&mut self) {
        for view in self.views.values_mut() {
            if let Some(input) = &mut view.input {
                input.focused = false;
            }
        }
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

    /// The view a press on `id` acts on: the nearest listener, unless a
    /// disabled view comes first (the press is blocked, not passed on).
    pub fn press_listener(&self, mut id: NodeId) -> Option<NodeId> {
        loop {
            let view = self.views.get(&id)?;
            if view.accessibility.blocks_press() {
                return None;
            }
            if view.events.contains(EventKind::Press) {
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
                    scroll_horizontal: None,
                    spans: Vec::new(),
                    test_id: None,
                    accessibility: Accessibility::default(),
                    span_ids: Vec::new(),
                    links: Vec::new(),
                    scroll_x: 0.0,
                    scroll_y: 0.0,
                    text_content: Bounds::default(),
                    text_layout: None,
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
            MountItem::Text {
                id,
                text,
                layout,
                content,
            } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.spans = text_runs(text);
                    view.text = Some(text.text.clone());
                    view.span_ids = text.test_ids.clone();
                    view.links = text.links.clone();
                    view.text_content = *content;
                    view.text_layout = Some(layout.clone());
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
            MountItem::ScrollConfig { id, horizontal, .. } => {
                self.ensure(*id, ViewKind::Scroll);
                if let Some(view) = self.views.get_mut(id) {
                    view.scroll_horizontal = Some(*horizontal);
                }
            }
            MountItem::ScrollBy { id, dx, dy } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.scroll_x += dx.0;
                    view.scroll_y += dy.0;
                }
            }
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
                        focused: view.input.as_ref().is_some_and(|i| i.focused),
                        selection: view.input.as_ref().and_then(|i| i.selection),
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
            MountItem::InputContent { .. } | MountItem::StateStyle { .. } => {}
            MountItem::Command { id, command } => match command {
                Command::ScrollTo { x, y, .. } => self.set_scroll(*id, x.0, y.0),
                Command::Blur => self.blur(),
                Command::SetSelection { start, end } => {
                    if let Some(view) = self.views.get_mut(id)
                        && let Some(input) = &mut view.input
                    {
                        input.selection = Some((*start, *end));
                    }
                }
            },
            MountItem::TestId { id, test_id } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.test_id = test_id.clone();
                }
            }
            MountItem::Accessibility { id, accessibility } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.accessibility = (**accessibility).clone();
                }
            }
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
            scroll_horizontal: None,
            spans: Vec::new(),
            test_id: None,
            accessibility: Accessibility::default(),
            span_ids: Vec::new(),
            links: Vec::new(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            text_content: Bounds::default(),
            text_layout: None,
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
        Self::start_with_config(EngineConfig::new(bundle, viewport), text_engine)
    }

    /// Starts the app `config` describes, with text engine `text_engine`.
    pub fn start_with_config<M, F>(
        config: EngineConfig,
        text_engine: F,
    ) -> Result<Headless, EngineError>
    where
        M: nlayer_core::text::TextEngine + 'static,
        F: FnOnce() -> M + Send + 'static,
    {
        let viewport = config.viewport;
        let (tx, rx) = unbounded();
        let engine = Engine::start(config, text_engine, Arc::new(Sink { tx }))?;
        let mut app = Headless {
            engine,
            rx,
            screen: Screen {
                viewport,
                ..Screen::default()
            },
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
        let Some(listener) = self.screen.press_listener(id) else {
            return false;
        };
        self.press(listener);
        true
    }

    /// Presses what `selector` names. A match without `onPress` presses the
    /// nearest ancestor that has it. The match has to be on screen.
    pub fn press_selector(&mut self, selector: &str) -> Result<(), String> {
        self.refresh();
        let target = selector::resolve(&self.screen, selector)?;
        self.require_visible(selector, target)?;
        let id = match target {
            selector::Target::Span { node: Some(id), .. } => id,
            _ => self
                .screen
                .press_listener(target.view())
                .ok_or_else(|| format!("`{selector}` does not take a press"))?,
        };
        self.press(id);
        Ok(())
    }

    /// Types `text` into the single-line input `selector` names. Focuses
    /// that field first (and no other), then appends one change per
    /// character to the value it already shows.
    pub fn type_selector(&mut self, selector: &str, text: &str) -> Result<(), String> {
        self.refresh();
        let target = selector::resolve(&self.screen, selector)?;
        self.require_visible(selector, target)?;
        let id = selector::input_of(&self.screen, target)
            .ok_or_else(|| format!("`{selector}` is not an input"))?;
        if self
            .screen
            .view(id)
            .and_then(|view| view.input.as_ref())
            .is_some_and(|input| input.config.disabled)
        {
            return Err(format!("`{selector}` is disabled"));
        }
        self.focus_input(id);
        let mut value = self
            .screen
            .view(id)
            .and_then(|view| view.input.as_ref())
            .map(|input| input.value.clone())
            .unwrap_or_default();
        for ch in text.chars() {
            value.push(ch);
            if let Some(view) = self.screen.views.get_mut(&id)
                && let Some(input) = &mut view.input
            {
                input.value = value.clone();
            }
            self.engine.control().input_changed(id, value.clone());
            let _ = self.next_frame(Duration::from_millis(200));
            if let Some(now) = self
                .screen
                .view(id)
                .and_then(|view| view.input.as_ref())
                .map(|input| input.value.clone())
            {
                value = now;
            }
        }
        Ok(())
    }

    /// Moves the selection of the input `selector` names, as the platform
    /// reports it (`onSelectionChange`).
    pub fn select_selector(&mut self, selector: &str, start: u32, end: u32) -> Result<(), String> {
        self.refresh();
        let target = selector::resolve(&self.screen, selector)?;
        self.require_visible(selector, target)?;
        let id = selector::input_of(&self.screen, target)
            .ok_or_else(|| format!("`{selector}` is not an input"))?;
        self.engine.control().selection_changed(id, start, end);
        let _ = self.next_frame(Duration::from_millis(200));
        Ok(())
    }

    /// Reports a change of the input `selector` names' composition, as an
    /// input method does (`onCompositionChange`).
    pub fn compose_selector(
        &mut self,
        selector: &str,
        text: &str,
        composing: bool,
    ) -> Result<(), String> {
        self.refresh();
        let target = selector::resolve(&self.screen, selector)?;
        self.require_visible(selector, target)?;
        let id = selector::input_of(&self.screen, target)
            .ok_or_else(|| format!("`{selector}` is not an input"))?;
        self.engine
            .control()
            .composition_changed(id, composing, text);
        let _ = self.next_frame(Duration::from_millis(200));
        Ok(())
    }

    /// Scrolls the scroll view `selector` names (or the one around it) by
    /// `distance` points toward the end of its content.
    pub fn scroll_selector(&mut self, selector: &str, distance: f32) -> Result<(), String> {
        self.refresh();
        let target = selector::resolve(&self.screen, selector)?;
        self.require_visible(selector, target)?;
        let id = selector::scroll_of(&self.screen, target)
            .ok_or_else(|| format!("`{selector}` is not in a scroll view"))?;
        selector::scroll_by(&mut self.screen, id, distance)
            .map_err(|_| format!("`{selector}` is not in a scroll view"))?;
        let view = self
            .screen
            .view(id)
            .ok_or_else(|| format!("`{selector}` is not in a scroll view"))?;
        let content = view.scroll_content.unwrap_or(view.frame.size);
        let payload = EventPayload::Scroll {
            x: view.scroll_x,
            y: view.scroll_y,
            content_w: content.width.0,
            content_h: content.height.0,
            width: view.frame.size.width.0,
            height: view.frame.size.height.0,
        };
        // A test scroll is a finished drag that glides to this offset, so
        // the six callbacks run with the offset the tree reports. The
        // engine applies one event per frame; `waitFor` pumps them.
        for event in [
            EventKind::ScrollDragStart,
            EventKind::Scroll,
            EventKind::ScrollDragEnd,
            EventKind::MomentumStart,
            EventKind::MomentumEnd,
            EventKind::ScrollEnd,
        ] {
            self.engine
                .control()
                .dispatch_event(id, event, payload.clone());
        }
        Ok(())
    }

    /// Waits until `selector` names one view that is on screen.
    pub fn wait_selector(&mut self, selector: &str, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.selector_state(selector) {
                SelectorState::Ready => return Ok(()),
                SelectorState::Wait => {}
                SelectorState::Fail(error) => return Err(error),
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || !self.next_frame(left) {
                break;
            }
        }
        match self.selector_state(selector) {
            SelectorState::Ready => Ok(()),
            SelectorState::Wait => Err(format!("`{selector}` matches nothing")),
            SelectorState::Fail(error) => Err(error),
        }
    }

    fn require_visible(&self, selector: &str, target: selector::Target) -> Result<(), String> {
        if selector::visible(&self.screen, target) {
            Ok(())
        } else {
            Err(format!("`{selector}` is not in view"))
        }
    }

    fn selector_state(&mut self, selector: &str) -> SelectorState {
        self.refresh();
        match selector::resolve(&self.screen, selector) {
            Ok(target) if selector::visible(&self.screen, target) => SelectorState::Ready,
            Ok(_) => SelectorState::Wait,
            Err(error) if error.contains("matches nothing") => SelectorState::Wait,
            Err(error) => SelectorState::Fail(error),
        }
    }

    /// The user typed `text` into input `id`. The field is the one editing.
    /// The native field shows the text at once (as a platform's does); the
    /// app's `onChange` decides what the controlled value becomes.
    pub fn type_text(&mut self, id: NodeId, text: impl Into<String>) {
        let text = text.into();
        self.refresh();
        self.focus_input(id);
        if let Some(view) = self.screen.views.get_mut(&id)
            && let Some(input) = &mut view.input
        {
            input.value = text.clone();
        }
        self.engine.control().input_changed(id, text);
    }

    /// `id` is the field being edited. Every other field is not.
    fn focus_input(&mut self, id: NodeId) {
        for (other, view) in self.screen.views.iter_mut() {
            if let Some(input) = &mut view.input {
                input.focused = *other == id;
            }
        }
    }

    /// The surface changed size.
    pub fn resize(&mut self, size: Size<Points>) {
        self.screen.set_viewport(size);
        self.engine.control().resize(size);
    }

    /// Stops the engine and waits for it.
    pub fn stop(self) {
        self.engine.stop();
    }
}

enum SelectorState {
    Ready,
    Wait,
    Fail(String),
}

fn text_runs(text: &nlayer_core::text::AttributedText) -> Vec<TextRun> {
    text.spans
        .iter()
        .map(|span| TextRun {
            text: utf16_slice(&text.text, span.start, span.end),
            font_size: span.style.font_size.0,
            font_weight: span.style.font_weight,
            italic: span.style.italic,
            color: span.style.color.0,
        })
        .collect()
}

fn utf16_slice(text: &str, start: u32, end: u32) -> String {
    let units: Vec<u16> = text
        .encode_utf16()
        .skip(start as usize)
        .take(end.saturating_sub(start) as usize)
        .collect();
    String::from_utf16_lossy(&units)
}
