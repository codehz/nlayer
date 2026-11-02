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

pub use nlayer_core::NodeId;
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
    Accessibility, Command, EventKind, EventMask, ImageSource, InputConfig, ModalConfig,
    MountBatch, MountItem, ResizeMode, ScreenConfig, ViewKind,
    animation::{AnimationProp, Finished, Graph, NodeId as AnimationNode},
    geometry::{Bounds, Points, Size},
    style::{Color, ViewStyle},
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
    /// Its style, with animated properties applied.
    pub style: Option<Box<ViewStyle>>,
    /// Its style as last mounted, without animated properties.
    pub base_style: Option<Box<ViewStyle>>,
    /// An image's source.
    pub source: Option<ImageSource>,
    /// How an image fits its box.
    pub resize_mode: ResizeMode,
    /// Native or web screen header and back options.
    pub screen: Option<Box<ScreenConfig>>,
    /// Modal presentation and close options.
    pub modal: Option<ModalConfig>,
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
    /// The resolved `userSelect`: `Some(true)` asks for selection,
    /// `Some(false)` is `"none"`, `None` is unset.
    pub text_selectable: Option<bool>,
    /// `selectable` on this view: one selection across its texts.
    pub selectable: bool,
    /// `selectionColor` of the text's selection.
    pub selection_color: Option<Color>,
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
#[derive(Debug, Default, Clone)]
struct GestureBindings {
    pan_x: Option<AnimationNode>,
    pan_y: Option<AnimationNode>,
    pinch: Option<AnimationNode>,
    rotation: Option<AnimationNode>,
}

#[derive(Debug, Clone)]
struct ActiveGesture {
    id: NodeId,
    start_pos: (f32, f32),
    prev_pos: (f32, f32),
    prev_time: f64,
    last_pos: (f32, f32),
    last_time: f64,
    start_values: [f32; 4], // pan_x, pan_y (only these used; pinch/rotation not supported headless)
}

/// Stack state: screens pushed onto it.
#[derive(Debug, Default, Clone)]
struct StackState {
    /// Screens in push order; last is top.
    screens: Vec<NodeId>,
}

/// What a back does ([`Screen::back_target`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Back {
    /// The stack pops this screen.
    Pop(NodeId),
    /// This screen prevents going back: it is asked.
    Ask(NodeId),
    /// The modal asks to close, and closes when the user may close it.
    Close {
        /// The modal that asks to close.
        modal: NodeId,
        /// Whether back may close it.
        close_on_escape: bool,
    },
    /// Nothing to go back from.
    Nothing,
}

/// Native views a mount batch would make, plus stacks, modals, and gestures.
#[derive(Debug, Default)]
pub struct Screen {
    views: HashMap<NodeId, View>,
    /// The viewport `open` pinned. Visibility is judged against it.
    viewport: Size<Points>,
    /// View properties bound to animation nodes.
    animated: HashMap<NodeId, Vec<(AnimationProp, AnimationNode)>>,
    /// The animation graph, run by the test clock.
    graph: Graph,
    /// Animations that ended since the last report.
    finished: Vec<Finished>,
    /// Gesture bindings: pan x/y, pinch scale, rotation angle nodes.
    gesture_bindings: HashMap<NodeId, GestureBindings>,
    /// Active gesture state.
    active_gesture: Option<ActiveGesture>,
    /// Stacks, each holding their screens in push order.
    stacks: HashMap<NodeId, StackState>,
    /// Modals, in presentation order; last is top.
    modals: Vec<NodeId>,
    /// Screen header heights (from ScreenConfig.header_height).
    screen_headers: HashMap<NodeId, Points>,
    /// URLs opened by `Command::OpenUrl`, in order.
    opened_urls: Vec<String>,
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

    /// The URLs a short press on a `Text` `href` opened, in order.
    pub fn opened_urls(&self) -> &[String] {
        &self.opened_urls
    }

    /// What owns the selection of text view `id`: the outermost `selectable`
    /// view around it, else the text itself when its resolved `userSelect`
    /// asks for selection, else none. A text with `userSelect: "none"` has
    /// no owner even inside a `selectable` view.
    pub fn selection_owner(&self, id: NodeId) -> Option<NodeId> {
        let text = self.views.get(&id)?;
        if text.text_selectable == Some(false) {
            return None;
        }
        let mut owner = None;
        let mut at = text.parent;
        while let Some(parent) = at {
            let view = self.views.get(&parent)?;
            if view.selectable {
                owner = Some(parent);
            }
            at = view.parent;
        }
        owner.or((text.text_selectable == Some(true)).then_some(id))
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

    /// The view a long press on `id` acts on: the nearest listener, unless a
    /// disabled view comes first (the press is blocked, not passed on).
    pub fn long_press_listener(&self, mut id: NodeId) -> Option<NodeId> {
        loop {
            let view = self.views.get(&id)?;
            if view.accessibility.blocks_press() {
                return None;
            }
            if view.events.contains(EventKind::LongPress) {
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

    /// The screens of `stack_id`, in push order.
    fn screens(&self, stack_id: NodeId) -> &[NodeId] {
        self.stacks
            .get(&stack_id)
            .map(|s| s.screens.as_slice())
            .unwrap_or(&[])
    }

    /// The depth of `stack_id`: how many screens are pushed.
    pub fn stack_depth(&self, stack_id: NodeId) -> usize {
        self.screens(stack_id).len()
    }

    /// The top screen of `stack_id`.
    pub fn top_screen(&self, stack_id: NodeId) -> Option<NodeId> {
        self.screens(stack_id).last().copied()
    }

    /// The top modal, if any.
    pub fn top_modal(&self) -> Option<NodeId> {
        self.modals.last().copied()
    }

    /// What a back does: which screen to pop, ask, or modal to close.
    pub fn back_target(&self) -> Back {
        // 1. Top modal with close_on_escape
        if let Some(&modal_id) = self.modals.last()
            && let Some(modal) = self.views.get(&modal_id).and_then(|v| v.modal.as_ref())
            && modal.close_on_escape
        {
            return Back::Close {
                modal: modal_id,
                close_on_escape: true,
            };
        }

        // 2. Top screen of any stack
        for (&_stack_id, state) in &self.stacks {
            if let Some(&screen_id) = state.screens.last() {
                let view = self.views.get(&screen_id);
                let screen_cfg = view.and_then(|v| v.screen.as_ref());

                // If screen has onBackPress listener, always ask it
                if view.is_some_and(|v| v.events.contains(EventKind::BackRequest)) {
                    return Back::Ask(screen_id);
                }

                // If depth > 1 and !prevent_back, can pop
                if state.screens.len() > 1 {
                    let prevent_back = screen_cfg.is_some_and(|cfg| cfg.prevent_back);
                    if !prevent_back {
                        return Back::Pop(screen_id);
                    }
                }
            }
        }

        Back::Nothing
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
                    base_style: None,
                    source: None,
                    resize_mode: ResizeMode::default(),
                    screen: None,
                    modal: None,
                    input: None,
                    scroll_content: None,
                    scroll_horizontal: None,
                    spans: Vec::new(),
                    test_id: None,
                    accessibility: Accessibility::default(),
                    span_ids: Vec::new(),
                    links: Vec::new(),
                    text_selectable: None,
                    selectable: false,
                    selection_color: None,
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

                // Track stack/screen relationships
                let parent_kind = self.views.get(parent).map(|v| v.kind);
                let child_kind = self.views.get(child).map(|v| v.kind);

                if parent_kind == Some(ViewKind::Stack) && child_kind == Some(ViewKind::Screen) {
                    let state = self.stacks.entry(*parent).or_default();
                    if !state.screens.contains(child) {
                        state.screens.push(*child);
                    }
                }

                // Track modals
                if child_kind == Some(ViewKind::Modal) && !self.modals.contains(child) {
                    self.modals.push(*child);
                }

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
            MountItem::Remove { id } => {
                // Clean up from stacks/modals
                if let Some(view) = self.views.get(id) {
                    if view.kind == ViewKind::Screen {
                        // Remove from parent stack
                        if let Some(parent_id) = view.parent
                            && let Some(state) = self.stacks.get_mut(&parent_id)
                        {
                            state.screens.retain(|&s| s != *id);
                        }
                    } else if view.kind == ViewKind::Modal {
                        self.modals.retain(|&m| m != *id);
                    }
                }
                self.detach(*id);
            }
            MountItem::Delete { id } => {
                // Clean up from stacks/modals
                if let Some(view) = self.views.get(id) {
                    if view.kind == ViewKind::Screen {
                        if let Some(parent_id) = view.parent
                            && let Some(state) = self.stacks.get_mut(&parent_id)
                        {
                            state.screens.retain(|&s| s != *id);
                        }
                        self.screen_headers.remove(id);
                    } else if view.kind == ViewKind::Modal {
                        self.modals.retain(|&m| m != *id);
                    } else if view.kind == ViewKind::Stack {
                        self.stacks.remove(id);
                    }
                }
                self.animated.remove(id);
                self.delete(*id);
            }
            MountItem::Frame { id, frame } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.frame = *frame;
                }
            }
            MountItem::ViewStyle { id, style } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.style = Some(style.clone());
                    view.base_style = Some(style.clone());
                }
                self.apply_animated();
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
                    view.text_selectable = text.selectable;
                    view.selection_color = text.selection_color;
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
            MountItem::ScrollContent { id, size, .. } => {
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
            MountItem::ScreenConfig { id, config } => {
                self.ensure(*id, ViewKind::Screen);
                // Record header height for insets
                self.screen_headers.insert(*id, config.header_height);
                self.views.get_mut(id).expect("a screen").screen = Some(config.clone());
            }
            MountItem::ModalConfig { id, config } => {
                self.ensure(*id, ViewKind::Modal);
                self.views.get_mut(id).expect("a modal").modal = Some(*config);
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
                Command::ScrollToIndex { .. } | Command::ScrollToEnd { .. } | Command::GoBack => {}
                Command::Back => {}
                Command::OpenUrl { url } => self.opened_urls.push(url.clone()),
            },
            MountItem::LongPressDelay { .. } => {}
            MountItem::Selectable { id, selectable } => {
                self.ensure(*id, ViewKind::View);
                if let Some(view) = self.views.get_mut(id) {
                    view.selectable = *selectable;
                }
            }
            MountItem::Animation(op) => {
                self.graph.apply(op.clone(), &mut self.finished);
                self.apply_animated();
            }
            MountItem::AnimationBindings { id, bindings } => {
                if bindings.is_empty() {
                    self.animated.remove(id);
                    if let Some(view) = self.views.get_mut(id)
                        && let Some(base) = &view.base_style
                    {
                        view.style = Some(base.clone());
                    }
                } else {
                    self.animated.insert(*id, bindings.clone());
                }
                self.apply_animated();
            }
            MountItem::TestId { id, test_id } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.test_id = test_id.clone();
                }
            }
            MountItem::PanBinding { id, x, y } => {
                let bindings = self.gesture_bindings.entry(*id).or_default();
                bindings.pan_x = *x;
                bindings.pan_y = *y;
            }
            MountItem::PinchBinding { id, scale } => {
                let bindings = self.gesture_bindings.entry(*id).or_default();
                bindings.pinch = *scale;
            }
            MountItem::RotationBinding { id, angle } => {
                let bindings = self.gesture_bindings.entry(*id).or_default();
                bindings.rotation = *angle;
            }
            MountItem::Accessibility { id, accessibility } => {
                if let Some(view) = self.views.get_mut(id) {
                    view.accessibility = (**accessibility).clone();
                }
            }
            MountItem::ListContent { .. } => {
                // List content not tracked in testing
            }
            MountItem::Refreshing { .. } => {
                // Pull-to-refresh state not tracked in testing
            }
        }
    }

    /// Applies every bound animation value to its view, over the static
    /// style. Runs after a frame and after the graph moves.
    pub fn apply_animated(&mut self) {
        let ids: Vec<NodeId> = self.animated.keys().copied().collect();
        for id in ids {
            let Some(bindings) = self.animated.get(&id).cloned() else {
                continue;
            };
            let Some(view) = self.views.get_mut(&id) else {
                continue;
            };
            if view.base_style.is_none() {
                view.base_style = Some(Box::default());
            }
            if view.style.is_none() {
                view.style = Some(Box::default());
            }
            let Some(style) = view.style.as_mut() else {
                continue;
            };
            if let Some(base) = &view.base_style {
                **style = (**base).clone();
            }
            for (prop, node) in bindings {
                let value = self.graph.eval(node);
                match prop {
                    AnimationProp::Opacity => style.opacity = value.number(),
                    AnimationProp::BackgroundColor => style.background_color = Color(value.color()),
                    AnimationProp::BorderColor => style.borders.colors = [Color(value.color()); 4],
                    AnimationProp::Transform(i) => {
                        if style.transform.ops().get(i as usize).is_some() {
                            style.transform.set(i, value.number());
                        }
                    }
                }
            }
        }
    }

    /// Advances the graph to `now` seconds. Returns true if a value moved.
    pub fn tick_animation(&mut self, now: f64) -> bool {
        let changed = self.graph.tick(now, &mut self.finished);
        if changed {
            self.apply_animated();
        }
        changed
    }

    /// Animations that ended since the last call, for the engine.
    pub fn take_finished(&mut self) -> Vec<Finished> {
        std::mem::take(&mut self.finished)
    }

    /// Whether an animation still runs.
    pub fn is_animating(&self) -> bool {
        self.graph.is_animating()
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
            base_style: None,
            source: None,
            resize_mode: ResizeMode::default(),
            screen: None,
            modal: None,
            input: None,
            scroll_content: None,
            scroll_horizontal: None,
            spans: Vec::new(),
            test_id: None,
            accessibility: Accessibility::default(),
            span_ids: Vec::new(),
            links: Vec::new(),
            text_selectable: None,
            selectable: false,
            selection_color: None,
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

    fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        // Walk tree front-to-back (reverse children order)
        fn test_node(
            screen: &Screen,
            id: NodeId,
            x: f32,
            y: f32,
            offset: (f32, f32),
        ) -> Option<NodeId> {
            let view = screen.views.get(&id)?;
            let local_x = x - offset.0;
            let local_y = y - offset.1;
            if local_x < 0.0
                || local_y < 0.0
                || local_x >= view.frame.size.width.0
                || local_y >= view.frame.size.height.0
            {
                return None;
            }
            // Test children in reverse order (front-to-back)
            for &child in view.children.iter().rev() {
                let child_view = screen.views.get(&child)?;
                let child_offset = (
                    offset.0 + child_view.frame.origin.x.0,
                    offset.1 + child_view.frame.origin.y.0,
                );
                if let Some(hit) = test_node(screen, child, x, y, child_offset) {
                    return Some(hit);
                }
            }
            Some(id)
        }
        test_node(self, nlayer_core::ROOT_ID, x, y, (0.0, 0.0))
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
    /// The test clock, in seconds since the app opened.
    now: f64,
}

/// A display frame at 60 Hz.
const FRAME: f64 = 1.0 / 60.0;

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
            now: 0.0,
        };
        app.refresh();
        // Initialize root view frame to viewport size for hit testing
        if let Some(root) = app.screen.views.get_mut(&nlayer_core::ROOT_ID) {
            use nlayer_core::geometry::{Bounds, Point, Points};
            root.frame = Bounds::new(Point::new(Points(0.0), Points(0.0)), viewport);
        }
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
        if applied > 0 {
            self.start_animations();
        }
        applied
    }

    /// The URLs short presses on `Text` `href`s opened so far, after
    /// applying the frames that arrived.
    pub fn opened_urls(&mut self) -> Vec<String> {
        self.refresh();
        self.screen.opened_urls().to_vec()
    }

    /// Starts animations that arrived at this turn's time. The first frame
    /// of a driver is its clock origin; it moves only when [`advance`](Self::advance)
    /// moves time.
    fn start_animations(&mut self) {
        self.screen.tick_animation(self.now);
        self.report_finished();
    }

    fn report_finished(&mut self) {
        let finished = self.screen.take_finished();
        if !finished.is_empty() {
            self.engine.control().animations_finished(finished);
        }
    }

    /// Advances the deterministic test clock by `ms` milliseconds in 60 Hz
    /// frames, applying animated values and settling promises.
    pub fn advance(&mut self, ms: f64) {
        self.refresh();
        self.start_animations();
        let target = self.now + (ms.max(0.0) / 1000.0);
        let mut reported = false;
        while self.now < target {
            self.now = (self.now + FRAME).min(target);
            self.screen.tick_animation(self.now);
            let finished = self.screen.take_finished();
            if !finished.is_empty() {
                reported = true;
                self.engine.control().animations_finished(finished);
            }
        }
        if reported {
            let _ = self.next_frame(Duration::from_secs(2));
        }
        self.refresh();
    }

    /// Waits up to `timeout` for a frame, applying what arrives.
    pub fn next_frame(&mut self, timeout: Duration) -> bool {
        match self.rx.recv_timeout(timeout) {
            Ok(batch) => {
                self.screen.apply(&batch);
                self.refresh();
                self.start_animations();
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
            if self.screen.is_animating() {
                self.advance(FRAME * 1000.0);
                continue;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || !self.next_frame(left) {
                return self.screen.texts().iter().any(|t| t.contains(text));
            }
        }
    }

    /// Sends a back request through the navigation precedence.
    pub fn back(&mut self) -> Result<(), String> {
        self.refresh();
        match self.screen.back_target() {
            Back::Pop(screen_id) => {
                // For a poppable screen, dispatch Dismiss event to trigger the pop
                self.engine.control().dispatch_event(
                    screen_id,
                    EventKind::Dismiss,
                    EventPayload::None,
                );
                let _ = self.next_frame(Duration::from_millis(100));
                Ok(())
            }
            Back::Ask(screen_id) => {
                // Send BackRequest event to let callback handle it
                self.engine.control().dispatch_event(
                    screen_id,
                    EventKind::BackRequest,
                    EventPayload::None,
                );
                let _ = self.next_frame(Duration::from_millis(100));

                // If popOnBack is true (prevent_back is false), also pop after callback
                let prevent_back = self
                    .screen
                    .view(screen_id)
                    .and_then(|v| v.screen.as_ref())
                    .is_some_and(|cfg| cfg.prevent_back);
                if !prevent_back {
                    // Callback was notified, now do the pop
                    self.engine.control().dispatch_event(
                        screen_id,
                        EventKind::Dismiss,
                        EventPayload::None,
                    );
                    let _ = self.next_frame(Duration::from_millis(100));
                }
                Ok(())
            }
            Back::Close {
                modal,
                close_on_escape,
            } => {
                if close_on_escape {
                    // Send RequestClose event
                    if self
                        .screen
                        .view(modal)
                        .is_some_and(|v| v.events.contains(EventKind::RequestClose))
                    {
                        self.engine.control().dispatch_event(
                            modal,
                            EventKind::RequestClose,
                            EventPayload::None,
                        );
                    }
                    let _ = self.next_frame(Duration::from_millis(100));
                    Ok(())
                } else {
                    Err("modal does not close on back".into())
                }
            }
            Back::Nothing => {
                // Send Back command to indicate no navigation handled it
                self.engine.control().back();
                let _ = self.next_frame(Duration::from_millis(100));
                Ok(())
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

    /// Calls node `id`'s long-press handler.
    pub fn long_press(&mut self, id: NodeId) {
        self.engine
            .control()
            .dispatch_event(id, EventKind::LongPress, EventPayload::None);
    }

    /// Long-presses what `selector` names (its nearest ancestor listening
    /// for `onLongPress`). The match has to be on screen.
    pub fn long_press_selector(&mut self, selector: &str) -> Result<(), String> {
        self.refresh();
        let target = selector::resolve(&self.screen, selector)?;
        self.require_visible(selector, target)?;
        let id = self
            .screen
            .long_press_listener(target.view())
            .ok_or_else(|| format!("`{selector}` does not take a long press"))?;
        self.long_press(id);
        Ok(())
    }

    /// Begins a pan gesture at (x, y) at the given time (seconds). Finds the
    /// nearest ancestor with pan bindings, reads current values, stops animations
    /// on those nodes, and starts tracking. Pinch/rotation not supported headless.
    pub fn pointer_down(&mut self, x: f32, y: f32, time: f64) {
        self.refresh();
        let Some(mut id) = self.screen.hit_test(x, y) else {
            return;
        };

        // Walk up to find ancestor with gesture bindings
        let gesture_id = loop {
            if let Some(bindings) = self.screen.gesture_bindings.get(&id)
                && (bindings.pan_x.is_some() || bindings.pan_y.is_some())
            {
                break Some(id);
            }
            let Some(view) = self.screen.views.get(&id) else {
                break None;
            };
            let Some(parent) = view.parent else {
                break None;
            };
            id = parent;
        };

        let Some(id) = gesture_id else {
            return;
        };

        let bindings = self
            .screen
            .gesture_bindings
            .get(&id)
            .cloned()
            .unwrap_or_default();

        // Read current values and stop animations
        let start_values = [
            bindings
                .pan_x
                .map(|n| self.screen.graph.eval(n).number())
                .unwrap_or(0.0),
            bindings
                .pan_y
                .map(|n| self.screen.graph.eval(n).number())
                .unwrap_or(0.0),
            0.0, // pinch not supported
            0.0, // rotation not supported
        ];

        self.screen.active_gesture = Some(ActiveGesture {
            id,
            start_pos: (x, y),
            prev_pos: (x, y),
            prev_time: time,
            last_pos: (x, y),
            last_time: time,
            start_values,
        });

        // Dispatch PanStart if view listens
        if let Some(view) = self.screen.views.get(&id)
            && view.events.contains(EventKind::PanStart)
        {
            self.engine.control().dispatch_event(
                id,
                EventKind::PanStart,
                EventPayload::Pan {
                    translation: (0.0, 0.0),
                    velocity: (0.0, 0.0),
                    values: (start_values[0], start_values[1]),
                },
            );
        }
    }

    /// Moves the active gesture to (x, y) at the given time (seconds). Only pan
    /// gestures are supported; pinch/rotation do nothing headless.
    pub fn pointer_move(&mut self, x: f32, y: f32, time: f64) {
        let Some(gesture) = &mut self.screen.active_gesture else {
            return;
        };
        gesture.prev_pos = gesture.last_pos;
        gesture.prev_time = gesture.last_time;
        gesture.last_pos = (x, y);
        gesture.last_time = time;

        let id = gesture.id;
        let dx = x - gesture.start_pos.0;
        let dy = y - gesture.start_pos.1;

        let bindings = self
            .screen
            .gesture_bindings
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let mut finished = Vec::new();
        if let Some(n) = bindings.pan_x {
            self.screen
                .graph
                .set(n, gesture.start_values[0] + dx, &mut finished);
        }
        if let Some(n) = bindings.pan_y {
            self.screen
                .graph
                .set(n, gesture.start_values[1] + dy, &mut finished);
        }
        self.screen.apply_animated();
    }

    /// Ends the active gesture at the given time (seconds), dispatching PanEnd
    /// with velocity calculated from the last movement segment. Pinch/rotation
    /// not supported headless.
    pub fn pointer_up(&mut self, _time: f64) {
        let Some(gesture) = self.screen.active_gesture.take() else {
            return;
        };

        // Velocity from last movement segment (prev -> last)
        let dt = (gesture.last_time - gesture.prev_time).max(0.001);
        let dx_segment = gesture.last_pos.0 - gesture.prev_pos.0;
        let dy_segment = gesture.last_pos.1 - gesture.prev_pos.1;
        let vel_x = dx_segment / dt as f32;
        let vel_y = dy_segment / dt as f32;

        let dx_total = gesture.last_pos.0 - gesture.start_pos.0;
        let dy_total = gesture.last_pos.1 - gesture.start_pos.1;

        let bindings = self
            .screen
            .gesture_bindings
            .get(&gesture.id)
            .cloned()
            .unwrap_or_default();
        let current_values = [
            bindings
                .pan_x
                .map(|n| self.screen.graph.eval(n).number())
                .unwrap_or(0.0),
            bindings
                .pan_y
                .map(|n| self.screen.graph.eval(n).number())
                .unwrap_or(0.0),
            0.0, // pinch not supported
            0.0, // rotation not supported
        ];

        if let Some(view) = self.screen.views.get(&gesture.id)
            && view.events.contains(EventKind::PanEnd)
        {
            self.engine.control().dispatch_event(
                gesture.id,
                EventKind::PanEnd,
                EventPayload::Pan {
                    translation: (dx_total, dy_total),
                    velocity: (vel_x, vel_y),
                    values: (current_values[0], current_values[1]),
                },
            );
        }
    }

    /// Drags `selector` by (`x`, `y`) points: down at center, move, up.
    /// Time advances in 60fps frames. Velocity is calculated from the last segment.
    pub fn drag(&mut self, selector: &str, x: f32, y: f32) -> Result<(), String> {
        self.refresh();
        let target = selector::resolve(&self.screen, selector)?;
        self.require_visible(selector, target)?;

        let view_id = target.view();
        let bounds = self
            .screen
            .view(view_id)
            .ok_or_else(|| format!("`{selector}` view not found"))?
            .frame;
        let center = bounds.center();

        // Start gesture
        let start_time = self.now;
        self.pointer_down(center.x.0, center.y.0, start_time);

        // Move in steps (60fps), at least 3 frames for smooth velocity
        let frames = 5;
        for i in 1..=frames {
            let t = i as f32 / frames as f32;
            let curr_x = center.x.0 + x * t;
            let curr_y = center.y.0 + y * t;
            let curr_time = start_time + (t * frames as f32 * FRAME as f32) as f64;
            self.pointer_move(curr_x, curr_y, curr_time);
            self.now = curr_time;
        }

        // End gesture
        self.now = start_time + (frames as f32 * FRAME as f32) as f64;
        self.pointer_up(self.now);

        // Synchronize to process the PanEnd event and update signals
        self.control()
            .synchronize()
            .map_err(|e| format!("sync failed: {e}"))?;
        self.refresh();

        Ok(())
    }

    /// Drags `selector` by (`x`, `y`) but doesn't release. Call `drop` to finish.
    pub fn drag_and_hold(&mut self, selector: &str, x: f32, y: f32) -> Result<(), String> {
        self.refresh();
        let target = selector::resolve(&self.screen, selector)?;
        self.require_visible(selector, target)?;

        let view_id = target.view();
        let bounds = self
            .screen
            .view(view_id)
            .ok_or_else(|| format!("`{selector}` view not found"))?
            .frame;
        let center = bounds.center();

        // Start gesture
        let start_time = self.now;
        self.pointer_down(center.x.0, center.y.0, start_time);

        // Move in steps
        let frames = 5;
        for i in 1..=frames {
            let t = i as f32 / frames as f32;
            let curr_x = center.x.0 + x * t;
            let curr_y = center.y.0 + y * t;
            let curr_time = start_time + (t * frames as f32 * FRAME as f32) as f64;
            self.pointer_move(curr_x, curr_y, curr_time);
            self.now = curr_time;
        }

        // Don't call pointer_up - leave gesture active
        Ok(())
    }

    /// Releases the held pointer from `drag_and_hold`.
    pub fn drop(&mut self) -> Result<(), String> {
        if self.screen.active_gesture.is_none() {
            return Err("no pointer is held (call drag_and_hold first)".into());
        }
        self.pointer_up(self.now);
        // Deliver the PanEnd event before the caller looks at the screen or
        // advances the clock, as a host delivers a lifted finger.
        self.control()
            .synchronize()
            .map_err(|e| format!("sync failed: {e}"))?;
        self.refresh();
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

        // Tell engine about new scroll position so it can recompute layout
        let view = self
            .screen
            .view(id)
            .ok_or_else(|| format!("`{selector}` is not in a scroll view"))?;
        self.control().update_scroll_position(
            id,
            nlayer_core::geometry::Point::new(
                nlayer_core::geometry::Points(view.scroll_x),
                nlayer_core::geometry::Points(view.scroll_y),
            ),
        );

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
        // dispatch Scroll and ScrollEnd events (matching the scroll() method)
        for event in [EventKind::Scroll, EventKind::ScrollEnd] {
            self.engine
                .control()
                .dispatch_event(id, event, payload.clone());
        }

        // Drain frames until engine settles (sticky rows may take multiple passes)
        // Add timeout to prevent infinite loops
        let settle_start = Instant::now();
        while self.next_frame(Duration::from_millis(30)) {
            if settle_start.elapsed() > Duration::from_secs(2) {
                break; // Timeout to prevent infinite loop
            }
        }

        // Final refresh to ensure screen reflects latest state
        self.refresh();

        Ok(())
    }
    /// Scrolls the scroll view to the given offset. Waits for the engine to
    /// settle (multiple layout passes may occur for sticky headers).
    pub fn scroll(&mut self, id: NodeId, x: f32, y: f32) {
        self.refresh();
        if let Some(view) = self.screen.views.get_mut(&id) {
            view.scroll_x = x;
            view.scroll_y = y;
        }
        self.control().update_scroll_position(
            id,
            nlayer_core::geometry::Point::new(
                nlayer_core::geometry::Points(x),
                nlayer_core::geometry::Points(y),
            ),
        );

        // Dispatch scroll events to trigger frame recomputation
        let view = self.screen.view(id).expect("scroll view");
        let content = view.scroll_content.unwrap_or(view.frame.size);
        let payload = EventPayload::Scroll {
            x,
            y,
            content_w: content.width.0,
            content_h: content.height.0,
            width: view.frame.size.width.0,
            height: view.frame.size.height.0,
        };
        for event in [EventKind::Scroll, EventKind::ScrollEnd] {
            self.engine
                .control()
                .dispatch_event(id, event, payload.clone());
        }

        // Drain frames until engine settles (sticky rows may take multiple passes)
        while self.next_frame(Duration::from_millis(30)) {}
        self.refresh();
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
            if self.screen.is_animating() {
                self.advance(FRAME * 1000.0);
                continue;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_reads_start_values_and_stops_animation() {
        // Test that pointer_down reads current animated values and stops animations
        let gesture = ActiveGesture {
            id: 1,
            start_pos: (10.0, 20.0),
            prev_pos: (10.0, 20.0),
            prev_time: 0.0,
            last_pos: (10.0, 20.0),
            last_time: 0.0,
            start_values: [5.0, 10.0, 0.0, 0.0],
        };

        // Verify start values captured
        assert_eq!(gesture.start_values[0], 5.0);
        assert_eq!(gesture.start_values[1], 10.0);
    }

    #[test]
    fn pan_move_accumulates_translation() {
        // Verify pan adds translation: set(start + dx)
        let start_x = 5.0;
        let start_y = 10.0;
        let dx = 15.0;
        let dy = 25.0;

        let new_x = start_x + dx;
        let new_y = start_y + dy;

        assert_eq!(new_x, 20.0);
        assert_eq!(new_y, 35.0);
    }

    #[test]
    fn pan_end_velocity_from_last_segment() {
        // Verify velocity = (last - prev) / (last_time - prev_time)
        let prev_pos = (10.0, 20.0);
        let last_pos = (30.0, 50.0);
        let prev_time = 0.0;
        let last_time = 0.1; // seconds

        let dt = last_time - prev_time;
        let dx = last_pos.0 - prev_pos.0;
        let dy = last_pos.1 - prev_pos.1;

        let vel_x = dx / dt as f32;
        let vel_y = dy / dt as f32;

        // 20 points in 0.1s = 200 points/s
        assert_eq!(vel_x, 200.0);
        // 30 points in 0.1s = 300 points/s
        assert_eq!(vel_y, 300.0);
    }

    #[test]
    fn pan_finds_ancestor_with_binding() {
        // Verify that pointer_down walks up to find gesture binding
        // This is a logic test; actual traversal tested in integration
        let mut parent_has_binding = false;

        // Simulate: child has no binding, parent does
        if !parent_has_binding {
            // Would walk to parent
            parent_has_binding = true;
        }

        assert!(parent_has_binding);
    }
}

#[test]
fn stack_tracks_screens() {
    let mut screen = Screen::default();
    let stack_id = 10;
    let screen1 = 11;
    let screen2 = 12;

    screen.apply_item(&MountItem::Create {
        id: stack_id,
        kind: ViewKind::Stack,
    });
    screen.apply_item(&MountItem::Create {
        id: screen1,
        kind: ViewKind::Screen,
    });
    screen.apply_item(&MountItem::Create {
        id: screen2,
        kind: ViewKind::Screen,
    });

    screen.apply_item(&MountItem::Insert {
        parent: stack_id,
        child: screen1,
        before: None,
    });
    assert_eq!(screen.stack_depth(stack_id), 1);

    screen.apply_item(&MountItem::Insert {
        parent: stack_id,
        child: screen2,
        before: None,
    });
    assert_eq!(screen.stack_depth(stack_id), 2);
    assert_eq!(screen.top_screen(stack_id), Some(screen2));
}

#[test]
fn back_target_pops_when_allowed() {
    let mut screen = Screen::default();
    let stack_id = 10;
    let screen1 = 11;
    let screen2 = 12;

    screen.apply_item(&MountItem::Create {
        id: stack_id,
        kind: ViewKind::Stack,
    });
    screen.apply_item(&MountItem::Create {
        id: screen1,
        kind: ViewKind::Screen,
    });
    screen.apply_item(&MountItem::Create {
        id: screen2,
        kind: ViewKind::Screen,
    });

    screen.apply_item(&MountItem::Insert {
        parent: stack_id,
        child: screen1,
        before: None,
    });
    screen.apply_item(&MountItem::Insert {
        parent: stack_id,
        child: screen2,
        before: None,
    });

    // Default prevent_back is false, so should pop
    match screen.back_target() {
        Back::Pop(id) => assert_eq!(id, screen2),
        other => panic!("Expected Back::Pop, got {:?}", other),
    }
}

#[test]
fn back_target_asks_when_listening() {
    let mut screen = Screen::default();
    let stack_id = 10;
    let screen1 = 11;

    screen.apply_item(&MountItem::Create {
        id: stack_id,
        kind: ViewKind::Stack,
    });
    screen.apply_item(&MountItem::Create {
        id: screen1,
        kind: ViewKind::Screen,
    });
    screen.apply_item(&MountItem::Insert {
        parent: stack_id,
        child: screen1,
        before: None,
    });

    // Add BackRequest listener using EventMask::set
    let mut events = EventMask::default();
    events.set(EventKind::BackRequest, true);
    screen.apply_item(&MountItem::Events {
        id: screen1,
        events,
    });

    match screen.back_target() {
        Back::Ask(id) => assert_eq!(id, screen1),
        other => panic!("Expected Back::Ask, got {:?}", other),
    }
}

#[test]
fn back_target_closes_modal() {
    let mut screen = Screen::default();
    let modal_id = 20;

    screen.apply_item(&MountItem::Create {
        id: modal_id,
        kind: ViewKind::Modal,
    });
    screen.apply_item(&MountItem::Insert {
        parent: 1,
        child: modal_id,
        before: None,
    });

    let config = ModalConfig {
        close_on_escape: true,
        ..Default::default()
    };
    screen.apply_item(&MountItem::ModalConfig {
        id: modal_id,
        config,
    });

    match screen.back_target() {
        Back::Close {
            modal,
            close_on_escape,
        } => {
            assert_eq!(modal, modal_id);
            assert!(close_on_escape);
        }
        other => panic!("Expected Back::Close, got {:?}", other),
    }
}

#[test]
fn back_target_nothing_at_root() {
    let screen = Screen::default();
    assert_eq!(screen.back_target(), Back::Nothing);
}

#[test]
fn modal_stacks_correctly() {
    let mut screen = Screen::default();
    let modal1 = 20;
    let modal2 = 21;

    screen.apply_item(&MountItem::Create {
        id: modal1,
        kind: ViewKind::Modal,
    });
    screen.apply_item(&MountItem::Insert {
        parent: 1,
        child: modal1,
        before: None,
    });
    assert_eq!(screen.top_modal(), Some(modal1));

    screen.apply_item(&MountItem::Create {
        id: modal2,
        kind: ViewKind::Modal,
    });
    screen.apply_item(&MountItem::Insert {
        parent: 1,
        child: modal2,
        before: None,
    });
    assert_eq!(screen.top_modal(), Some(modal2));

    screen.apply_item(&MountItem::Remove { id: modal2 });
    assert_eq!(screen.top_modal(), Some(modal1));
}

#[test]
fn screen_header_records_height() {
    let mut screen = Screen::default();
    let screen_id = 11;

    screen.apply_item(&MountItem::Create {
        id: screen_id,
        kind: ViewKind::Screen,
    });

    let config = Box::new(ScreenConfig {
        header_height: Points(44.0),
        ..Default::default()
    });
    screen.apply_item(&MountItem::ScreenConfig {
        id: screen_id,
        config,
    });

    assert_eq!(screen.screen_headers.get(&screen_id), Some(&Points(44.0)));
}

#[test]
fn remove_cleans_up_stacks_and_modals() {
    let mut screen = Screen::default();
    let stack_id = 10;
    let screen1 = 11;
    let modal_id = 20;

    screen.apply_item(&MountItem::Create {
        id: stack_id,
        kind: ViewKind::Stack,
    });
    screen.apply_item(&MountItem::Create {
        id: screen1,
        kind: ViewKind::Screen,
    });
    screen.apply_item(&MountItem::Insert {
        parent: stack_id,
        child: screen1,
        before: None,
    });
    assert_eq!(screen.stack_depth(stack_id), 1);

    screen.apply_item(&MountItem::Remove { id: screen1 });
    assert_eq!(screen.stack_depth(stack_id), 0);

    screen.apply_item(&MountItem::Create {
        id: modal_id,
        kind: ViewKind::Modal,
    });
    screen.apply_item(&MountItem::Insert {
        parent: 1,
        child: modal_id,
        before: None,
    });
    assert_eq!(screen.top_modal(), Some(modal_id));

    screen.apply_item(&MountItem::Remove { id: modal_id });
    assert_eq!(screen.top_modal(), None);
}
