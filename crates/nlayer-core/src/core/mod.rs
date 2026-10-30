//! Shadow tree.
//!
//! Apply, commit and frame emission for views, text, images, scroll views,
//! virtualized lists and inputs, extracted from `nsh_core::core` and cut to those
//! hosts. `env()` on padding and margin follows the surface safe area and
//! the keyboard inset. `activeStyle`, `hoverStyle` and `focusStyle` are
//! stored and sent to the platform; they do not change layout. `testID` is
//! a name for tests: a view's is mounted, a nested span's is recorded on
//! its block, and neither is drawn. A pressable nested span is a link in
//! that block. Inline views inside text and text selection are not part of
//! this slice.

mod fenwick;
mod layout_tree;
mod list;

use std::{mem, sync::Arc};

use taffy::{AvailableSpace, Layout, Position, Style as TaffyStyle};

use layout_tree::LayoutTree;

use rustc_hash::FxHashSet;

use crate::{
    geometry::{Bounds, Edges, Percentage, Point, Points, Size, rect},
    mount::{InputConfig, MountBatch, MountItem, ViewKind},
    ops::{
        Accessibility, AccessibilityProp, CellSlot, Command, EventKind, EventMask, ImageSource,
        Interaction, KeyboardDismiss, KeyboardTaps, NodeId, Op, Prop, ROOT_ID, ResizeMode, Tag,
    },
    style::{self, EllipsizeMode, EnvEdges, Style, TextAlign, TextStyle, ViewStyle},
    text::{AttributedText, MeasureCache, ResolvedTextStyle, SpanTestId, TextEngine, TextLayout},
};

/// Slab index of a node. Internal links use keys, never ids.
type Key = u32;

const NO_KEY: Key = Key::MAX;

/// The longest run without a line-break opportunity a multiline input's
/// value is measured with unbroken (see [`broken_runs`]).
const MEASURED_RUN: usize = 1024;

type LayoutId = taffy::NodeId;

mod dirty {
    /// Emit [`crate::MountItem::ViewStyle`].
    pub(super) const VIEW: u32 = 1 << 0;
    /// Emit [`crate::MountItem::Events`].
    pub(super) const EVENTS: u32 = 1 << 1;
    /// Rebuild the attributed text of a text block.
    pub(super) const TEXT_BUILD: u32 = 1 << 2;
    /// Lay out the text block again and emit [`crate::MountItem::Text`].
    pub(super) const TEXT_MOUNT: u32 = 1 << 3;
    /// Emit [`crate::MountItem::Image`].
    pub(super) const IMAGE: u32 = 1 << 4;
    /// Emit [`crate::MountItem::ScrollConfig`].
    pub(super) const SCROLL: u32 = 1 << 5;
    /// Emit [`crate::MountItem::Input`].
    pub(super) const INPUT: u32 = 1 << 6;
    /// Emit [`crate::MountItem::InputValue`].
    pub(super) const INPUT_VALUE: u32 = 1 << 7;
    /// Emit [`crate::MountItem::StateStyle`] for the slots in `state_dirty`.
    pub(super) const STATE: u32 = 1 << 8;
    /// Emit [`crate::MountItem::TestId`].
    pub(super) const TEST_ID: u32 = 1 << 9;
    /// Emit [`crate::MountItem::Accessibility`].
    pub(super) const ACCESSIBILITY: u32 = 1 << 10;
    /// Emit [`crate::MountItem::Command`] `SetSelection`.
    pub(super) const SELECTION: u32 = 1 << 11;
    /// The list needs its window recomputed.
    pub(super) const LIST: u32 = 1 << 12;
    /// Emit [`crate::MountItem::AnimationBindings`].
    pub(super) const ANIMATION: u32 = 1 << 13;
}

/// Host kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    View,
    /// A text block, or a span when [`Node::inline`].
    Text,
    /// A run of characters. Not a native view.
    RawText,
    Image,
    /// Scrolls on one axis ([`ScrollState::horizontal`]).
    Scroll,
    /// A virtualized list: a scroll view whose cell children are layout
    /// roots of their own.
    List,
    /// A recycled list cell.
    Cell,
    /// A text field, one line or several.
    Input,
}

impl Kind {
    fn is_container(self) -> bool {
        matches!(
            self,
            Kind::View | Kind::Image | Kind::Scroll | Kind::List | Kind::Cell
        )
    }

    fn of(tag: Tag) -> Self {
        match tag {
            Tag::View => Kind::View,
            Tag::Text => Kind::Text,
            Tag::Image => Kind::Image,
            Tag::Scroll => Kind::Scroll,
            Tag::List => Kind::List,
            Tag::Cell => Kind::Cell,
            Tag::Input => Kind::Input,
        }
    }

    fn view_kind(self) -> ViewKind {
        match self {
            Kind::View => ViewKind::View,
            Kind::Text => ViewKind::Text,
            Kind::Image => ViewKind::Image,
            Kind::Scroll | Kind::List => ViewKind::Scroll,
            Kind::Cell => ViewKind::View,
            Kind::Input => ViewKind::Input,
            // Raw text is never created as a native view.
            Kind::RawText => ViewKind::View,
        }
    }
}

/// Cells the runtime must render before the frame shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListBind {
    /// The list.
    pub list: NodeId,
    /// `(slot, item index)` pairs.
    pub binds: Vec<(u32, u32)>,
}

/// An op referenced a node in an impossible way. This is a renderer bug;
/// the op is skipped and the tree stays consistent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyError {
    /// The id is unknown (never created or already released).
    UnknownNode(NodeId),
    /// The id is already in use.
    DuplicateNode(NodeId),
    /// The child is not a child of the given parent.
    NotAChild(NodeId),
    /// The `before` sibling is not a child of the given parent.
    BadAnchor(NodeId),
    /// The insert would create a cycle.
    Cycle(NodeId),
    /// Creating the node would exceed the node quota ([`Core::set_max_nodes`]).
    TooManyNodes(usize),
    /// A new id far beyond the ids in use (ids index a table, so they must
    /// stay dense).
    IdOutOfRange(NodeId),
}

impl ApplyError {
    /// The app went over a quota or sent what no runtime sends: it should
    /// be stopped rather than continue.
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            ApplyError::TooManyNodes(_) | ApplyError::IdOutOfRange(_)
        )
    }
}

/// A scroll view's axis and the content size last sent to the platform.
#[derive(Debug, Default)]
struct ScrollState {
    horizontal: bool,
    taps: KeyboardTaps,
    dismiss: KeyboardDismiss,
    /// Last content size sent.
    content: Option<Size<Points>>,
    /// Index of that [`MountItem::ScrollContent`] in the current mount
    /// batch. A later size in the same batch replaces it, so the platform
    /// never sees a transient size (which would clamp the offset before the
    /// content grows back).
    content_item: Option<usize>,
}

/// A single-line input's text and the configuration last sent.
#[derive(Debug, Default)]
struct InputState {
    value: String,
    /// Edits reported by the platform.
    count: u32,
    /// Everything but the text style, which comes from the node's style.
    config: InputConfig,
    /// Last content box sent to the platform.
    content: Option<Bounds<Points>>,
    /// The input is measured with its placeholder (its value is empty).
    placeholder_measured: bool,
    /// `selectedRange` to send after this batch's value.
    selection: Option<(u32, u32)>,
}

/// Paint for one pointer state. Not the node's layout style.
#[derive(Debug, Clone, PartialEq)]
struct StatePaint {
    view: ViewStyle,
    text: TextStyle,
}

struct Node {
    id: NodeId,
    kind: Kind,
    /// Inside a text block: a nested `<text>` span or a raw text run.
    inline: bool,
    parent: Key,
    children: Vec<Key>,
    /// Layout node. Absent for raw text and for spans.
    layout: Option<LayoutId>,
    /// Layout style kept while a text block is a span (no layout node).
    base_layout: Option<Box<TaffyStyle>>,
    view_style: ViewStyle,
    /// Visual properties driven by animation nodes; they override the
    /// static values while bound.
    animated: Vec<(crate::animation::AnimationProp, crate::animation::NodeId)>,
    text_style: TextStyle,
    /// `numberOfLines` (`Some(0)`: no clamp).
    number_of_lines: Option<u32>,
    /// `textOverflow`.
    ellipsize: Option<EllipsizeMode>,
    /// Raw text content.
    text: String,
    /// Start / end properties, if any, so a direction change can place them
    /// again.
    logical: Option<Box<style::Logical>>,
    events: EventMask,
    /// `objectFit`. `None` is the image default (cover).
    object_fit: Option<ResizeMode>,
    /// Present exactly for [`Kind::Image`]: `None` until `src` is set.
    image: Option<Option<ImageSource>>,
    /// Present for [`Kind::Scroll`] and [`Kind::List`].
    scroll: Option<Box<ScrollState>>,
    /// Present exactly for [`Kind::List`].
    list: Option<Box<list::ListState>>,
    /// Present exactly for a [`Kind::Cell`] in a list.
    slot: Option<CellSlot>,
    /// Present exactly for [`Kind::Input`].
    input: Option<Box<InputState>>,
    /// `zIndex`: native views are ordered by it (stable) among siblings.
    z_index: i32,
    /// Laid out right to left (the resolved `direction`).
    rtl: bool,
    /// `direction`: `None` inherits.
    direction: Option<bool>,
    /// `env()` terms of the padding and margin, and the insets last added
    /// to the authored lengths (padding, then margin).
    env: Option<Box<(EnvEdges, [Edges<Points>; 2])>>,
    /// Its layout root was laid out this commit: its subtree's frames are
    /// new.
    relaid: bool,
    /// The native view exists.
    created: bool,
    /// Last frame sent to the platform.
    frame: Bounds<Points>,
    dirty: u32,
    queued: bool,
    text_queued: bool,
    /// `activeStyle`, `hoverStyle`, `focusStyle`, in that order.
    state: [Option<StatePaint>; 3],
    /// Slots of [`Self::state`] to send on the next [`dirty::STATE`].
    state_dirty: u8,
    /// `testID`. Absent until set. A span's is also copied into its block.
    test_id: Option<Arc<str>>,
    /// `role`, label, hint, state, value and whether the subtree is hidden
    /// from assistive technologies.
    accessibility: Accessibility,
}
impl Node {
    fn new(id: NodeId, kind: Kind) -> Self {
        Self {
            id,
            kind,
            inline: false,
            parent: NO_KEY,
            children: Vec::new(),
            layout: None,
            base_layout: None,
            view_style: ViewStyle::default(),
            animated: Vec::new(),
            text_style: TextStyle::default(),
            number_of_lines: None,
            ellipsize: None,
            text: String::new(),
            logical: None,
            events: EventMask::default(),
            object_fit: None,
            image: matches!(kind, Kind::Image).then_some(None),
            scroll: matches!(kind, Kind::Scroll | Kind::List).then(Box::default),
            list: matches!(kind, Kind::List).then(|| Box::new(list::ListState::new())),
            slot: None,
            input: matches!(kind, Kind::Input).then(Box::default),
            z_index: 0,
            rtl: false,
            direction: None,
            env: None,
            relaid: false,
            created: false,
            frame: Bounds::default(),
            dirty: 0,
            queued: false,
            text_queued: false,
            state: [None, None, None],
            state_dirty: 0,
            test_id: None,
            accessibility: Accessibility::default(),
        }
    }
}

/// Taffy context of a text block.
struct TextContext<P> {
    text: Arc<AttributedText>,
    /// `text.layout_hash()`, computed once per content.
    layout_hash: u64,
    /// Platform typesetting state, created on first use.
    prepared: Option<P>,
    /// Natural width of the widest UAX #14 unbreakable segment.
    min_content: Option<Points>,
    /// Last measurement: `(max_width, size)`.
    cache: Option<(Option<f32>, Size<Points>)>,
    /// Last final layout sent to the platform and the content box it was
    /// made for (where in the view the text goes).
    laid_out: Option<LaidText>,
    /// `testID`s changed and the glyphs did not: remount the text on the
    /// layout already sent.
    metadata_changed: bool,
}

/// The final text sent with its layout.
struct LaidText {
    content: Bounds<Points>,
    layout: TextLayout,
}

impl<P> TextContext<P> {
    fn new(text: Arc<AttributedText>) -> Self {
        Self {
            layout_hash: text.layout_hash(),
            text,
            prepared: None,
            min_content: None,
            cache: None,
            laid_out: None,
            metadata_changed: false,
        }
    }
}

/// Measured leaves: text blocks, inputs and images with a known
/// natural size.
enum LeafContext<P> {
    Text(TextContext<P>),
    Image(Option<Size<Points>>),
}

/// The shadow tree of one surface.
///
/// Feed it [`Op`]s with [`Core::apply`], then call [`Core::commit`] once per
/// JS turn to get the [`MountBatch`] for the platform. `M` measures text.
pub struct Core<M: TextEngine> {
    nodes: slab::Slab<Node>,
    /// `slots[id] = key + 1`, 0 if free. Ids index the table directly.
    slots: Vec<Key>,
    layout: LayoutTree<LeafContext<M::Prepared>>,
    text_engine: M,
    measure_cache: MeasureCache,
    max_nodes: usize,
    root: Key,
    viewport: Size<Points>,
    mount: Vec<MountItem>,
    queue: Vec<Key>,
    /// Text blocks whose final layout must be sent.
    pending_text: Vec<Key>,
    needs_layout: bool,
    /// Device pixels per point. Bundled image variants are picked from it.
    pixel_ratio: f32,
    /// Safe-area insets of the surface. `env(safe-area-inset-*)` on
    /// padding and margin reads them.
    safe_area: Edges<Points>,
    /// Software keyboard height over the surface. `env(keyboard-inset-height)`
    /// reads it; negative values count as zero.
    keyboard_inset: Points,
    /// Nodes whose padding or margin uses `env()`.
    env_styled: FxHashSet<NodeId>,
    /// Live lists.
    lists: Vec<Key>,
    /// List scroll commands waiting for this commit's list update.
    commands: Vec<(NodeId, Command)>,
    /// What the runtime must bind before the frame shows.
    list_binds: Vec<ListBind>,
    /// Events the core raises for JS (a list's end came near).
    events: Vec<(NodeId, EventKind)>,
}

impl<M: TextEngine> Core<M> {
    /// Creates a core whose root view (id [`ROOT_ID`]) fills `viewport`.
    /// The platform owns that view; it is not in the mount batch. `text_engine`
    /// measures and lays out text on the engine thread.
    pub fn new(text_engine: M, viewport: Size<Points>) -> Self {
        let mut layout = LayoutTree::with_capacity(256);
        let root_layout = layout.new_leaf(root_style(viewport));
        let mut nodes = slab::Slab::with_capacity(256);
        let mut root = Node::new(ROOT_ID, Kind::View);
        root.layout = Some(root_layout);
        root.created = true;
        let root = nodes.insert(root) as Key;
        layout.set_tag(root_layout, root);
        let mut slots = vec![0; 256];
        slots[ROOT_ID as usize] = root + 1;
        Self {
            nodes,
            slots,
            layout,
            text_engine,
            measure_cache: MeasureCache::default(),
            max_nodes: usize::MAX,
            root,
            viewport,
            mount: Vec::new(),
            queue: Vec::new(),
            pending_text: Vec::new(),
            needs_layout: true,
            pixel_ratio: 1.0,
            safe_area: Edges::default(),
            keyboard_inset: Points::ZERO,
            env_styled: FxHashSet::default(),
            lists: Vec::new(),
            commands: Vec::new(),
            list_binds: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Caps how many nodes [`Op::Create`] may add.
    pub fn set_max_nodes(&mut self, max: usize) {
        self.max_nodes = max;
    }

    /// Changes the size of the root view.
    pub fn set_viewport(&mut self, viewport: Size<Points>) {
        if viewport != self.viewport {
            self.viewport = viewport;
            let root = self.nodes[self.root as usize]
                .layout
                .expect("root has a layout");
            self.layout.set_style(root, root_style(viewport));
            self.needs_layout = true;
        }
    }

    /// Device pixels per point. Images whose bundled variant changes are
    /// mounted again; their layout does not change.
    pub fn set_pixel_ratio(&mut self, pixel_ratio: f32) {
        if pixel_ratio == self.pixel_ratio {
            return;
        }
        let old = self.pixel_ratio;
        self.pixel_ratio = pixel_ratio;
        self.repick_images(old, pixel_ratio);
    }

    /// Safe-area insets of the surface. Padding and margin that name
    /// `env(safe-area-inset-*)` follow them. One surface: v1 has no
    /// per-screen insets.
    pub fn set_safe_area(&mut self, insets: Edges<Points>) {
        if self.safe_area != insets {
            self.safe_area = insets;
            self.sync_env();
        }
    }

    /// Height of the software keyboard over the surface.
    /// `env(keyboard-inset-height)` on padding and margin follows it.
    /// Layout updates at once; v1 does not animate with the keyboard.
    pub fn set_keyboard_inset(&mut self, height: Points) {
        if self.keyboard_inset != height {
            self.keyboard_inset = height;
            self.sync_env();
        }
    }

    /// The platform scrolled a list to `offset`. A report from before a
    /// scroll the core issued is replayed onto it.
    pub fn set_scroll_offset(&mut self, id: NodeId, offset: Point<Points>) {
        let Ok(key) = self.key(id) else { return };
        let Some(list) = self.nodes[key as usize].list.as_mut() else {
            return;
        };
        list.reported(offset.y.0);
        self.mark(key, dirty::LIST);
    }

    /// Moves the cells the runtime must render before the frame shows.
    pub fn take_list_binds(&mut self, out: &mut Vec<ListBind>) {
        out.append(&mut self.list_binds);
    }

    /// The events the core raised for JS.
    pub fn take_events(&mut self, out: &mut Vec<(NodeId, EventKind)>) {
        out.append(&mut self.events);
    }

    /// The user edited input `id` (before JS sees the edit): keeps the text
    /// for layout and for stale-value detection.
    pub fn input_changed(&mut self, id: NodeId, text: &str, count: u32) {
        let Ok(key) = self.key(id) else { return };
        let Some(input) = self.nodes[key as usize].input.as_mut() else {
            return;
        };
        input.count = count;
        if input.value != text {
            input.value.clear();
            input.value.push_str(text);
            self.measure_input(key);
        }
    }

    /// Applies one op from JS.
    pub fn apply(&mut self, op: Op) -> Result<(), ApplyError> {
        match op {
            Op::Create { id, tag } => self.create(id, tag),
            Op::CreateText { id, text } => {
                let key = self.alloc(id, Kind::RawText)?;
                self.nodes[key as usize].text = text;
                Ok(())
            }
            Op::SetText { id, text } => {
                let key = self.key(id)?;
                let node = &mut self.nodes[key as usize];
                if node.text != text {
                    node.text = text;
                    if node.inline {
                        self.mark_text_block(key);
                    }
                }
                Ok(())
            }
            Op::Animation(op) => {
                self.mount.push(MountItem::Animation(op));
                Ok(())
            }
            Op::SetStyle { id, style, .. } => self.set_style(id, *style),
            Op::SetProp { id, prop } => self.set_prop(id, prop),
            Op::Listen { id, event, enabled } => {
                let key = self.key(id)?;
                let node = &mut self.nodes[key as usize];
                let before = node.events;
                node.events.set(event, enabled);
                if node.events != before {
                    // A span's press is a link in its block, not a native view.
                    if node.inline && node.kind == Kind::Text {
                        self.mark_text_block(key);
                    } else {
                        self.mark(key, dirty::EVENTS);
                    }
                }
                Ok(())
            }
            Op::Insert {
                parent,
                child,
                before,
            } => self.insert(parent, child, before),
            Op::Remove { parent, child } => {
                let parent = self.key(parent)?;
                let child = self.key(child)?;
                if self.nodes[child as usize].parent != parent {
                    return Err(ApplyError::NotAChild(self.nodes[child as usize].id));
                }
                self.unlink(child, true);
                Ok(())
            }
            Op::Release { id } => self.release(id),
            Op::Command { id, command } => {
                let list = self
                    .key(id)
                    .is_ok_and(|key| self.nodes[key as usize].kind == Kind::List);
                if list
                    && matches!(
                        command,
                        Command::ScrollToIndex { .. } | Command::ScrollToEnd { .. }
                    )
                {
                    if let Ok(key) = self.key(id) {
                        self.mark(key, dirty::LIST);
                    }
                    self.commands.push((id, command));
                } else {
                    self.mount.push(MountItem::Command { id, command });
                }
                Ok(())
            }
        }
    }

    /// Finishes a turn: runs layout and returns the mount items, or `None`
    /// when nothing changed.
    pub fn commit(&mut self) -> Option<MountBatch> {
        self.flush_dirty();
        if mem::take(&mut self.needs_layout) {
            let relaid = self.relayout_boundaries();
            if self.layout_main() {
                let root = self.root;
                let children = mem::take(&mut self.nodes[root as usize].children);
                for &child in &children {
                    self.emit_frames(child);
                }
                self.nodes[root as usize].children = children;
            }
            for key in relaid {
                self.emit_frames(key);
            }
        }
        self.layout_cells();
        self.update_lists();
        self.flush_text();
        if self.mount.is_empty() {
            None
        } else {
            Some(MountBatch {
                items: mem::take(&mut self.mount),
            })
        }
    }

    fn key(&self, id: NodeId) -> Result<Key, ApplyError> {
        match self.slots.get(id as usize) {
            Some(&k) if k != 0 => Ok(k - 1),
            _ => Err(ApplyError::UnknownNode(id)),
        }
    }

    fn alloc(&mut self, id: NodeId, kind: Kind) -> Result<Key, ApplyError> {
        let idx = id as usize;
        if self.nodes.len() >= self.max_nodes {
            return Err(ApplyError::TooManyNodes(self.max_nodes));
        }
        if idx > self.slots.len().saturating_add(self.max_nodes) {
            return Err(ApplyError::IdOutOfRange(id));
        }
        if idx >= self.slots.len() {
            self.slots.resize((idx + 1).next_power_of_two(), 0);
        } else if self.slots[idx] != 0 {
            return Err(ApplyError::DuplicateNode(id));
        }
        let key = self.nodes.insert(Node::new(id, kind)) as Key;
        self.slots[idx] = key + 1;
        Ok(key)
    }

    fn create(&mut self, id: NodeId, tag: Tag) -> Result<(), ApplyError> {
        let kind = Kind::of(tag);
        let key = self.alloc(id, kind)?;
        let mut layout_style = Style::default().layout;
        style::resolve_normal_alignment(&mut layout_style);
        // The authored style. [`Self::sync_scroll_style`] derives the
        // taffy style (the scroll axis, and a horizontal row).
        let scroll_base = matches!(kind, Kind::Scroll | Kind::List).then(|| layout_style.clone());
        let layout = match kind {
            Kind::Image => self
                .layout
                .new_leaf_with_context(layout_style, LeafContext::Image(None)),
            Kind::Text | Kind::Input => self.layout.new_leaf_with_context(
                layout_style,
                LeafContext::Text(TextContext::new(Arc::default())),
            ),
            Kind::View | Kind::Scroll | Kind::List | Kind::Cell => {
                self.layout.new_leaf(layout_style)
            }
            Kind::RawText => unreachable!("raw text is CreateText, not a tag"),
        };
        self.layout.set_tag(layout, key);
        let node = &mut self.nodes[key as usize];
        node.layout = Some(layout);
        if kind == Kind::Text {
            node.dirty |= dirty::TEXT_BUILD;
        }
        if let Some(base) = scroll_base {
            node.base_layout = Some(Box::new(base));
        }
        if matches!(kind, Kind::Scroll | Kind::List) {
            self.sync_scroll_style(key);
        }
        if kind == Kind::List {
            self.lists.push(key);
        }
        if kind == Kind::Input {
            self.measure_input(key);
        }
        Ok(())
    }

    fn set_style(&mut self, id: NodeId, style: Style) -> Result<(), ApplyError> {
        let fit = style.object_fit();
        let key = self.key(id)?;
        let Style {
            mut layout,
            mut view,
            text,
            logical,
            z_index,
            direction,
            animated,
            pending,
            ..
        } = style;
        if key != self.root {
            self.nodes[key as usize].direction = direction;
        }
        self.nodes[key as usize].logical = (!logical.is_empty()).then(|| Box::new(logical));
        let rtl = self.resolved_rtl(key);
        let flipped = self.nodes[key as usize].rtl != rtl;
        style::resolve_normal_alignment(&mut layout);
        style::apply_direction(&mut layout, &mut view, &logical, rtl);
        let env_edges = pending.env;
        let applied = (!env_edges.is_empty()).then(|| self.env_insets(&env_edges));
        let node = &mut self.nodes[key as usize];
        let span_background_changed = node.inline
            && node.kind == Kind::Text
            && span_background(&node.view_style, node.id) != span_background(&view, node.id);
        let lines_changed = node.kind == Kind::Text
            && (node.number_of_lines != pending.number_of_lines
                || node.ellipsize != pending.text_overflow);
        let text_changed = node.text_style != text || span_background_changed || lines_changed;
        node.rtl = rtl;
        let view_changed = node.view_style != view;
        let animation_changed = node.animated != animated;
        let z_changed = node.z_index != z_index;
        node.view_style = view;
        node.animated = animated;
        node.text_style = text;
        node.number_of_lines = pending.number_of_lines;
        node.ellipsize = pending.text_overflow;
        node.z_index = z_index;
        let fit_changed = node.image.is_some() && node.object_fit != fit;
        if fit_changed {
            node.object_fit = fit;
        }
        // `overflow: "hidden"` clips in layout as well as in paint. `Clip`,
        // not `Hidden`: both stop the overflow from propagating, but `Hidden`
        // is a scroll container in CSS and would also make the box's
        // automatic minimum size as a flex or grid item 0.
        let clip = if node.view_style.overflow_hidden {
            taffy::Overflow::Clip
        } else {
            taffy::Overflow::Visible
        };
        layout.overflow = taffy::Point { x: clip, y: clip };
        let kind = node.kind;
        let id = node.id;
        match applied {
            Some(applied) => node.env = Some(Box::new((env_edges, applied))),
            None => {
                node.env = None;
            }
        }
        let tracked = applied.is_some();
        if tracked {
            self.env_styled.insert(id);
        } else {
            self.env_styled.remove(&id);
        }
        if matches!(kind, Kind::Text | Kind::Scroll | Kind::List) {
            self.nodes[key as usize].base_layout = Some(Box::new(layout.clone()));
        }
        if matches!(kind, Kind::Scroll | Kind::List) {
            // The scroll axis overrides the authored overflow. `env()` is
            // added there, on the physical edges, before a right-to-left
            // row swaps padding.
            self.sync_scroll_style(key);
        } else if let Some(l) = self.nodes[key as usize].layout {
            if let Some((_, applied)) = self.nodes[key as usize].env.as_deref() {
                add_insets(&mut layout, applied[0], applied[1]);
            }
            self.layout.set_style(l, layout);
            self.needs_layout = true;
        }
        if z_changed {
            self.restack(key);
        }
        if view_changed {
            self.mark(key, dirty::VIEW);
        }
        if animation_changed {
            self.mark(key, dirty::ANIMATION);
        }
        if fit_changed {
            self.mark(key, dirty::IMAGE);
        }
        if text_changed || flipped {
            match self.nodes[key as usize].kind {
                Kind::Text => self.mark_text_block(key),
                Kind::Input => {
                    self.measure_input(key);
                    self.mark(key, dirty::INPUT);
                }
                _ if text_changed => self.mark_inheriting_text(key),
                _ => {}
            }
        }
        if flipped {
            self.update_children_direction(key);
        }
        Ok(())
    }

    fn set_prop(&mut self, id: NodeId, prop: Prop) -> Result<(), ApplyError> {
        let key = self.key(id)?;
        match prop {
            Prop::Source(source) => {
                let Some(image) = self.nodes[key as usize].image.as_mut() else {
                    return Ok(());
                };
                let size = source.as_ref().and_then(|s| s.size);
                *image = source;
                self.mark(key, dirty::IMAGE);
                let Some(layout) = self.nodes[key as usize].layout else {
                    return Ok(());
                };
                if let Some(LeafContext::Image(natural)) = self.layout.get_node_context_mut(layout)
                    && *natural != size
                {
                    *natural = size;
                    self.layout.mark_dirty(layout);
                    self.needs_layout = true;
                }
            }
            Prop::Horizontal(horizontal) => {
                if self.nodes[key as usize].kind == Kind::List {
                    if horizontal {
                        log::warn!("a v2 list scrolls vertically; `horizontal` is ignored");
                    }
                    return Ok(());
                }
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.horizontal != horizontal;
                        scroll.horizontal = horizontal;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                    self.sync_scroll_style(key);
                }
            }
            Prop::KeyboardTaps(taps) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.taps != taps;
                        scroll.taps = taps;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::KeyboardDismiss(mode) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.dismiss != mode;
                        scroll.dismiss = mode;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::Count(count) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_count(count);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::EstimatedItemSize(size) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_estimated(size.0);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Columns(columns) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_columns(columns);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Splice {
                at,
                removed,
                inserted,
            } => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.splice(at, removed, inserted);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Slot(slot) => {
                self.nodes[key as usize].slot = Some(slot);
                let parent = self.nodes[key as usize].parent;
                if parent != NO_KEY
                    && let Some(list) = self.nodes[parent as usize].list.as_mut()
                {
                    list.add_slot_cell(slot, key);
                    self.mark(parent, dirty::LIST);
                }
            }
            Prop::CellIndex(index) => {
                let parent = self.nodes[key as usize].parent;
                if parent != NO_KEY
                    && let Some(list) = self.nodes[parent as usize].list.as_mut()
                {
                    list.rendered(key, index);
                    self.mark(parent, dirty::LIST);
                }
            }
            Prop::LoadMoreThreshold(threshold) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_end_threshold(threshold);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Value(value) => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                if input.value != value {
                    input.value = value;
                    self.mark(key, dirty::INPUT_VALUE);
                    self.measure_input(key);
                }
            }
            Prop::Placeholder(text) => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                input.config.placeholder = text;
                self.mark(key, dirty::INPUT);
                self.measure_input(key);
            }
            Prop::Secure(secure) => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                input.config.secure = secure;
                self.mark(key, dirty::INPUT);
            }
            Prop::Keyboard(keyboard) => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                input.config.keyboard = keyboard;
                self.mark(key, dirty::INPUT);
            }
            Prop::Multiline(multiline) => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                if input.config.multiline != multiline {
                    input.config.multiline = multiline;
                    self.mark(key, dirty::INPUT);
                    self.measure_input(key);
                }
            }
            Prop::Selection { start, end } => {
                let Some(input) = self.nodes[key as usize].input.as_mut() else {
                    return Ok(());
                };
                input.selection = Some((start, end));
                self.mark(key, dirty::SELECTION);
            }
            Prop::Disabled(disabled) => {
                if let Some(input) = self.nodes[key as usize].input.as_mut() {
                    input.config.disabled = disabled;
                    self.mark(key, dirty::INPUT);
                }
                if self.nodes[key as usize].accessibility.state.disabled != disabled {
                    self.nodes[key as usize].accessibility.state.disabled = disabled;
                    self.mark(key, dirty::ACCESSIBILITY);
                }
            }
            Prop::Accessibility(prop) => {
                let node = &mut self.nodes[key as usize];
                let accessibility = &mut node.accessibility;
                let changed = match prop {
                    AccessibilityProp::Label(label) => {
                        let changed = accessibility.label != label;
                        accessibility.label = label;
                        changed
                    }
                    AccessibilityProp::Hint(hint) => {
                        let changed = accessibility.hint != hint;
                        accessibility.hint = hint;
                        changed
                    }
                    AccessibilityProp::Role(role) => {
                        let changed = accessibility.role != role;
                        accessibility.role = role;
                        changed
                    }
                    AccessibilityProp::State(state) => {
                        let changed = accessibility.state != state;
                        accessibility.state = state;
                        changed
                    }
                    AccessibilityProp::Value(value) => {
                        let changed = accessibility.value != value;
                        accessibility.value = value;
                        changed
                    }
                    AccessibilityProp::Hidden(hidden) => {
                        let changed = accessibility.hidden != hidden;
                        accessibility.hidden = hidden;
                        changed
                    }
                };
                if changed {
                    self.mark(key, dirty::ACCESSIBILITY);
                }
            }
            Prop::TestId(value) => {
                let node = &mut self.nodes[key as usize];
                if node.test_id == value {
                    return Ok(());
                }
                let inline = node.kind == Kind::Text && node.inline;
                node.test_id = value;
                if inline {
                    self.mark_text_block(key);
                } else {
                    self.mark(key, dirty::TEST_ID);
                }
            }
            Prop::StateStyle { interaction, paint } => {
                let slot = interaction.index();
                let next = paint.map(|paint| StatePaint {
                    view: paint.0,
                    text: paint.1,
                });
                if self.nodes[key as usize].state[slot] != next {
                    self.nodes[key as usize].state[slot] = next;
                    self.nodes[key as usize].state_dirty |= 1 << slot;
                    self.mark(key, dirty::STATE);
                }
            }
        }
        Ok(())
    }

    /// The pixel ratio went from `old` to `new`: the images whose variant
    /// changes are sent again.
    fn repick_images(&mut self, old: f32, new: f32) {
        let images: Vec<Key> = self
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.image.as_ref().is_some_and(|source| {
                    source
                        .as_ref()
                        .is_some_and(|s| !s.scales.is_empty() && s.variant(old) != s.variant(new))
                })
            })
            .map(|(k, _)| k as Key)
            .collect();
        for key in images {
            self.mark(key, dirty::IMAGE);
        }
    }

    /// The direction `key` lays out in: its own, else its parent's (the
    /// root's while detached).
    fn resolved_rtl(&self, key: Key) -> bool {
        let node = &self.nodes[key as usize];
        if let Some(rtl) = node.direction {
            return rtl;
        }
        match node.parent {
            NO_KEY => key != self.root && self.nodes[self.root as usize].rtl,
            parent => self.nodes[parent as usize].rtl,
        }
    }

    fn insert(
        &mut self,
        parent_id: NodeId,
        child_id: NodeId,
        before: Option<NodeId>,
    ) -> Result<(), ApplyError> {
        let parent = self.key(parent_id)?;
        let child = self.key(child_id)?;
        let before = before.map(|b| self.key(b)).transpose()?;
        if child == self.root || self.is_ancestor(child, parent) {
            return Err(ApplyError::Cycle(child_id));
        }
        if let Some(b) = before
            && (b == child || self.nodes[b as usize].parent != parent)
        {
            return Err(ApplyError::BadAnchor(self.nodes[b as usize].id));
        }

        let parent_created = self.nodes[parent as usize].created;
        if self.nodes[child as usize].parent != NO_KEY {
            self.unlink(child, !parent_created);
        }

        // Inside a block or a span. Other elements inside text are not
        // part of v1; `set_inline` leaves them out of layout.
        let inline = self.nodes[parent as usize].kind == Kind::Text;
        let pos = match before {
            Some(b) => self.nodes[parent as usize]
                .children
                .iter()
                .position(|&c| c == b)
                .expect("anchor is a child"),
            None => self.nodes[parent as usize].children.len(),
        };
        self.set_inline(child, inline);
        self.nodes[child as usize].parent = parent;
        self.nodes[parent as usize].children.insert(pos, child);
        self.update_direction(child);
        if inline {
            self.mark_text_block(parent);
            return Ok(());
        }
        // A cell is its list's child natively but a layout root of its own:
        // its size is measured and its frame placed by the list.
        let list_parent = self.nodes[parent as usize].kind == Kind::List;
        if list_parent {
            if self.nodes[child as usize].kind != Kind::Cell {
                log::warn!("a list takes `<cell>` children only");
            }
            if let Some(slot) = self.nodes[child as usize].slot
                && let Some(list) = self.nodes[parent as usize].list.as_mut()
            {
                list.add_slot_cell(slot, child);
                self.mark(parent, dirty::LIST);
            }
            if parent_created {
                self.ensure_created(child);
                let before = self.native_before(parent, child);
                self.mount.push(MountItem::Insert {
                    parent: self.nodes[parent as usize].id,
                    child: self.nodes[child as usize].id,
                    before,
                });
            }
            return Ok(());
        }
        if let Some(child_layout) = self.nodes[child as usize].layout {
            let parent_layout = self.nodes[parent as usize]
                .layout
                .expect("block parent has layout");
            self.layout
                .insert_child_at_index(parent_layout, pos, child_layout);
            self.needs_layout = true;
            if parent_created {
                self.ensure_created(child);
                let before = self.native_before(parent, child);
                self.mount.push(MountItem::Insert {
                    parent: self.nodes[parent as usize].id,
                    child: self.nodes[child as usize].id,
                    before,
                });
            }
        }
        Ok(())
    }

    fn unlink(&mut self, key: Key, emit_remove: bool) {
        let parent = self.nodes[key as usize].parent;
        let siblings = &mut self.nodes[parent as usize].children;
        let pos = siblings
            .iter()
            .position(|&c| c == key)
            .expect("child of parent");
        siblings.remove(pos);
        self.nodes[key as usize].parent = NO_KEY;
        let (inline, layout, created, id) = (
            self.nodes[key as usize].inline,
            self.nodes[key as usize].layout,
            self.nodes[key as usize].created,
            self.nodes[key as usize].id,
        );
        if inline {
            self.mark_text_block(parent);
            return;
        }
        if let Some(layout) = layout {
            if self.nodes[parent as usize].kind == Kind::List {
                if self.nodes[key as usize].kind == Kind::Cell
                    && let Some(list) = self.nodes[parent as usize].list.as_mut()
                {
                    list.remove_cell(key);
                    self.mark(parent, dirty::LIST);
                }
            } else {
                let parent_layout = self.nodes[parent as usize]
                    .layout
                    .expect("block parent has layout");
                self.layout.remove_child(parent_layout, layout);
                self.needs_layout = true;
            }
            if emit_remove && created && self.nodes[parent as usize].created {
                self.mount.push(MountItem::Remove { id });
            }
        }
    }

    fn release(&mut self, id: NodeId) -> Result<(), ApplyError> {
        let key = self.key(id)?;
        if key == self.root {
            return Ok(());
        }
        if self.nodes[key as usize].parent != NO_KEY {
            self.unlink(key, false);
        }
        let node = self.nodes.remove(key as usize);
        self.slots[id as usize] = 0;
        self.env_styled.remove(&id);
        self.lists.retain(|&k| k != key);
        self.list_binds.retain(|b| b.list != id);
        for &c in &node.children {
            if let Some(child) = self.nodes.get_mut(c as usize) {
                child.parent = NO_KEY;
            }
        }
        if let Some(layout) = node.layout {
            self.layout.remove(layout);
        }
        if node.created {
            self.mount.push(MountItem::Delete { id });
        }
        Ok(())
    }

    fn is_ancestor(&self, ancestor: Key, mut key: Key) -> bool {
        while key != NO_KEY {
            if key == ancestor {
                return true;
            }
            key = self.nodes[key as usize].parent;
        }
        false
    }

    /// The native sibling `child` goes below: native views are stacked by
    /// `zIndex`, then in tree order.
    fn native_before(&self, parent: Key, child: Key) -> Option<NodeId> {
        let children = &self.nodes[parent as usize].children;
        let z = self.nodes[child as usize].z_index;
        let pos = children.iter().position(|&c| c == child)?;
        let mut best: Option<(i32, usize, NodeId)> = None;
        for (i, &c) in children.iter().enumerate() {
            let n = &self.nodes[c as usize];
            if c == child || !n.created {
                continue;
            }
            if (n.z_index, i) > (z, pos) && best.is_none_or(|b| (n.z_index, i) < (b.0, b.1)) {
                best = Some((n.z_index, i, n.id));
            }
        }
        best.map(|b| b.2)
    }

    /// `key`'s `zIndex` changed: moves its native view among its siblings.
    fn restack(&mut self, key: Key) {
        let node = &self.nodes[key as usize];
        let (parent, id) = (node.parent, node.id);
        if parent == NO_KEY || !node.created || node.inline {
            return;
        }
        if !self.nodes[parent as usize].created {
            return;
        }
        let before = self.native_before(parent, key);
        self.mount.push(MountItem::Insert {
            parent: self.nodes[parent as usize].id,
            child: id,
            before,
        });
    }

    /// Moves `key` (and its subtree) into or out of an inline context.
    fn set_inline(&mut self, key: Key, inline: bool) {
        let node = &mut self.nodes[key as usize];
        if node.inline == inline {
            return;
        }
        node.inline = inline;
        if node.kind != Kind::Text {
            // Other elements inside text are not part of v1; they are ignored.
            return;
        }
        if inline {
            // Block -> span: spans have no layout node or native view.
            if let Some(l) = node.layout.take() {
                self.layout.remove(l);
            }
            if node.created {
                node.created = false;
                self.mount.push(MountItem::Delete { id: node.id });
            }
        } else {
            let mut style = self.nodes[key as usize]
                .base_layout
                .as_deref()
                .cloned()
                .unwrap_or_default();
            if let Some((_, applied)) = self.nodes[key as usize].env.as_deref() {
                add_insets(&mut style, applied[0], applied[1]);
            }
            let cx = LeafContext::Text(TextContext::new(Arc::default()));
            let layout = self.layout.new_leaf_with_context(style, cx);
            self.layout.set_tag(layout, key);
            self.nodes[key as usize].layout = Some(layout);
            self.mark(key, dirty::TEXT_BUILD);
        }
    }

    /// Creates the native views of `key`'s subtree if needed, wiring up the
    /// children in order.
    fn ensure_created(&mut self, key: Key) {
        let node = &mut self.nodes[key as usize];
        if node.created || node.layout.is_none() {
            return;
        }
        node.created = true;
        node.frame = Bounds::UNSET;
        let id = node.id;
        let kind = node.kind;
        let mut flags = 0;
        if node.view_style != ViewStyle::default() {
            flags |= dirty::VIEW;
        }
        if !node.animated.is_empty() {
            flags |= dirty::ANIMATION;
        }
        if !node.events.is_empty() {
            flags |= dirty::EVENTS;
        }
        if kind == Kind::Text {
            flags |= dirty::TEXT_MOUNT;
        }
        if node.image.as_ref().is_some_and(|source| source.is_some()) {
            flags |= dirty::IMAGE;
        }
        if matches!(kind, Kind::Scroll | Kind::List) {
            // A new native view has not been told its content size.
            node.scroll.as_mut().expect("scroll").content = None;
            flags |= dirty::SCROLL;
        }
        if let Some(input) = node.input.as_mut() {
            input.content = None;
            flags |= dirty::INPUT;
            if !input.value.is_empty() {
                flags |= dirty::INPUT_VALUE;
            }
        }
        let slots = node
            .state
            .iter()
            .enumerate()
            .fold(node.state_dirty, |slots, (i, paint)| {
                if paint.is_some() {
                    slots | (1 << i)
                } else {
                    slots
                }
            });
        if slots != 0 {
            node.state_dirty = slots;
            flags |= dirty::STATE;
        }
        if node.test_id.is_some() {
            flags |= dirty::TEST_ID;
        }
        if !node.accessibility.is_default() {
            flags |= dirty::ACCESSIBILITY;
        }
        self.mount.push(MountItem::Create {
            id,
            kind: kind.view_kind(),
        });
        self.mark(key, flags);
        if !kind.is_container() {
            return;
        }
        let mut children = mem::take(&mut self.nodes[key as usize].children);
        let original = children.clone();
        if children
            .iter()
            .any(|&c| self.nodes[c as usize].z_index != 0)
        {
            children.sort_by_key(|&c| self.nodes[c as usize].z_index);
        }
        for &child in &children {
            if self.nodes[child as usize].layout.is_none() {
                continue;
            }
            self.ensure_created(child);
            let child_id = self.nodes[child as usize].id;
            self.mount.push(MountItem::Insert {
                parent: id,
                child: child_id,
                before: None,
            });
        }
        self.nodes[key as usize].children = original;
    }

    fn mark(&mut self, key: Key, flags: u32) {
        if flags == 0 {
            return;
        }
        let node = &mut self.nodes[key as usize];
        node.dirty |= flags;
        if !node.queued {
            node.queued = true;
            self.queue.push(key);
        }
    }

    fn flush_dirty(&mut self) {
        let queue = mem::take(&mut self.queue);
        for key in queue {
            let (mut flags, has_layout) = {
                let Some(node) = self.nodes.get_mut(key as usize) else {
                    continue;
                };
                let flags = mem::take(&mut node.dirty);
                node.queued = false;
                (flags, node.layout.is_some())
            };
            if flags & dirty::TEXT_BUILD != 0 && has_layout && self.rebuild_text(key) {
                flags |= dirty::TEXT_MOUNT;
            }
            let node = &self.nodes[key as usize];
            if !node.created {
                continue;
            }
            let id = node.id;
            if flags & dirty::VIEW != 0 {
                let style = self.mounted_view_style(key);
                self.mount.push(MountItem::ViewStyle { id, style });
            }
            if flags & dirty::ANIMATION != 0 {
                let bindings = self.nodes[key as usize].animated.clone();
                self.mount
                    .push(MountItem::AnimationBindings { id, bindings });
            }
            if flags & dirty::EVENTS != 0 {
                let events = self.nodes[key as usize].events;
                self.mount.push(MountItem::Events { id, events });
            }
            if flags & dirty::IMAGE != 0
                && let Some(image) = &self.nodes[key as usize].image
            {
                let resize_mode = self.nodes[key as usize].object_fit.unwrap_or_default();
                let ratio = self.pixel_ratio;
                self.mount.push(MountItem::Image {
                    id,
                    source: image.as_ref().map(|s| s.resolved(ratio)),
                    resize_mode,
                });
            }
            if flags & dirty::SCROLL != 0
                && let Some(scroll) = self.nodes[key as usize].scroll.as_ref()
            {
                self.mount.push(MountItem::ScrollConfig {
                    id,
                    horizontal: scroll.horizontal,
                    taps: scroll.taps,
                    dismiss: scroll.dismiss,
                });
            }
            if flags & dirty::TEXT_MOUNT != 0 {
                self.queue_text(key);
            }
            if flags & (dirty::INPUT | dirty::INPUT_VALUE) != 0 {
                self.emit_input(key, id, flags);
            }
            if flags & dirty::SELECTION != 0 {
                let selection = self.nodes[key as usize]
                    .input
                    .as_mut()
                    .and_then(|input| input.selection.take());
                if let Some((start, end)) = selection {
                    self.mount.push(MountItem::Command {
                        id,
                        command: Command::SetSelection { start, end },
                    });
                }
            }
            if flags & dirty::STATE != 0 {
                let slots = mem::take(&mut self.nodes[key as usize].state_dirty);
                for slot in 0..3 {
                    if slots & (1 << slot) != 0 {
                        self.emit_state(key, id, slot);
                    }
                }
            }
            if flags & dirty::TEST_ID != 0 {
                let test_id = self.nodes[key as usize].test_id.clone();
                self.mount.push(MountItem::TestId { id, test_id });
            }
            if flags & dirty::ACCESSIBILITY != 0 {
                let accessibility = Box::new(self.nodes[key as usize].accessibility.clone());
                self.mount
                    .push(MountItem::Accessibility { id, accessibility });
            }
        }
    }

    /// The view style as platforms take it: translations by a fraction of
    /// the size resolved with the current frame.
    fn mounted_view_style(&self, key: Key) -> Box<ViewStyle> {
        let node = &self.nodes[key as usize];
        let mut style = Box::new(node.view_style.clone());
        if style.transform.has_fractions() {
            let size = if node.frame.size.width.is_nan() {
                Size::default()
            } else {
                node.frame.size
            };
            style.transform = style.transform.resolve_fractions(size);
        }
        style
    }

    /// One pointer state's paint, with fractional translations resolved
    /// against the current frame. A default style clears the state.
    fn emit_state(&mut self, key: Key, id: NodeId, slot: usize) {
        let node = &self.nodes[key as usize];
        let (view, text) = match &node.state[slot] {
            Some(paint) => {
                let mut view = paint.view.clone();
                if view.transform.has_fractions() {
                    let size = if node.frame.size.width.is_nan() {
                        Size::default()
                    } else {
                        node.frame.size
                    };
                    view.transform = view.transform.resolve_fractions(size);
                }
                (view, paint.text)
            }
            None => (ViewStyle::default(), TextStyle::default()),
        };
        self.mount.push(MountItem::StateStyle {
            id,
            interaction: Interaction::from_index(slot),
            view: Box::new(view),
            text,
        });
    }

    fn positioning_parent(&self, layout: LayoutId) -> Option<LayoutId> {
        let mut parent = self.layout.parent(layout)?;
        match self.layout.style(layout).position {
            Position::Absolute => {
                while !self.layout.style(parent).position.is_positioned() {
                    let Some(above) = self.layout.parent(parent) else {
                        break;
                    };
                    parent = above;
                }
            }
            Position::Fixed => {
                while let Some(above) = self.layout.parent(parent) {
                    parent = above;
                }
            }
            _ => {}
        }
        Some(parent)
    }

    fn layout_origin(&self, layout: LayoutId) -> taffy::Point<f32> {
        let mut above = Vec::new();
        let mut at = self.positioning_parent(layout);
        while let Some(node) = at {
            above.push(node);
            at = self.positioning_parent(node);
        }
        above
            .iter()
            .rev()
            .fold(taffy::Point::ZERO, |origin, &node| {
                let l = self.layout.unrounded_layout(node);
                taffy::Point {
                    x: origin.x + l.location.x,
                    y: origin.y + l.location.y,
                }
            })
    }

    /// `layout`'s layout, rounded to whole points as Taffy's rounding pass
    /// does ([`rounded`]).
    fn rounded_layout(&self, layout: LayoutId) -> taffy::Layout {
        rounded(
            self.layout.unrounded_layout(layout),
            self.layout_origin(layout),
        )
    }

    fn layout_main(&mut self) -> bool {
        let root = self.nodes[self.root as usize]
            .layout
            .expect("root has a layout");
        let available = taffy::Size {
            width: AvailableSpace::Definite(self.viewport.width.0),
            height: AvailableSpace::Definite(self.viewport.height.0),
        };
        let main = self.layout.dirty(root);
        if main {
            let engine = &self.text_engine;
            let cache = &mut self.measure_cache;
            self.layout
                .compute_layout_with_measure(root, available, |input, _, cx, style| {
                    measure_leaf(engine, cache, input, cx, style)
                });
        }
        main
    }

    fn relayout_boundaries(&mut self) -> Vec<Key> {
        let engine = &self.text_engine;
        let cache = &mut self.measure_cache;
        self.layout.relayout_boundaries(|input, _, cx, style| {
            measure_leaf(engine, cache, input, cx, style)
        })
    }

    fn displays_none(&self, key: Key) -> bool {
        let mut k = key;
        while k != NO_KEY {
            if let Some(layout) = self.nodes[k as usize].layout
                && self.layout.style(layout).display == taffy::Display::None
            {
                return true;
            }
            k = self.nodes[k as usize].parent;
        }
        false
    }

    fn emit_frames(&mut self, key: Key) {
        let Some(layout) = self.nodes[key as usize].layout else {
            return;
        };
        let parent = self.nodes[key as usize].parent;
        let hidden = parent != NO_KEY && self.displays_none(parent);
        let origin = self
            .layout
            .parent(layout)
            .map_or(taffy::Point::ZERO, |parent| {
                let origin = self.layout_origin(parent);
                let location = self.layout.unrounded_layout(parent).location;
                taffy::Point {
                    x: origin.x + location.x,
                    y: origin.y + location.y,
                }
            });
        self.emit_frames_in(key, hidden, origin);
    }

    fn emit_frames_in(&mut self, key: Key, hidden: bool, origin: taffy::Point<f32>) {
        let Some(layout) = self.nodes[key as usize].layout else {
            return;
        };
        if !self.nodes[key as usize].created {
            return;
        }
        let hidden = hidden || self.layout.style(layout).display == taffy::Display::None;
        if hidden {
            self.set_frame(key, Bounds::default());
            let children = mem::take(&mut self.nodes[key as usize].children);
            for &child in &children {
                self.emit_frames_in(child, true, origin);
            }
            self.nodes[key as usize].children = children;
            return;
        }
        let unrounded = *self.layout.unrounded_layout(layout);
        let containing = if self.layout.style(layout).position.is_out_of_flow() {
            self.layout_origin(layout)
        } else {
            origin
        };
        let inner = taffy::Point {
            x: containing.x + unrounded.location.x,
            y: containing.y + unrounded.location.y,
        };
        let l = rounded(&unrounded, containing);
        let mut frame = rect(
            l.location.x + round(containing.x) - round(origin.x),
            l.location.y + round(containing.y) - round(origin.y),
            l.size.width,
            l.size.height,
        );
        // Children of a right-to-left horizontal scroll view are mirrored
        // in its content. The view is laid out left to right, so a row
        // does not overflow to negative offsets.
        let parent = self.nodes[key as usize].parent;
        if parent != NO_KEY {
            let parent = &self.nodes[parent as usize];
            if parent.rtl
                && parent.kind == Kind::Scroll
                && let Some(content) = parent
                    .scroll
                    .as_ref()
                    .filter(|scroll| scroll.horizontal)
                    .and_then(|scroll| scroll.content)
            {
                frame.origin.x = content.width - frame.origin.x - frame.size.width;
            }
        }
        self.set_frame(key, frame);
        if self.nodes[key as usize].kind == Kind::Input {
            let content = content_box(&l);
            let node = &mut self.nodes[key as usize];
            let input = node.input.as_mut().expect("input state");
            if input.content != Some(content) {
                input.content = Some(content);
                self.mount.push(MountItem::InputContent {
                    id: node.id,
                    content,
                });
            }
        }
        if self.nodes[key as usize].kind == Kind::Scroll {
            // Scrollable extent. Taffy 0.15 already adds a scroll container's
            // end padding to `scrollable_overflow_rect`: the rect is measured
            // from the padding edge, stops at the children's border boxes,
            // then grows by that padding. Adding it again made the content
            // long by the padding (a safe-area `env()` included). The leading
            // border is still outside the rect.
            let o = l.scrollable_overflow_rect;
            let size = crate::geometry::size_pt(
                l.size.width.max(l.border.left + o.right),
                l.size.height.max(l.border.top + o.bottom),
            );
            self.set_scroll_content(key, size);
        }
        let children = mem::take(&mut self.nodes[key as usize].children);
        for &child in &children {
            self.emit_frames_in(child, false, inner);
        }
        self.nodes[key as usize].children = children;
    }

    fn set_frame(&mut self, key: Key, frame: Bounds<Points>) {
        let (id, is_text, restyle, fractional) = {
            let node = &mut self.nodes[key as usize];
            if node.frame == frame {
                return;
            }
            node.frame = frame;
            let fractional: Vec<usize> = node
                .state
                .iter()
                .enumerate()
                .filter_map(|(slot, paint)| {
                    paint
                        .as_ref()
                        .is_some_and(|paint| paint.view.transform.has_fractions())
                        .then_some(slot)
                })
                .collect();
            (
                node.id,
                node.kind == Kind::Text,
                node.view_style.transform.has_fractions(),
                fractional,
            )
        };
        self.mount.push(MountItem::Frame { id, frame });
        if restyle {
            let style = self.mounted_view_style(key);
            self.mount.push(MountItem::ViewStyle { id, style });
        }
        for slot in fractional {
            self.emit_state(key, id, slot);
        }
        // The content box is inside this frame. A size or padding change
        // does not rebuild the string, so the text is sent again from here.
        if is_text {
            self.queue_text(key);
        }
    }

    /// Re-resolves the direction of `key` (moved, or its parent's changed)
    /// and of the nodes inheriting it.
    fn update_direction(&mut self, key: Key) {
        let rtl = self.resolved_rtl(key);
        if self.nodes[key as usize].rtl == rtl {
            return;
        }
        self.flip_direction(key, rtl);
        self.update_children_direction(key);
    }

    fn update_children_direction(&mut self, key: Key) {
        let children = self.nodes[key as usize].children.clone();
        for child in children {
            self.update_direction(child);
        }
    }

    /// Lays `key` out in the other direction.
    fn flip_direction(&mut self, key: Key, rtl: bool) {
        let direction = if rtl {
            taffy::Direction::Rtl
        } else {
            taffy::Direction::Ltr
        };
        self.nodes[key as usize].rtl = rtl;
        let kind = self.nodes[key as usize].kind;
        let layout_id = self.nodes[key as usize].layout;
        if matches!(kind, Kind::Scroll | Kind::List) {
            if let Some(base) = self.nodes[key as usize].base_layout.as_mut() {
                base.direction = direction;
            }
            self.sync_scroll_style(key);
        } else if let Some(l) = layout_id {
            let mut layout = self.layout.style(l).clone();
            layout.direction = direction;
            self.layout.set_style(l, layout);
            self.needs_layout = true;
        }
        self.mark(key, dirty::VIEW);
        if kind == Kind::Text {
            self.mark_text_block(key);
        }
        if kind == Kind::Input {
            self.measure_input(key);
            self.mark(key, dirty::INPUT);
        }
    }

    /// Marks the text block containing `key` (or `key` itself) for rebuild.
    fn mark_text_block(&mut self, mut key: Key) {
        loop {
            let node = &self.nodes[key as usize];
            if !node.inline {
                if node.kind == Kind::Text {
                    self.mark(key, dirty::TEXT_BUILD);
                }
                return;
            }
            if node.parent == NO_KEY {
                return;
            }
            key = node.parent;
        }
    }

    /// Text blocks inside `key` inherit its new text style.
    fn mark_inheriting_text(&mut self, key: Key) {
        let mut stack: Vec<Key> = self.nodes[key as usize].children.clone();
        while let Some(k) = stack.pop() {
            match self.nodes[k as usize].kind {
                Kind::Text => self.mark_text_block(k),
                Kind::Input => {
                    self.measure_input(k);
                    self.mark(k, dirty::INPUT);
                }
                Kind::RawText => {}
                _ => stack.extend_from_slice(&self.nodes[k as usize].children),
            }
        }
    }

    /// Rebuilds the text an input is measured with: its value, else its
    /// placeholder, else an empty line.
    fn measure_input(&mut self, key: Key) {
        let node = &self.nodes[key as usize];
        let (Some(input), Some(layout)) = (&node.input, node.layout) else {
            return;
        };
        let placeholder = input.value.is_empty() && !input.config.placeholder.is_empty();
        let text = self.input_text(
            key,
            if placeholder {
                &input.config.placeholder
            } else {
                &input.value
            },
        );
        let cx = self.text_cx(layout);
        let same = cx.text.layout_eq(&text);
        let input = self.nodes[key as usize]
            .input
            .as_mut()
            .expect("input state");
        // A value that reads as the placeholder measures the same, but
        // it is not the placeholder: laid out again so the two stay distinct.
        if same && input.placeholder_measured == placeholder {
            return;
        }
        input.placeholder_measured = placeholder;
        if !same {
            *self.text_cx(layout) = TextContext::new(Arc::new(text));
        }
        self.layout.mark_dirty(layout);
        self.needs_layout = true;
    }

    /// `content` in the text style of input `key`, as it is measured. A
    /// single line has `max_lines` 1; several wrap at the width, and an
    /// empty string or a trailing line break still takes a line.
    fn input_text(&self, key: Key, content: &str) -> AttributedText {
        let node = &self.nodes[key as usize];
        let multiline = node.input.as_ref().is_some_and(|i| i.config.multiline);
        let mut text = AttributedText {
            max_lines: if multiline { 0 } else { 1 },
            ..AttributedText::default()
        };
        let style = self.inherited_text_style(key).inherit(&node.text_style);
        if multiline {
            text.push(&broken_runs(content), style);
        } else {
            text.push(content, style);
        }
        if content.is_empty() || (multiline && content.ends_with('\n')) {
            text.push("\u{200b}", style);
        }
        text
    }

    fn emit_input(&mut self, key: Key, id: NodeId, flags: u32) {
        let text_style = self.nodes[key as usize].text_style;
        let rtl = self.nodes[key as usize].rtl;
        let style = self.inherited_text_style(key).inherit(&text_style);
        let align = directed_align(
            text_style.text_align.unwrap_or_default(),
            rtl && !text_style.text_align_physical,
        );
        let writing = text_style.writing_direction.unwrap_or(rtl);
        let config = (flags & dirty::INPUT != 0).then(|| {
            let input = self.nodes[key as usize]
                .input
                .as_ref()
                .expect("input state");
            let mut config = input.config.clone();
            config.style = style;
            config.align = align;
            config.rtl = writing;
            config
        });
        let value = (flags & dirty::INPUT_VALUE != 0).then(|| {
            let input = self.nodes[key as usize]
                .input
                .as_ref()
                .expect("input state");
            (input.value.clone(), input.count)
        });
        if let Some(config) = config {
            self.mount.push(MountItem::Input {
                id,
                config: Box::new(config),
            });
        }
        if let Some((text, count)) = value {
            self.mount.push(MountItem::InputValue { id, text, count });
        }
    }

    fn text_cx(&mut self, layout: LayoutId) -> &mut TextContext<M::Prepared> {
        match self.layout.get_node_context_mut(layout) {
            Some(LeafContext::Text(cx)) => cx,
            _ => unreachable!("text block without text context"),
        }
    }

    /// The style text inherits at the root: React Native's defaults.
    fn base_text_style(&self) -> ResolvedTextStyle {
        ResolvedTextStyle::default()
    }

    /// The text style `key` inherits from the views around it, as CSS
    /// does: `color` on a container styles the text in it.
    fn inherited_text_style(&self, key: Key) -> ResolvedTextStyle {
        let mut chain: Vec<Key> = Vec::new();
        let mut k = self.nodes[key as usize].parent;
        while k != NO_KEY {
            let node = &self.nodes[k as usize];
            if node.kind != Kind::Text && node.text_style != TextStyle::default() {
                chain.push(k);
            }
            k = node.parent;
        }
        chain
            .iter()
            .rev()
            .fold(self.base_text_style(), |style, &k| {
                style.inherit(&self.nodes[k as usize].text_style)
            })
    }

    /// The nearest `textAlign` set on a view around `key`.
    fn inherited_text_align(&self, key: Key) -> Option<(TextAlign, bool)> {
        let mut k = self.nodes[key as usize].parent;
        while k != NO_KEY {
            let node = &self.nodes[k as usize];
            if let Some(align) = node.text_style.text_align {
                return Some((align, node.text_style.text_align_physical));
            }
            k = node.parent;
        }
        None
    }

    /// Rebuilds the attributed text of a block. Returns true if it changed.
    ///
    /// Only layout-relevant changes invalidate measurement and layout; a
    /// color change keeps both and just needs a new final layout.
    fn rebuild_text(&mut self, key: Key) -> bool {
        let node = &self.nodes[key as usize];
        let max_lines = node.number_of_lines.unwrap_or(0);
        let ellipsize = node.ellipsize.unwrap_or_default();
        let inherited = self.inherited_text_style(key);
        let (align, physical) = node
            .text_style
            .text_align
            .map(|align| (align, node.text_style.text_align_physical))
            .or_else(|| self.inherited_text_align(key))
            .unwrap_or_default();
        let mut text = AttributedText {
            align: directed_align(align, node.rtl && !physical),
            writing_direction: node.text_style.writing_direction,
            rtl: node.rtl,
            max_lines,
            ellipsize,
            shadow: node.text_style.shadow(),
            ..AttributedText::default()
        };
        let base = inherited.inherit(&node.text_style);
        self.collect_text(key, base, &mut text);
        let layout = self.nodes[key as usize]
            .layout
            .expect("text block has layout");
        let same_layout = {
            let cx = self.text_cx(layout);
            if *cx.text == text {
                return false;
            }
            // A test id is not a glyph: keep the layout and remount the text.
            if cx.text.test_ids != text.test_ids && cx.text.rendering_eq(&text) {
                cx.text = Arc::new(text);
                cx.metadata_changed = true;
                return true;
            }
            cx.metadata_changed = false;
            let same_layout = cx.text.layout_eq(&text);
            cx.text = Arc::new(text);
            cx.prepared = None;
            cx.min_content = None;
            cx.laid_out = None;
            if !same_layout {
                cx.layout_hash = cx.text.layout_hash();
                cx.cache = None;
            }
            same_layout
        };
        if !same_layout {
            let engine = &self.text_engine;
            let cache = &mut self.measure_cache;
            if !self
                .layout
                .measures_as_before(layout, |input, _, cx, style| {
                    measure_leaf(engine, cache, input, cx, style)
                })
            {
                self.layout.mark_dirty(layout);
                self.needs_layout = true;
            }
        }
        true
    }

    fn collect_text(&self, key: Key, style: ResolvedTextStyle, out: &mut AttributedText) {
        for &c in &self.nodes[key as usize].children {
            let child = &self.nodes[c as usize];
            match child.kind {
                Kind::RawText => out.push(&child.text, style),
                Kind::Text => {
                    let start = out.len_utf16();
                    let mut inner = style.inherit(&child.text_style);
                    if let Some(background) = span_background(&child.view_style, child.id) {
                        inner.background = Some(background);
                    }
                    let events = child.events;
                    let id = child.id;
                    let test_id = child.test_id.clone();
                    self.collect_text(c, inner, out);
                    if events.contains(EventKind::Press) {
                        out.links.push(crate::text::TextLink {
                            start,
                            end: out.len_utf16(),
                            node: id,
                            events,
                        });
                    }
                    if let Some(test_id) = test_id {
                        out.test_ids.push(SpanTestId {
                            start,
                            end: out.len_utf16(),
                            test_id,
                        });
                    }
                }
                // Inline views are not part of v1.
                _ => {}
            }
        }
    }

    #[inline]
    fn queue_text(&mut self, key: Key) {
        let node = &mut self.nodes[key as usize];
        if !node.text_queued {
            node.text_queued = true;
            self.pending_text.push(key);
        }
    }

    /// Makes the final layout of every queued text block whose content or
    /// box changed, and emits it.
    fn flush_text(&mut self) {
        let pending = mem::take(&mut self.pending_text);
        for key in pending {
            let (align_vertical, id, layout) = {
                let Some(node) = self.nodes.get_mut(key as usize) else {
                    continue;
                };
                node.text_queued = false;
                let align_vertical = node.text_style.align_vertical.unwrap_or_default();
                if !node.created {
                    continue;
                }
                let Some(layout) = node.layout else {
                    continue;
                };
                (align_vertical, node.id, layout)
            };
            let hidden = self.displays_none(key);
            let mut content = if hidden {
                Bounds::default()
            } else {
                content_box(&self.rounded_layout(layout))
            };
            let engine = &self.text_engine;
            let cache = &mut self.measure_cache;
            let Some(LeafContext::Text(cx)) = self.layout.get_node_context_mut(layout) else {
                continue;
            };
            if hidden {
                // Out of the layout (`display: none`). Glyphs typeset in
                // its zero box would still show where a platform does not
                // clip text to its box, so it gets an empty layout; the
                // text itself stays what it is.
                if matches!(&cx.laid_out, Some(l) if l.content == content) {
                    continue;
                }
                let text_layout =
                    engine.layout(&engine.prepare(&AttributedText::default()), content.size);
                cx.laid_out = Some(LaidText {
                    content,
                    layout: text_layout.clone(),
                });
                self.mount.push(MountItem::Text {
                    id,
                    text: cx.text.clone(),
                    layout: text_layout,
                    content,
                });
                continue;
            }
            if cx.text.no_wrap {
                content.size.width = content
                    .size
                    .width
                    .max(measure_text(engine, cache, cx, None).width);
            }
            if align_vertical > Percentage(0.0) {
                let height = measure_text(engine, cache, cx, Some(content.size.width.0)).height;
                if height < content.size.height {
                    content.origin.y += align_vertical.of(content.size.height - height);
                    content.size.height = height;
                }
            }
            if cx.metadata_changed {
                let reuse = cx
                    .laid_out
                    .as_ref()
                    .filter(|laid| laid.content == content)
                    .map(|laid| laid.layout.clone());
                if let Some(layout) = reuse {
                    let text = cx.text.clone();
                    cx.metadata_changed = false;
                    self.mount.push(MountItem::Text {
                        id,
                        text,
                        layout,
                        content,
                    });
                    continue;
                }
                cx.metadata_changed = false;
            }
            let size = content.size;
            let text_layout = match &cx.laid_out {
                Some(l) if l.content == content => continue,
                Some(l) if l.content.size == size => l.layout.clone(),
                _ => {
                    let prepared = cx.prepared.get_or_insert_with(|| engine.prepare(&cx.text));
                    engine.layout(prepared, size)
                }
            };
            cx.laid_out = Some(LaidText {
                content,
                layout: text_layout.clone(),
            });
            self.mount.push(MountItem::Text {
                id,
                text: cx.text.clone(),
                layout: text_layout,
                content,
            });
        }
    }
}

fn measure_leaf<M: TextEngine>(
    engine: &M,
    cache: &mut MeasureCache,
    input: taffy::LayoutInput,
    cx: Option<&mut LeafContext<M::Prepared>>,
    style: &TaffyStyle,
) -> taffy::LayoutOutput {
    taffy::compute_leaf_layout(
        input,
        style,
        |_, _| 0.0,
        |known, available| match cx {
            Some(LeafContext::Text(cx)) => {
                let max_width = if cx.text.no_wrap {
                    None
                } else {
                    known.width.or(match available.width {
                        AvailableSpace::Definite(w) => Some(w),
                        AvailableSpace::MinContent => Some(min_content_width(engine, cache, cx).0),
                        AvailableSpace::MaxContent => None,
                    })
                };
                let measured = measure_text(engine, cache, cx, max_width);
                taffy::Size {
                    width: known.width.unwrap_or(measured.width.0),
                    height: known.height.unwrap_or(measured.height.0),
                }
            }
            Some(LeafContext::Image(Some(natural))) => {
                image_size(*natural, known.width, known.height)
            }
            _ => taffy::Size::ZERO,
        },
    )
}

/// Size of an image with a known natural size: fill a known dimension and
/// keep the aspect ratio for the other.
fn image_size(natural: Size<Points>, w: Option<f32>, h: Option<f32>) -> taffy::Size<f32> {
    let ratio = if natural.height > Points::ZERO {
        natural.width / natural.height
    } else {
        1.0
    };
    let (width, height) = match (w, h) {
        (Some(w), None) => (w, w / ratio),
        (None, Some(h)) => (h * ratio, h),
        (Some(w), Some(h)) => (w, h),
        (None, None) => (natural.width.0, natural.height.0),
    };
    taffy::Size { width, height }
}
/// The content box (inside padding and border) of a laid-out node.
fn content_box(l: &taffy::Layout) -> Bounds<Points> {
    rect(
        l.padding.left + l.border.left,
        l.padding.top + l.border.top,
        (l.size.width - l.padding.left - l.padding.right - l.border.left - l.border.right).max(0.0),
        (l.size.height - l.padding.top - l.padding.bottom - l.border.top - l.border.bottom)
            .max(0.0),
    )
}

/// Min-content keeps every unbreakable segment intact. Measure the styled
/// segments with the platform engine rather than estimating glyph advances.
fn min_content_width<M: TextEngine>(
    engine: &M,
    cache: &mut MeasureCache,
    cx: &mut TextContext<M::Prepared>,
) -> Points {
    if let Some(width) = cx.min_content {
        return width;
    }
    let text = &cx.text;
    let mut offsets = Vec::with_capacity(text.text.chars().count() + 1);
    let mut utf16 = 0u32;
    for (byte, ch) in text.text.char_indices() {
        offsets.push((utf16, byte));
        utf16 += ch.len_utf16() as u32;
    }
    offsets.push((utf16, text.text.len()));
    let byte_at = |offset: u32| {
        offsets[offsets
            .binary_search_by_key(&offset, |&(at, _)| at)
            .unwrap()]
        .1
    };
    let mut start = 0usize;
    let mut width = Points::ZERO;
    for (end, _) in unicode_linebreak::linebreaks(&text.text) {
        let word = text.text[start..end].trim_end_matches([' ', '\t', '\n', '\r', '\u{000c}']);
        let from = offsets[offsets
            .binary_search_by_key(&start, |&(_, byte)| byte)
            .unwrap()]
        .0;
        let to = from + word.encode_utf16().count() as u32;
        if from < to {
            let mut part = AttributedText {
                no_wrap: true,
                rtl: text.rtl,
                writing_direction: text.writing_direction,
                ..Default::default()
            };
            for span in &text.spans {
                let (lo, hi) = (from.max(span.start), to.min(span.end));
                if lo < hi {
                    part.push(&text.text[byte_at(lo)..byte_at(hi)], span.style);
                }
            }
            for attachment in &text.attachments {
                if (from..to).contains(&attachment.index) {
                    let mut attachment = *attachment;
                    attachment.index -= from;
                    part.attachments.push(attachment);
                }
            }
            let hash = part.layout_hash();
            let measured = cache.get(hash, None).unwrap_or_else(|| {
                let measured = engine.measure(&engine.prepare(&part), None);
                cache.insert(hash, None, measured);
                measured
            });
            width = width.max(measured.width);
        }
        start = end;
    }
    cx.min_content = Some(width);
    width
}

fn measure_text<M: TextEngine>(
    engine: &M,
    cache: &mut MeasureCache,
    cx: &mut TextContext<M::Prepared>,
    max_width: Option<f32>,
) -> Size<Points> {
    if let Some((w, size)) = cx.cache
        && w == max_width
    {
        return size;
    }
    let typed = max_width.map(Points);
    let size = match cache.get(cx.layout_hash, typed) {
        Some(size) => size,
        None => {
            let prepared = cx.prepared.get_or_insert_with(|| engine.prepare(&cx.text));
            let size = engine.measure(prepared, typed);
            // Round up so rendering never truncates the last glyph.
            let size = size.map(Points::ceil);
            cache.insert(cx.layout_hash, typed, size);
            size
        }
    };
    cx.cache = Some((max_width, size));
    size
}

/// `align` in a layout of direction `rtl`: left and right trade places
/// right to left, as all left / right properties do.
fn directed_align(align: TextAlign, rtl: bool) -> TextAlign {
    match (align, rtl) {
        (TextAlign::Left, true) => TextAlign::Right,
        (TextAlign::Right, true) => TextAlign::Left,
        (align, _) => align,
    }
}

/// The background a nested text's view style gives its characters.
fn span_background(style: &ViewStyle, node: NodeId) -> Option<crate::text::TextBackground> {
    (style.background_color.0 & 0xff != 0).then_some(crate::text::TextBackground {
        color: style.background_color,
        radius: style.border_radius.max(Points::ZERO),
        node,
    })
}

fn root_style(viewport: Size<Points>) -> TaffyStyle {
    let mut style = TaffyStyle {
        size: taffy::Size {
            width: taffy::Dimension::length(viewport.width.0),
            height: taffy::Dimension::length(viewport.height.0),
        },
        ..Style::default().layout
    };
    style::resolve_normal_alignment(&mut style);
    style
}

/// What `env` comes to now: the surface safe area and the keyboard height.
/// Left and right stay physical.
fn env_value(safe: Edges<Points>, keyboard: Points, var: style::EnvVar) -> Points {
    use style::EnvVar;
    match var {
        EnvVar::KeyboardInsetHeight => keyboard.max(Points::ZERO),
        EnvVar::SafeAreaInsetTop => safe.top,
        EnvVar::SafeAreaInsetRight => safe.right,
        EnvVar::SafeAreaInsetBottom => safe.bottom,
        EnvVar::SafeAreaInsetLeft => safe.left,
    }
}

/// The points `term` is now.
fn env_term(safe: Edges<Points>, keyboard: Points, term: style::EnvTerm) -> Points {
    use style::EnvTerm;
    match term {
        EnvTerm::Var(var) => env_value(safe, keyboard, var),
        EnvTerm::Max(a, b) => env_value(safe, keyboard, a).max(env_value(safe, keyboard, b)),
    }
}

/// Adds point insets to padding and margin lengths. Percentages and `auto`
/// margins cannot take an addition, as in nsh.
fn add_insets(layout: &mut TaffyStyle, padding: Edges<Points>, margin: Edges<Points>) {
    let add_pad = |p: &mut taffy::LengthPercentage, d: f32| {
        if d == 0.0 {
            return;
        }
        if let taffy::style::ExpandedLengthPercentage::Length(v) = p.expand() {
            *p = taffy::LengthPercentage::length(v + d);
        }
    };
    add_pad(&mut layout.padding.top, padding.top.0);
    add_pad(&mut layout.padding.right, padding.right.0);
    add_pad(&mut layout.padding.bottom, padding.bottom.0);
    add_pad(&mut layout.padding.left, padding.left.0);
    let add_margin = |m: &mut taffy::LengthPercentageAuto, d: f32| {
        if d == 0.0 {
            return;
        }
        if let taffy::style::ExpandedLengthPercentageAuto::Length(v) = m.expand() {
            *m = taffy::LengthPercentageAuto::length(v + d);
        }
    };
    add_margin(&mut layout.margin.top, margin.top.0);
    add_margin(&mut layout.margin.right, margin.right.0);
    add_margin(&mut layout.margin.bottom, margin.bottom.0);
    add_margin(&mut layout.margin.left, margin.left.0);
}

/// Replaces insets `was` with `now` on a style that already has `was` added.
fn shift_insets(layout: &mut TaffyStyle, was: [Edges<Points>; 2], now: [Edges<Points>; 2]) {
    let delta = |now: Edges<Points>, was: Edges<Points>| Edges {
        top: now.top - was.top,
        right: now.right - was.right,
        bottom: now.bottom - was.bottom,
        left: now.left - was.left,
    };
    add_insets(layout, delta(now[0], was[0]), delta(now[1], was[1]));
}

/// Taffy's rounding of a coordinate.
fn round(value: f32) -> f32 {
    (value + 0.5).floor()
}

/// `layout` rounded as Taffy's rounding pass (`taffy::round_layout`) rounds
/// it, `origin` being the unrounded position of the node around it in its
/// layout root: edges are rounded where they are in the root, so adjacent
/// views stay adjacent. The core rounds itself: Taffy's pass reaches the
/// absolutely positioned nodes through their containing block's list,
/// which it can leave holding removed nodes (a panic) or never fill (a
/// hidden subtree's), while here every node is reached through its parent;
/// out-of-flow offsets use their Taffy containing block.
fn rounded(layout: &Layout, origin: taffy::Point<f32>) -> Layout {
    let mut out = *layout;
    let (x, y) = (origin.x + layout.location.x, origin.y + layout.location.y);
    out.location.x = round(x) - round(origin.x);
    out.location.y = round(y) - round(origin.y);
    out.size.width = round(x + layout.size.width) - round(x);
    out.size.height = round(y + layout.size.height) - round(y);
    out.scrollbar_size.width = round(layout.scrollbar_size.width);
    out.scrollbar_size.height = round(layout.scrollbar_size.height);
    out.border.left = round(x + layout.border.left) - round(x);
    out.border.right =
        round(x + layout.size.width) - round(x + layout.size.width - layout.border.right);
    out.border.top = round(y + layout.border.top) - round(y);
    out.border.bottom =
        round(y + layout.size.height) - round(y + layout.size.height - layout.border.bottom);
    out.padding.left = round(x + layout.padding.left) - round(x);
    out.padding.right =
        round(x + layout.size.width) - round(x + layout.size.width - layout.padding.right);
    out.padding.top = round(y + layout.padding.top) - round(y);
    out.padding.bottom =
        round(y + layout.size.height) - round(y + layout.size.height - layout.padding.bottom);
    let o = layout.scrollable_overflow_rect;
    out.scrollable_overflow_rect.left = round(x + o.left) - round(x);
    out.scrollable_overflow_rect.right = round(x + o.right) - round(x);
    out.scrollable_overflow_rect.top = round(y + o.top) - round(y);
    out.scrollable_overflow_rect.bottom = round(y + o.bottom) - round(y);
    out
}

impl<M: TextEngine> Core<M> {
    /// The taffy style of a scroll view: children overflow on its axis, and a
    /// horizontal one lays them out in a row. Right to left, that row is laid
    /// out left to right (padding and border swapped back) and
    /// [`Core::emit_frames_in`] mirrors the children: a right-to-left row would
    /// overflow to negative offsets.
    fn sync_scroll_style(&mut self, key: Key) {
        let (layout, mut style, horizontal, rtl) = {
            let node = &self.nodes[key as usize];
            let (Some(layout), Some(base)) = (node.layout, node.base_layout.as_deref()) else {
                return;
            };
            let horizontal = node.scroll.as_ref().is_some_and(|scroll| scroll.horizontal);
            (layout, base.clone(), horizontal, node.rtl)
        };
        // On the physical edges, then swapped with the padding when a
        // right-to-left row is laid out left to right.
        if let Some((_, applied)) = self.nodes[key as usize].env.as_deref() {
            add_insets(&mut style, applied[0], applied[1]);
        }
        if horizontal {
            style.flex_direction = taffy::FlexDirection::Row;
            style.overflow.x = taffy::Overflow::Scroll;
            if rtl {
                mem::swap(&mut style.padding.left, &mut style.padding.right);
                mem::swap(&mut style.border.left, &mut style.border.right);
                style.direction = taffy::Direction::Ltr;
            }
        } else {
            style.overflow.y = taffy::Overflow::Scroll;
        }
        self.layout.set_style(layout, style);
        self.needs_layout = true;
    }

    /// Lays out the list cells that changed: items at their column's
    /// width, the header, footer and empty placeholder at the full width.
    fn layout_cells(&mut self) {
        for i in 0..self.lists.len() {
            let key = self.lists[i];
            let Some(list_layout) = self.nodes.get(key as usize).and_then(|n| n.layout) else {
                continue;
            };
            if !self.nodes[key as usize].created {
                continue;
            }
            let size = self.rounded_layout(list_layout).size;
            let cross = size.width;
            let state = self.nodes[key as usize].list.as_mut().expect("list");
            let resized = state.cells_cross != cross;
            state.cells_cross = cross;
            let item_cross = state.column(cross, 0).1;
            let extra_cross = state.full_width(cross).1;
            let cells = std::mem::take(&mut self.nodes[key as usize].children);
            for &cell in &cells {
                let node = &self.nodes[cell as usize];
                let Some(cell_layout) = node.layout else {
                    continue;
                };
                let width = match node.slot {
                    Some(CellSlot::Item(_)) | None => item_cross,
                    Some(_) => extra_cross,
                };
                if resized || self.layout.dirty(cell_layout) {
                    let available = taffy::Size {
                        width: taffy::AvailableSpace::Definite(width),
                        height: taffy::AvailableSpace::MaxContent,
                    };
                    let engine = &self.text_engine;
                    let cache = &mut self.measure_cache;
                    self.layout.compute_layout_with_measure(
                        cell_layout,
                        available,
                        |input, _, cx, style| measure_leaf(engine, cache, input, cx, style),
                    );
                    self.nodes[cell as usize].relaid = true;
                }
            }
            self.nodes[key as usize].children = cells;
        }
    }

    /// Positions the rows of every list: measures the cells that show
    /// their item, follows a scroll command's target and asks the runtime
    /// for the rows the window needs.
    fn update_lists(&mut self) {
        for i in 0..self.lists.len() {
            let key = self.lists[i];
            if !self.nodes[key as usize].created {
                continue;
            }
            let viewport = self.nodes[key as usize]
                .layout
                .map(|layout| self.rounded_layout(layout).size.height)
                .unwrap_or(0.0);
            let cross = self.nodes[key as usize]
                .layout
                .map(|layout| self.rounded_layout(layout).size.width)
                .unwrap_or(0.0);
            self.nodes[key as usize]
                .list
                .as_mut()
                .expect("list")
                .viewport = viewport;

            // Sizes of cells showing their item, and of the extras.
            let (mut ready, extras) = {
                let list = self.nodes[key as usize].list.as_ref().expect("list");
                (
                    list.slots
                        .iter()
                        .filter_map(|s| s.ready().map(|i| (i, s.cell)))
                        .collect::<Vec<_>>(),
                    list.extras,
                )
            };
            let mut anchor = 0.0;
            let mut scrolls: Vec<(f32, bool)> = Vec::new();
            for &(index, cell) in &ready {
                if let Some(cell_layout) = self.nodes.get(cell as usize).and_then(|c| c.layout) {
                    let size = self.rounded_layout(cell_layout).size.height;
                    let list = self.nodes[key as usize].list.as_mut().expect("list");
                    anchor += list.measured(index, size);
                }
            }
            for (kind, extra) in [
                list::ExtraKind::Header,
                list::ExtraKind::Footer,
                list::ExtraKind::Empty,
            ]
            .into_iter()
            .zip(extras)
            {
                if let Some(cell_layout) =
                    self.nodes.get(extra.cell as usize).and_then(|c| c.layout)
                {
                    let size = self.rounded_layout(cell_layout).size.height;
                    let list = self.nodes[key as usize].list.as_mut().expect("list");
                    anchor += list.measured_extra(kind, size);
                }
            }
            {
                let list = self.nodes[key as usize].list.as_mut().expect("list");
                list.offset += anchor;
                anchor += list.take_splice_anchor();
                if let Some(target) = list.follow_pin() {
                    scrolls.push((target, true));
                }
            }

            // Scroll commands for the list, now that sizes are known.
            let id = self.nodes[key as usize].id;
            let mut commands = Vec::new();
            for command in &self.commands {
                if command.0 == id {
                    commands.push(command.1.clone());
                }
            }
            self.commands.retain(|c| c.0 != id);
            for command in commands {
                let (target, animated) = match command {
                    Command::ScrollToIndex { index, animated } => {
                        (list::PinTarget::Index(index), animated)
                    }
                    Command::ScrollToEnd { animated } => (list::PinTarget::End, animated),
                    _ => continue,
                };
                let list = self.nodes[key as usize].list.as_mut().expect("list");
                let offset = list.target_offset(target);
                list.pin(Some(target), offset);
                list.jump(offset);
                scrolls.push((offset, animated));
            }

            // Content can shrink below the offset (the last page cleared):
            // follow the platform's clamp before placing the window.
            let content_len = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .content_len();
            let max_offset = (content_len - viewport).max(0.0);
            if self.nodes[key as usize].list.as_ref().expect("list").offset > max_offset {
                let list = self.nodes[key as usize].list.as_mut().expect("list");
                list.jump(max_offset);
                scrolls.push((max_offset, false));
            }

            let mut binds = Vec::new();
            let load_more = {
                let list = self.nodes[key as usize].list.as_mut().expect("list");
                list.update_window(&mut binds)
            };
            if load_more {
                self.events.push((id, EventKind::LoadMore));
            }
            if !binds.is_empty() {
                self.list_binds.push(ListBind { list: id, binds });
            }

            ready.clear();
            ready.extend(
                self.nodes[key as usize]
                    .list
                    .as_ref()
                    .expect("list")
                    .slots
                    .iter()
                    .filter_map(|s| s.ready().map(|i| (i, s.cell))),
            );
            let (window_start, window_end) = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .render_window();
            let hidden = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .hidden_y();
            let items_changed = self.nodes[key as usize]
                .list
                .as_mut()
                .expect("list")
                .items_changed;
            // Cells without an item can still show their last one: move
            // those whose old place is in the window out of sight.
            let unbound: Vec<Key> = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .slots
                .iter()
                .filter(|s| s.ready().is_none() && s.cell != u32::MAX)
                .map(|s| s.cell)
                .collect();
            for cell in unbound {
                let frame = self.nodes[cell as usize].frame;
                if frame.size.width.is_nan() {
                    continue;
                }
                let (start, size) = (frame.origin.y.0, frame.size.height.0);
                if (items_changed || (start + size > window_start && start < window_end))
                    && frame.origin.y.0 != hidden
                {
                    let away = Bounds {
                        origin: Point::new(frame.origin.x, Points(hidden)),
                        ..frame
                    };
                    self.set_frame(cell, away);
                }
            }
            // Place the cells that show their item.
            for &(index, cell) in &ready {
                let Some(cell_layout) = self.nodes.get(cell as usize).and_then(|c| c.layout) else {
                    continue;
                };
                let size = self.rounded_layout(cell_layout).size;
                let (column, width) = {
                    let list = self.nodes[key as usize].list.as_ref().expect("list");
                    list.column(cross, list.column_of(index))
                };
                let at = self.nodes[key as usize]
                    .list
                    .as_ref()
                    .expect("list")
                    .item_at(index);
                let frame = rect(column, at, width, size.height);
                self.set_frame(cell, frame);
                if std::mem::take(&mut self.nodes[cell as usize].relaid) {
                    let children = std::mem::take(&mut self.nodes[cell as usize].children);
                    for &child in &children {
                        self.emit_frames(child);
                    }
                    self.nodes[cell as usize].children = children;
                }
            }
            // Header, footer and the empty placeholder.
            let (blocks, count) = {
                let list = self.nodes[key as usize].list.as_ref().expect("list");
                (list.blocks(), list.count)
            };
            let (full_x, full_width) = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .full_width(cross);
            let ats = [blocks.header, blocks.footer, blocks.body];
            for (i, extra) in extras.iter().enumerate() {
                if extra.cell == u32::MAX {
                    continue;
                }
                let cell = extra.cell;
                if i == 2 && count > 0 {
                    // The empty placeholder shows only without items.
                    let frame = self.nodes[cell as usize].frame;
                    if !frame.size.width.is_nan() && frame.origin.y.0 != hidden {
                        let away = Bounds {
                            origin: Point::new(frame.origin.x, Points(hidden)),
                            ..frame
                        };
                        self.set_frame(cell, away);
                    }
                    continue;
                }
                if let Some(cell_layout) = self.nodes.get(cell as usize).and_then(|c| c.layout) {
                    let size = self.rounded_layout(cell_layout).size;
                    let frame = rect(full_x, ats[i], full_width, size.height);
                    self.set_frame(cell, frame);
                    if std::mem::take(&mut self.nodes[cell as usize].relaid) {
                        let children = std::mem::take(&mut self.nodes[cell as usize].children);
                        for &child in &children {
                            self.emit_frames(child);
                        }
                        self.nodes[cell as usize].children = children;
                    }
                }
            }
            self.nodes[key as usize]
                .list
                .as_mut()
                .expect("list")
                .items_changed = false;

            let content = Size {
                width: Points(cross),
                height: Points(content_len),
            };
            let old_len = self.nodes[key as usize]
                .scroll
                .as_ref()
                .and_then(|s| s.content)
                .map_or(0.0, |c| c.height.0);
            if anchor != 0.0 {
                if content_len >= old_len {
                    self.set_scroll_content(key, content);
                    self.mount.push(MountItem::ScrollBy {
                        id,
                        dx: Points::ZERO,
                        dy: Points(anchor),
                    });
                } else {
                    self.mount.push(MountItem::ScrollBy {
                        id,
                        dx: Points::ZERO,
                        dy: Points(anchor),
                    });
                    self.set_scroll_content(key, content);
                }
                self.nodes[key as usize]
                    .list
                    .as_mut()
                    .expect("list")
                    .issue_scroll_by(anchor);
            } else {
                self.set_scroll_content(key, content);
            }
            // The content is final before a scroll command: platforms
            // clamp an offset to the content they know.
            for (offset, animated) in scrolls {
                self.nodes[key as usize]
                    .list
                    .as_mut()
                    .expect("list")
                    .issue_scroll_to(offset);
                self.mount.push(MountItem::Command {
                    id,
                    command: Command::ScrollTo {
                        x: Points::ZERO,
                        y: Points(offset),
                        animated,
                    },
                });
            }
        }
    }

    fn set_scroll_content(&mut self, key: Key, size: Size<Points>) {
        let (id, rtl, kind, viewport, horizontal, old, content_item) = {
            let node = &mut self.nodes[key as usize];
            let Some(scroll) = node.scroll.as_mut() else {
                return;
            };
            if scroll.content == Some(size) {
                return;
            }
            let old = scroll.content.replace(size);
            (
                node.id,
                node.rtl,
                node.kind,
                node.frame.size.width,
                scroll.horizontal,
                old,
                scroll.content_item,
            )
        };
        // Items are only inserted before it since (a `ScrollBy`), so it is
        // at its index or later.
        let queued = content_item.and_then(|i| {
            let at = self.mount.get(i..)?.iter().position(
                |item| matches!(item, MountItem::ScrollContent { id: queued, .. } if *queued == id),
            )?;
            Some(i + at)
        });
        if let Some(j) = queued {
            self.mount[j] = MountItem::ScrollContent { id, size };
        }
        // A scroll queued after it must still come before the new size.
        let scrolled_since = queued.is_some_and(|j| {
            self.mount[j + 1..].iter().any(
                |item| matches!(item, MountItem::ScrollBy { id: scrolled, .. } if *scrolled == id),
            )
        });
        if queued.is_none() || scrolled_since {
            let at = self.mount.len();
            self.mount.push(MountItem::ScrollContent { id, size });
            if let Some(scroll) = self.nodes[key as usize].scroll.as_mut() {
                scroll.content_item = Some(at);
            }
        }
        // Right to left, a horizontal scroll view starts at its right end
        // and keeps its distance from it as the content changes.
        if kind == Kind::Scroll && horizontal && rtl {
            match old {
                None => self.mount.push(MountItem::Command {
                    id,
                    command: Command::ScrollTo {
                        x: (size.width - viewport).max(Points::ZERO),
                        y: Points::ZERO,
                        animated: false,
                    },
                }),
                Some(old) if old.width != size.width => {
                    self.mount.push(MountItem::ScrollBy {
                        id,
                        dx: size.width - old.width,
                        dy: Points::ZERO,
                    });
                }
                Some(_) => {}
            }
        }
    }
}

impl<M: TextEngine> Core<M> {
    fn env_insets(&self, env: &EnvEdges) -> [Edges<Points>; 2] {
        let edge = |term: Option<style::EnvTerm>| {
            term.map_or(Points::ZERO, |term| {
                env_term(self.safe_area, self.keyboard_inset, term)
            })
        };
        let edges = |terms: &[Option<style::EnvTerm>; 4]| Edges {
            top: edge(terms[0]),
            right: edge(terms[1]),
            bottom: edge(terms[2]),
            left: edge(terms[3]),
        };
        [edges(&env.padding), edges(&env.margin)]
    }

    /// Restyles nodes whose `env()` terms changed value. Nothing to do
    /// without such nodes. No layout animation: v1 applies the keyboard
    /// inset on the next commit.
    fn sync_env(&mut self) {
        if self.env_styled.is_empty() {
            return;
        }
        let ids: Vec<NodeId> = self.env_styled.iter().copied().collect();
        let mut stale = Vec::new();
        for id in ids {
            let Ok(key) = self.key(id) else {
                stale.push(id);
                continue;
            };
            let Some(env) = self.nodes[key as usize].env.as_deref().map(|(env, _)| *env) else {
                stale.push(id);
                continue;
            };
            let now = self.env_insets(&env);
            let was = self.nodes[key as usize].env.as_ref().expect("env").1;
            if now == was {
                continue;
            }
            self.nodes[key as usize].env.as_mut().expect("env").1 = now;
            if self.nodes[key as usize].kind == Kind::Scroll {
                self.sync_scroll_style(key);
                continue;
            }
            let Some(layout) = self.nodes[key as usize].layout else {
                continue;
            };
            let mut style = self.layout.style(layout).clone();
            shift_insets(&mut style, was, now);
            self.layout.set_style(layout, style);
            self.needs_layout = true;
        }
        for id in stale {
            self.env_styled.remove(&id);
        }
    }
}

/// Shortens a long run without a line-break opportunity so that measuring a
/// multiline value stays linear in its length. A 474 KB minified JSON takes
/// seconds to measure at every edit; broken into runs of 1024 it is fast.
/// The input shows and reports its own text; the measured height can only
/// grow by a line per break, and text without such runs is measured as is.
fn broken_runs(content: &str) -> std::borrow::Cow<'_, str> {
    let mut broken: Option<String> = None;
    let (mut start, mut from) = (0, 0);
    for (end, _) in unicode_linebreak::linebreaks(content) {
        let mut run = 0;
        for (offset, c) in content[start..end].char_indices() {
            let i = start + offset;
            // The final character may itself provide the break (space or
            // newline). Do not introduce another line immediately before it.
            if run == MEASURED_RUN && i + c.len_utf8() < end {
                let broken = broken.get_or_insert_with(|| {
                    String::with_capacity(content.len() + content.len() / MEASURED_RUN)
                });
                broken.push_str(&content[from..i]);
                broken.push('\n');
                (run, from) = (0, i);
            }
            run += 1;
        }
        start = end;
    }
    match broken {
        Some(mut broken) => {
            broken.push_str(&content[from..]);
            broken.into()
        }
        None => content.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use taffy::{Dimension, Display, Position};

    use super::*;
    use crate::{
        geometry::{px, size, size_pt},
        ops::{EventKind, ImageScales, ImageSource, Interaction, KeyboardType, Prop, ResizeMode},
        style::{Color, StylePatch, StyleValue, TextStyle, ViewStyle},
        text::{AttributedText, TextEngine, TextLayout},
    };

    /// 8pt per character, 16pt per line, wrapping at `max_width`.
    struct FakeEngine;

    struct FakeLayout {
        chars: usize,
    }

    impl TextEngine for FakeEngine {
        type Prepared = usize;

        fn prepare(&self, text: &AttributedText) -> usize {
            text.text.chars().count()
        }

        fn measure(&self, chars: &usize, max_width: Option<Points>) -> Size<Points> {
            let w = *chars as f32 * 8.0;
            match max_width.map(|m| m.0) {
                Some(max) if w > max && max > 0.0 => size_pt(max, (w / max).ceil() * 16.0),
                _ => size_pt(w, 16.0),
            }
        }

        fn layout(&self, chars: &usize, _: Size<Points>) -> TextLayout {
            TextLayout(Arc::new(FakeLayout { chars: *chars }))
        }
    }

    /// [`FakeEngine`], except `max_lines` clamps the line count (0: none).
    struct ClampedEngine;

    impl TextEngine for ClampedEngine {
        type Prepared = (usize, u32);

        fn prepare(&self, text: &AttributedText) -> (usize, u32) {
            (text.text.chars().count(), text.max_lines)
        }

        fn measure(&self, prepared: &Self::Prepared, max_width: Option<Points>) -> Size<Points> {
            let (chars, max_lines) = *prepared;
            let w = chars as f32 * 8.0;
            let (width, mut lines) = match max_width.map(|m| m.0) {
                Some(max) if w > max && max > 0.0 => (max, (w / max).ceil()),
                _ => (w, 1.0),
            };
            if max_lines > 0 {
                lines = lines.min(max_lines as f32);
            }
            size_pt(width, lines * 16.0)
        }

        fn layout(&self, _: &Self::Prepared, _: Size<Points>) -> TextLayout {
            TextLayout(Arc::new(()))
        }
    }

    fn core() -> Core<FakeEngine> {
        Core::new(FakeEngine, size(px(100.0), px(200.0)))
    }

    fn sized(width: f32, height: f32) -> Style {
        let mut style = Style::default();
        style.layout.size = taffy::Size {
            width: Dimension::length(width),
            height: Dimension::length(height),
        };
        style
    }

    fn apply_style(core: &mut Core<FakeEngine>, id: NodeId, style: Style) {
        core.apply(Op::SetStyle {
            id,
            style: Box::new(style),
            patch: StylePatch::default(),
        })
        .unwrap();
    }

    fn frame(batch: &MountBatch, id: NodeId) -> Bounds<Points> {
        batch
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                MountItem::Frame { id: i, frame } if *i == id => Some(*frame),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no frame for {id}"))
    }

    fn list_core() -> Core<FakeEngine> {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::List,
        })
        .unwrap();
        apply_style(&mut c, 2, sized(100.0, 200.0));
        c.apply(Op::Insert {
            parent: 1,
            child: 2,
            before: None,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::EstimatedItemSize(Points(50.0)),
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Count(100),
        })
        .unwrap();
        c
    }

    fn cell_in_list(c: &mut Core<FakeEngine>, slot: u32, index: u32, id: NodeId) {
        c.apply(Op::Create { id, tag: Tag::Cell }).unwrap();
        c.apply(Op::SetProp {
            id,
            prop: Prop::Slot(CellSlot::Item(slot)),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: id,
            before: None,
        })
        .unwrap();
        c.apply(Op::Create {
            id: id + 1,
            tag: Tag::View,
        })
        .unwrap();
        apply_style(c, id + 1, sized(100.0, 20.0));
        c.apply(Op::Insert {
            parent: id,
            child: id + 1,
            before: None,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id,
            prop: Prop::CellIndex(index),
        })
        .unwrap();
    }

    #[test]
    fn a_list_binds_places_and_recycles_cell_roots() {
        let mut c = list_core();
        let _ = c.commit().expect("list");
        let mut binds = Vec::new();
        c.take_list_binds(&mut binds);
        assert_eq!(binds.len(), 1);
        let (slot, index) = binds[0].binds[0];
        assert_eq!(index, 0);
        cell_in_list(&mut c, slot, index, 3);
        let batch = c.commit().expect("cell");
        let placed = frame(&batch, 3);
        assert_eq!(placed.origin.x, Points(0.0));
        assert_eq!(placed.origin.y, Points(0.0));
        assert_eq!(placed.size.height, Points(20.0));
        // Scrolling moves the window; the same cell slot takes item 50.
        c.set_scroll_offset(2, Point::new(Points(0.0), Points(2500.0)));
        let _ = c.commit();
        let mut binds = Vec::new();
        c.take_list_binds(&mut binds);
        assert_eq!(binds[0].binds[0].1, 46);
        assert_eq!(binds[0].list, 2);
        assert!(binds[0].binds.iter().all(|b| b.1 >= 46));
    }

    #[test]
    fn a_scroll_to_index_resolves_to_a_content_offset_and_binds_there() {
        let mut c = list_core();
        c.apply(Op::Command {
            id: 2,
            command: Command::ScrollToIndex {
                index: 50,
                animated: false,
            },
        })
        .unwrap();
        let batch = c.commit().expect("list");
        let command = batch.items.iter().find_map(|item| match item {
            MountItem::Command {
                id: 2,
                command: Command::ScrollTo { y, animated, .. },
            } => Some((y.0, *animated)),
            _ => None,
        });
        assert_eq!(command, Some((2500.0, false)));
        let mut binds = Vec::new();
        c.take_list_binds(&mut binds);
        assert_eq!(binds[0].binds[0].1, 46);
    }

    #[test]
    fn column_flex_stacks_children_and_rounds_in_the_root() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        let mut top = sized(100.0, 20.0);
        top.layout.margin.left = taffy::LengthPercentageAuto::length(0.6);
        top.layout.size.width = Dimension::length(10.4);
        apply_style(&mut c, 2, top);
        c.apply(Op::Create {
            id: 3,
            tag: Tag::View,
        })
        .unwrap();
        apply_style(&mut c, 3, sized(100.0, 30.0));
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 3,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let top = frame(&batch, 2);
        let bottom = frame(&batch, 3);
        assert_eq!(top.origin.x, px(1.0));
        assert_eq!(top.size.width, px(10.0));
        assert_eq!(top.origin.y, px(0.0));
        assert_eq!(top.size.height, px(20.0));
        assert_eq!(bottom.origin.y, px(20.0));
        assert_eq!(bottom.size.height, px(30.0));
        assert!(c.commit().is_none());
    }

    #[test]
    fn a_grid_places_cells_in_document_order() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        let mut grid = sized(100.0, 80.0);
        grid.layout.display = Display::Grid;
        grid.layout.grid_template_columns = vec![
            taffy::style_helpers::length(40.0),
            taffy::style_helpers::length(60.0),
        ];
        grid.layout.grid_template_rows = vec![taffy::style_helpers::length(30.0)];
        apply_style(&mut c, 2, grid);
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        for id in [3, 4] {
            c.apply(Op::Create { id, tag: Tag::View }).unwrap();
            apply_style(&mut c, id, sized(10.0, 10.0));
            c.apply(Op::Insert {
                parent: 2,
                child: id,
                before: None,
            })
            .unwrap();
        }
        let batch = c.commit().unwrap();
        assert_eq!(
            frame(&batch, 3).origin,
            crate::geometry::point(px(0.0), px(0.0))
        );
        assert_eq!(
            frame(&batch, 4).origin,
            crate::geometry::point(px(40.0), px(0.0))
        );
    }

    #[test]
    fn an_absolute_child_is_placed_in_its_containing_block() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        let mut style = sized(10.0, 12.0);
        style.layout.position = Position::Absolute;
        style.layout.inset = taffy::Rect {
            top: taffy::style_helpers::length(4.0),
            left: taffy::style_helpers::length(6.0),
            right: taffy::style_helpers::auto(),
            bottom: taffy::style_helpers::auto(),
        };
        apply_style(&mut c, 2, style);
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let frame = frame(&batch, 2);
        assert_eq!(frame.origin, crate::geometry::point(px(6.0), px(4.0)));
        assert_eq!(frame.size, size(px(10.0), px(12.0)));
    }

    #[test]
    fn display_none_clears_the_frame() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        apply_style(&mut c, 2, sized(40.0, 20.0));
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let shown = c.commit().unwrap();
        assert_eq!(frame(&shown, 2).size, size(px(40.0), px(20.0)));
        let mut hidden = sized(40.0, 20.0);
        hidden.layout.display = Display::None;
        apply_style(&mut c, 2, hidden);
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 2), Bounds::default());
    }

    #[test]
    fn on_press_is_mounted_with_the_view() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        c.apply(Op::Listen {
            id: 2,
            event: EventKind::Press,
            enabled: true,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let events = batch.items.iter().find_map(|item| match item {
            MountItem::Events { id: 2, events } => Some(*events),
            _ => None,
        });
        assert_eq!(
            events,
            Some(
                EventMask::default() | {
                    let mut mask = EventMask::default();
                    mask.set(EventKind::Press, true);
                    mask
                }
            )
        );
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::Create {
                id: 2,
                kind: ViewKind::View
            }
        )));
    }

    #[test]
    fn a_higher_z_index_is_stacked_above() {
        let mut c = core();
        for id in [2, 3] {
            c.apply(Op::Create { id, tag: Tag::View }).unwrap();
            c.apply(Op::Insert {
                parent: ROOT_ID,
                child: id,
                before: None,
            })
            .unwrap();
        }
        c.commit().unwrap();
        let high = Style {
            z_index: 5,
            ..Default::default()
        };
        apply_style(&mut c, 3, high);
        let mid = Style {
            z_index: 1,
            ..Default::default()
        };
        apply_style(&mut c, 2, mid);
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::Insert {
                parent: ROOT_ID,
                child: 2,
                before: Some(3),
            }
        )));
    }

    #[test]
    fn remove_and_release_emit_in_order() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.commit().unwrap();
        c.apply(Op::Remove {
            parent: ROOT_ID,
            child: 2,
        })
        .unwrap();
        c.apply(Op::Release { id: 2 }).unwrap();
        let batch = c.commit().unwrap();
        let kinds: Vec<_> = batch
            .items
            .iter()
            .filter_map(|item| match item {
                MountItem::Remove { id: 2 } => Some("remove"),
                MountItem::Delete { id: 2 } => Some("delete"),
                _ => None,
            })
            .collect();
        assert_eq!(kinds, ["remove", "delete"]);
        assert_eq!(
            c.apply(Op::SetStyle {
                id: 2,
                style: Box::new(Style::default()),
                patch: StylePatch::default(),
            }),
            Err(ApplyError::UnknownNode(2))
        );
    }

    #[test]
    fn insert_rejects_a_cycle() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        c.apply(Op::Create {
            id: 3,
            tag: Tag::View,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        assert_eq!(
            c.apply(Op::Insert {
                parent: 3,
                child: 2,
                before: None,
            }),
            Err(ApplyError::Cycle(2))
        );
    }

    fn source(uri: &str, width: f32, height: f32, scales: &[f32]) -> ImageSource {
        ImageSource {
            uri: uri.into(),
            size: Some(size_pt(width, height)),
            scales: scales.iter().copied().collect(),
        }
    }

    #[test]
    fn an_image_uses_its_natural_size_and_aspect_ratio() {
        let mut c = Core::new(FakeEngine, size(px(320.0), px(480.0)));
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Image,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Source(Some(source("asset:/logo.png", 200.0, 100.0, &[]))),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.apply(Op::Create {
            id: 3,
            tag: Tag::Image,
        })
        .unwrap();
        let mut narrow = Style::default();
        narrow.set("width", StyleValue::Number(50.0)).unwrap();
        narrow
            .set("alignSelf", StyleValue::Str("flex-start"))
            .unwrap();
        apply_style(&mut c, 3, narrow);
        c.apply(Op::SetProp {
            id: 3,
            prop: Prop::Source(Some(source("asset:/logo.png", 200.0, 100.0, &[]))),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 3,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::Create {
                id: 2,
                kind: ViewKind::Image
            }
        )));
        assert_eq!(frame(&batch, 2).size, size(px(320.0), px(160.0)));
        assert_eq!(frame(&batch, 3).size, size(px(50.0), px(25.0)));
    }

    #[test]
    fn object_fit_is_mounted_on_the_image() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Image,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Source(Some(source("https://example.com/a.png", 40.0, 30.0, &[]))),
        })
        .unwrap();
        let mut style = Style::default();
        style.set("objectFit", StyleValue::Str("contain")).unwrap();
        apply_style(&mut c, 2, style);
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let fitted = batch.items.iter().rev().find_map(|item| match item {
            MountItem::Image {
                id: 2,
                resize_mode,
                source,
            } => Some((*resize_mode, source.as_ref().map(|s| s.uri.clone()))),
            _ => None,
        });
        assert_eq!(
            fitted,
            Some((
                ResizeMode::Contain,
                Some(std::sync::Arc::from("https://example.com/a.png"))
            ))
        );
    }

    #[test]
    fn a_bundled_image_picks_the_screen_density() {
        let mut c = core();
        c.set_pixel_ratio(2.0);
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Image,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Source(Some(source(
                "asset:/logo.png",
                40.0,
                30.0,
                &[1.0, 2.0, 2.5],
            ))),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let uri = batch.items.iter().find_map(|item| match item {
            MountItem::Image {
                id: 2,
                source: Some(source),
                ..
            } => Some(source.uri.clone()),
            _ => None,
        });
        assert_eq!(uri.as_deref(), Some("asset:/logo@2x.png"));
        assert_eq!(
            source("asset:/logo.png", 1.0, 1.0, &[1.0, 2.0, 2.5]).scales,
            ImageScales::NONE.with(1.0).with(2.0)
        );
        c.set_pixel_ratio(1.0);
        let batch = c.commit().unwrap();
        let uri = batch.items.iter().find_map(|item| match item {
            MountItem::Image {
                id: 2,
                source: Some(source),
                ..
            } => Some(source.uri.clone()),
            _ => None,
        });
        assert_eq!(uri.as_deref(), Some("asset:/logo.png"));
        c.set_pixel_ratio(1.0);
        assert!(c.commit().is_none());
    }

    #[test]
    fn animated_style_binds_properties_to_nodes() {
        use crate::animation::{AnimationOp, AnimationProp};
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: crate::ops::ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let mut style = sized(10.0, 10.0);
        style.animated = vec![
            (AnimationProp::Opacity, 7),
            (AnimationProp::BackgroundColor, 9),
            (AnimationProp::Transform(0), 11),
        ];
        apply_style(&mut c, 2, style);
        c.apply(Op::Animation(AnimationOp::CreateValue {
            node: 7,
            value: 0.5,
        }))
        .unwrap();
        let batch = c.commit().expect("a frame");
        let animation = batch
            .items
            .iter()
            .position(|item| matches!(item, MountItem::Animation(_)))
            .expect("the graph op");
        let bindings = batch
            .items
            .iter()
            .position(|item| matches!(item, MountItem::AnimationBindings { id: 2, .. }))
            .expect("the bindings");
        assert!(animation < bindings, "graph ops precede bindings");
        let MountItem::AnimationBindings { bindings, .. } = &batch.items[bindings] else {
            unreachable!()
        };
        assert_eq!(
            bindings,
            &[
                (AnimationProp::Opacity, 7),
                (AnimationProp::BackgroundColor, 9),
                (AnimationProp::Transform(0), 11),
            ]
        );

        let mut style = sized(10.0, 10.0);
        style.animated = vec![(AnimationProp::BorderColor, 13)];
        apply_style(&mut c, 2, style);
        let batch = c.commit().expect("a frame");
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::AnimationBindings { id: 2, bindings }
                if bindings == &[(AnimationProp::BorderColor, 13)]
        )));
    }

    fn mounted_text(batch: &MountBatch, id: NodeId) -> Arc<AttributedText> {
        batch
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                MountItem::Text { id: i, text, .. } if *i == id => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no text for {id}"))
    }

    #[test]
    fn a_text_block_mounts_its_run_and_stretches() {
        let mut c = Core::new(FakeEngine, size(px(320.0), px(480.0)));
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        c.apply(Op::Create {
            id: 3,
            tag: Tag::Text,
        })
        .unwrap();
        c.apply(Op::CreateText {
            id: 4,
            text: "hello".into(),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 3,
            child: 4,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        let mut padded = Style::default();
        padded.set("padding", StyleValue::Number(10.0)).unwrap();
        apply_style(&mut c, 2, padded);
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let creates: Vec<_> = batch
            .items
            .iter()
            .filter_map(|item| match item {
                MountItem::Create { id, kind } => Some((*id, *kind)),
                _ => None,
            })
            .collect();
        assert_eq!(creates, vec![(2, ViewKind::View), (3, ViewKind::Text)]);
        assert_eq!(mounted_text(&batch, 3).text, "hello");
        assert_eq!(
            frame(&batch, 2),
            crate::geometry::rect(0.0, 0.0, 320.0, 36.0)
        );
        assert_eq!(
            frame(&batch, 3),
            crate::geometry::rect(10.0, 10.0, 300.0, 16.0)
        );
        assert!(c.commit().is_none());
    }

    #[test]
    fn updating_text_emits_the_string_without_a_new_frame() {
        let mut c = Core::new(FakeEngine, size(px(320.0), px(480.0)));
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Text,
        })
        .unwrap();
        c.apply(Op::CreateText {
            id: 3,
            text: "a".into(),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.commit().unwrap();
        c.apply(Op::SetText {
            id: 3,
            text: "abc".into(),
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert_eq!(mounted_text(&batch, 2).text, "abc");
        assert_eq!(batch.items.len(), 1);
    }

    #[test]
    fn nested_text_inherits_and_overrides_span_style() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        let mut parent = Style::default();
        parent.set("color", StyleValue::Str("#00ff00")).unwrap();
        apply_style(&mut c, 2, parent);
        c.apply(Op::Create {
            id: 3,
            tag: Tag::Text,
        })
        .unwrap();
        c.apply(Op::CreateText {
            id: 4,
            text: "hi ".into(),
        })
        .unwrap();
        c.apply(Op::Create {
            id: 5,
            tag: Tag::Text,
        })
        .unwrap();
        let mut span = Style::default();
        span.set("color", StyleValue::Str("#ff0000")).unwrap();
        span.set("fontWeight", StyleValue::Str("bold")).unwrap();
        span.set("fontSize", StyleValue::Number(20.0)).unwrap();
        span.set("fontStyle", StyleValue::Str("italic")).unwrap();
        span.set("backgroundColor", StyleValue::Str("#0000ff"))
            .unwrap();
        apply_style(&mut c, 5, span);
        c.apply(Op::CreateText {
            id: 6,
            text: "there".into(),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 5,
            child: 6,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 3,
            child: 4,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 3,
            child: 5,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let text = mounted_text(&batch, 3);
        assert_eq!(text.text, "hi there");
        assert_eq!(text.spans.len(), 2);
        assert_eq!(
            text.spans[0].style.color,
            crate::Color::parse("#00ff00").unwrap()
        );
        assert!(!text.spans[0].style.italic);
        assert_eq!(text.spans[0].style.font_weight, 400);
        assert_eq!(
            text.spans[1].style.color,
            crate::Color::parse("#ff0000").unwrap()
        );
        assert_eq!(text.spans[1].style.font_weight, 700);
        assert_eq!(text.spans[1].style.font_size, px(20.0));
        assert!(text.spans[1].style.italic);
        assert_eq!(
            text.spans[1].style.background.unwrap().color,
            crate::Color::parse("#0000ff").unwrap()
        );
        assert!(!batch.items.iter().any(|item| matches!(
            item,
            MountItem::Create { id: 5, .. } | MountItem::Insert { child: 5, .. }
        )));
    }

    #[test]
    fn hidden_text_keeps_its_string_and_lays_out_nothing() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Text,
        })
        .unwrap();
        c.apply(Op::CreateText {
            id: 3,
            text: "check".into(),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        let mut hidden = Style::default();
        hidden.layout.display = Display::None;
        apply_style(&mut c, 2, hidden);
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let (text, chars) = batch
            .items
            .iter()
            .find_map(|item| match item {
                MountItem::Text {
                    id: 2,
                    text,
                    layout,
                    ..
                } => Some((
                    text.text.clone(),
                    layout.downcast::<FakeLayout>().unwrap().chars,
                )),
                _ => None,
            })
            .unwrap();
        assert_eq!(text, "check");
        assert_eq!(chars, 0);
        assert_eq!(frame(&batch, 2).size, size(px(0.0), px(0.0)));
    }

    #[test]
    fn number_of_lines_reaches_the_block() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Text,
        })
        .unwrap();
        c.apply(Op::CreateText {
            id: 3,
            text: "one two three four".into(),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        let mut clamped = Style::default();
        clamped
            .set("numberOfLines", StyleValue::Number(1.0))
            .unwrap();
        apply_style(&mut c, 2, clamped);
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        assert_eq!(mounted_text(&c.commit().unwrap(), 2).max_lines, 1);
    }

    #[test]
    fn a_frame_change_remounts_the_text_content_box() {
        let mut c = Core::new(FakeEngine, size(px(320.0), px(480.0)));
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Text,
        })
        .unwrap();
        c.apply(Op::CreateText {
            id: 3,
            text: "hello".into(),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.commit().unwrap();
        let mut padded = Style::default();
        padded.set("padding", StyleValue::Number(10.0)).unwrap();
        apply_style(&mut c, 2, padded);
        let batch = c.commit().unwrap();
        let content = batch.items.iter().rev().find_map(|item| match item {
            MountItem::Text { id: 2, content, .. } => Some(*content),
            _ => None,
        });
        assert_eq!(
            content,
            Some(crate::geometry::rect(10.0, 10.0, 300.0, 16.0))
        );
        assert_eq!(
            frame(&batch, 2),
            crate::geometry::rect(0.0, 0.0, 320.0, 36.0)
        );
    }

    fn scroll_content(batch: &MountBatch, id: NodeId) -> Size<Points> {
        batch
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                MountItem::ScrollContent { id: i, size } if *i == id => Some(*size),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no scroll content for {id}"))
    }

    #[test]
    fn a_vertical_scroll_reports_its_content_and_keeps_child_heights() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Scroll,
        })
        .unwrap();
        apply_style(&mut c, 2, sized(100.0, 50.0));
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        for id in [3, 4, 5] {
            c.apply(Op::Create { id, tag: Tag::View }).unwrap();
            apply_style(&mut c, id, sized(100.0, 40.0));
            c.apply(Op::Insert {
                parent: 2,
                child: id,
                before: None,
            })
            .unwrap();
        }
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::Create {
                id: 2,
                kind: ViewKind::Scroll
            }
        )));
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::ScrollConfig {
                id: 2,
                horizontal: false,
                ..
            }
        )));
        assert_eq!(scroll_content(&batch, 2), size_pt(100.0, 120.0));
        // Children are not shrunk to the scroll view's height.
        assert_eq!(frame(&batch, 5).origin.y, px(80.0));
        assert_eq!(frame(&batch, 5).size.height, px(40.0));
    }

    #[test]
    fn a_horizontal_scroll_lays_its_children_out_in_a_row() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Scroll,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Horizontal(true),
        })
        .unwrap();
        apply_style(&mut c, 2, sized(100.0, 40.0));
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        for id in [3, 4] {
            c.apply(Op::Create { id, tag: Tag::View }).unwrap();
            apply_style(&mut c, id, sized(80.0, 40.0));
            c.apply(Op::Insert {
                parent: 2,
                child: id,
                before: None,
            })
            .unwrap();
        }
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::ScrollConfig {
                id: 2,
                horizontal: true,
                ..
            }
        )));
        assert_eq!(scroll_content(&batch, 2), size_pt(160.0, 40.0));
        assert_eq!(frame(&batch, 4).origin.x, px(80.0));
    }

    #[test]
    fn scroll_keyboard_props_are_mounted_and_blur_is_a_command() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Scroll,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::KeyboardTaps(crate::KeyboardTaps::Handled),
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::KeyboardDismiss(crate::KeyboardDismiss::OnDrag),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.apply(Op::Command {
            id: ROOT_ID,
            command: Command::Blur,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::ScrollConfig {
                id: 2,
                taps: crate::KeyboardTaps::Handled,
                dismiss: crate::KeyboardDismiss::OnDrag,
                ..
            }
        )));
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::Command {
                command: Command::Blur,
                ..
            }
        )));
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::KeyboardTaps(crate::KeyboardTaps::Always),
        })
        .unwrap();
        // A later change is mounted again. Dismiss stays.
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::ScrollConfig {
                taps: crate::KeyboardTaps::Always,
                dismiss: crate::KeyboardDismiss::OnDrag,
                ..
            }
        )));
    }

    /// `overflow: "hidden"` clips in layout as well as in paint: what a box
    /// clips away must not grow an ancestor scroll view's content size.
    #[test]
    fn clipped_overflow_does_not_grow_scroll_content() {
        fn content_width(clips: bool) -> f32 {
            let mut c = core();
            c.apply(Op::Create {
                id: 2,
                tag: Tag::Scroll,
            })
            .unwrap();
            apply_style(&mut c, 2, sized(100.0, 80.0));
            c.apply(Op::Insert {
                parent: ROOT_ID,
                child: 2,
                before: None,
            })
            .unwrap();
            c.apply(Op::Create {
                id: 3,
                tag: Tag::View,
            })
            .unwrap();
            let mut boxed = sized(80.0, 40.0);
            boxed.view.overflow_hidden = clips;
            apply_style(&mut c, 3, boxed);
            c.apply(Op::Insert {
                parent: 2,
                child: 3,
                before: None,
            })
            .unwrap();
            c.apply(Op::Create {
                id: 4,
                tag: Tag::View,
            })
            .unwrap();
            let mut child = sized(150.0, 20.0);
            child.layout.position = Position::Absolute;
            child.layout.inset.left = taffy::LengthPercentageAuto::length(0.0);
            child.layout.inset.top = taffy::LengthPercentageAuto::length(0.0);
            apply_style(&mut c, 4, child);
            c.apply(Op::Insert {
                parent: 3,
                child: 4,
                before: None,
            })
            .unwrap();
            scroll_content(&c.commit().unwrap(), 2).width.0
        }
        // Visible: the 150pt child reaches the scroll view.
        assert_eq!(content_width(false), 150.0);
        // Clipped: the scroll view keeps its own width.
        assert_eq!(content_width(true), 100.0);
    }

    #[test]
    fn a_right_to_left_horizontal_scroll_starts_at_its_end() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Scroll,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Horizontal(true),
        })
        .unwrap();
        let mut scroll = sized(150.0, 50.0);
        scroll.direction = Some(true);
        scroll.logical.padding[0] = Some(taffy::LengthPercentage::length(10.0));
        apply_style(&mut c, 2, scroll);
        for id in [3, 4] {
            c.apply(Op::Create { id, tag: Tag::View }).unwrap();
            apply_style(&mut c, id, sized(100.0, 50.0));
            c.apply(Op::Insert {
                parent: 2,
                child: id,
                before: None,
            })
            .unwrap();
        }
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        // Content: 10 (start padding) + 200; the first child at the right end.
        assert_eq!(scroll_content(&batch, 2), size_pt(210.0, 50.0));
        assert_eq!(frame(&batch, 3).origin.x, px(210.0 - 10.0 - 100.0));
        assert_eq!(frame(&batch, 4).origin.x, px(0.0));
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::Command {
                id: 2,
                command: Command::ScrollTo { x, .. }
            } if *x == px(60.0)
        )));

        // Wider content keeps its distance from the right end.
        apply_style(&mut c, 3, sized(130.0, 50.0));
        let batch = c.commit().unwrap();
        assert_eq!(scroll_content(&batch, 2).width, px(240.0));
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::ScrollBy { id: 2, dx, .. } if *dx == px(30.0)
        )));
    }

    #[test]
    fn a_scroll_view_mounts_the_scroll_events_it_listens_to() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Scroll,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let kinds = [
            EventKind::Scroll,
            EventKind::ScrollEnd,
            EventKind::ScrollDragStart,
            EventKind::ScrollDragEnd,
            EventKind::MomentumStart,
            EventKind::MomentumEnd,
        ];
        for event in kinds {
            c.apply(Op::Listen {
                id: 2,
                event,
                enabled: true,
            })
            .unwrap();
        }
        let batch = c.commit().unwrap();
        let events = batch
            .items
            .iter()
            .find_map(|item| match item {
                MountItem::Events { id: 2, events } => Some(*events),
                _ => None,
            })
            .expect("scroll events");
        for event in kinds {
            assert!(events.contains(event));
        }
        assert!(!events.contains(EventKind::Press));
        assert_eq!(EventKind::from_prop("onScroll"), Some(EventKind::Scroll));
        assert_eq!(
            EventKind::from_prop("onScrollDragEnd"),
            Some(EventKind::ScrollDragEnd)
        );
        assert_eq!(EventKind::MomentumEnd.prop_name(), "onMomentumEnd");
        assert_eq!(
            EventKind::from_raw(EventKind::Scroll as u8),
            Some(EventKind::Scroll)
        );
        assert_eq!(
            EventKind::from_raw(EventKind::MomentumEnd as u8),
            Some(EventKind::MomentumEnd)
        );
        assert_eq!(EventKind::from_raw(99), None);
    }

    #[test]
    fn an_input_measures_its_placeholder_and_reports_its_content_box() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Input,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Placeholder("Name".into()),
        })
        .unwrap();
        let mut padded = Style::default();
        padded.set("padding", StyleValue::Number(6.0)).unwrap();
        apply_style(&mut c, 2, padded);
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        // One line (16pt) plus padding, stretched across the root.
        assert_eq!(frame(&batch, 2).size.height, px(28.0));
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::Input { id: 2, config }
                if config.placeholder == "Name"
                    && !config.secure
                    && !config.disabled
                    && config.keyboard == KeyboardType::Text
        )));
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::InputContent { id: 2, content }
                if *content == crate::geometry::rect(6.0, 6.0, 88.0, 16.0)
        )));
        // No text yet: nothing to send.
        assert!(
            !batch
                .items
                .iter()
                .any(|item| matches!(item, MountItem::InputValue { .. }))
        );
        assert_eq!(Tag::from_name("input"), Some(Tag::Input));
    }

    #[test]
    fn an_empty_input_is_one_line_tall() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Input,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        assert_eq!(frame(&c.commit().unwrap(), 2).size, size_pt(100.0, 16.0));
    }

    #[test]
    fn a_single_line_input_stays_one_line_when_its_text_wraps() {
        // 50 characters at 8pt in a 100pt box is four lines unless
        // `max_lines` is 1. [`FakeEngine`] ignores that, so this engine
        // does not.
        let mut c = Core::new(ClampedEngine, size(px(100.0), px(200.0)));
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Input,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Value("x".repeat(50)),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        assert_eq!(frame(&c.commit().unwrap(), 2).size.height, px(16.0));
    }

    #[test]
    fn a_multiline_input_wraps_instead_of_clamping_to_one_line() {
        let mut c = Core::new(ClampedEngine, size(px(100.0), px(200.0)));
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Input,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Multiline(true),
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Value("x".repeat(50)),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        assert_eq!(frame(&c.commit().unwrap(), 2).size.height, px(64.0));
    }

    #[test]
    fn selected_range_is_sent_after_the_value() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Input,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Value("hello".into()),
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Selection { start: 1, end: 3 },
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let value = batch
            .items
            .iter()
            .position(|item| matches!(item, MountItem::InputValue { id: 2, .. }))
            .expect("the value");
        let selection = batch
            .items
            .iter()
            .position(|item| {
                matches!(
                    item,
                    MountItem::Command {
                        id: 2,
                        command: Command::SetSelection { start: 1, end: 3 },
                    }
                )
            })
            .expect("the selection");
        assert!(value < selection, "the value is applied first");
    }

    #[test]
    fn broken_runs_break_a_long_unbroken_run() {
        let content = "x".repeat(MEASURED_RUN * 2 + 10);
        let broken = super::broken_runs(&content);
        assert_eq!(broken.matches('\n').count(), 2);
        assert_eq!(broken.chars().filter(|c| *c != '\n').count(), content.len());
        // Runs a browser would break are left alone.
        let text = "word ".repeat(600);
        assert_eq!(super::broken_runs(&text), text);
    }

    #[test]
    fn input_values_from_js_and_native_edits() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Input,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Placeholder("Name".into()),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.commit().unwrap();
        // The user types; JS echoes the same text: nothing goes back.
        c.input_changed(2, "ab", 2);
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Value("ab".into()),
        })
        .unwrap();
        assert!(c.commit().is_none());
        // JS transforms the text: it is sent with the edit count it is based on.
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Value("AB".into()),
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::InputValue { id: 2, text, count: 2 } if text == "AB"
        )));
    }

    #[test]
    fn an_input_mounts_secure_keyboard_disabled_and_inherited_style() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        let mut parent = Style::default();
        parent.set("color", StyleValue::Str("#00ff00")).unwrap();
        apply_style(&mut c, 2, parent);
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.apply(Op::Create {
            id: 3,
            tag: Tag::Input,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 3,
            prop: Prop::Secure(true),
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 3,
            prop: Prop::Keyboard(KeyboardType::Email),
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 3,
            prop: Prop::Disabled(true),
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 3,
            prop: Prop::Value("secret".into()),
        })
        .unwrap();
        c.apply(Op::Listen {
            id: 3,
            event: EventKind::Change,
            enabled: true,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        let config = batch
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                MountItem::Input { id: 3, config } => Some(config),
                _ => None,
            })
            .expect("input config");
        assert!(config.secure);
        assert!(config.disabled);
        assert_eq!(config.keyboard, KeyboardType::Email);
        assert_eq!(config.style.color, crate::Color::parse("#00ff00").unwrap());
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::InputValue { id: 3, text, count: 0 } if text == "secret"
        )));
        let events = batch
            .items
            .iter()
            .find_map(|item| match item {
                MountItem::Events { id: 3, events } => Some(*events),
                _ => None,
            })
            .expect("input events");
        assert!(events.contains(EventKind::Change));
        assert_eq!(EventKind::from_prop("onChange"), Some(EventKind::Change));
        assert_eq!(EventKind::Change.prop_name(), "onChange");
        assert_eq!(KeyboardType::from_name("phone"), Some(KeyboardType::Phone));
        assert_eq!(KeyboardType::from_name("url"), None);

        let mut own = Style::default();
        own.set("color", StyleValue::Str("#ff0000")).unwrap();
        own.set("textAlign", StyleValue::Str("center")).unwrap();
        own.direction = Some(true);
        apply_style(&mut c, 3, own);
        let batch = c.commit().unwrap();
        let config = batch
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                MountItem::Input { id: 3, config } => Some(config),
                _ => None,
            })
            .expect("restyled input");
        assert_eq!(config.style.color, crate::Color::parse("#ff0000").unwrap());
        assert_eq!(config.align, crate::style::TextAlign::Center);
        assert!(config.rtl);
    }

    #[test]
    fn keyboard_inset_pads_and_stops_when_the_term_is_replaced() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        let mut box_style = Style::default();
        box_style.set("height", StyleValue::Number(100.0)).unwrap();
        box_style
            .set(
                "paddingBottom",
                StyleValue::Str("env(keyboard-inset-height)"),
            )
            .unwrap();
        apply_style(&mut c, 2, box_style);
        c.apply(Op::Create {
            id: 3,
            tag: Tag::View,
        })
        .unwrap();
        let mut child = Style::default();
        child.set("flex", StyleValue::Number(1.0)).unwrap();
        apply_style(&mut c, 3, child);
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 3).size.height, px(100.0));

        c.set_keyboard_inset(px(40.0));
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 3).size.height, px(60.0));

        // A negative height is hidden.
        c.set_keyboard_inset(px(-8.0));
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 3).size.height, px(100.0));

        c.set_keyboard_inset(px(40.0));
        c.commit().unwrap();
        let mut plain = Style::default();
        plain.set("height", StyleValue::Number(100.0)).unwrap();
        plain
            .set("paddingBottom", StyleValue::Number(10.0))
            .unwrap();
        apply_style(&mut c, 2, plain);
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 3).size.height, px(90.0));
        c.set_keyboard_inset(Points::ZERO);
        assert!(c.commit().is_none());
    }

    #[test]
    fn safe_area_and_calc_add_to_padding_and_margin() {
        let mut c = core();
        c.set_safe_area(Edges {
            top: px(20.0),
            right: px(4.0),
            bottom: px(34.0),
            left: px(12.0),
        });
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        let mut padded = Style::default();
        padded
            .set(
                "paddingTop",
                StyleValue::Str("calc(env(safe-area-inset-top) + 8px)"),
            )
            .unwrap();
        padded
            .set(
                "paddingBottom",
                StyleValue::Str("env(safe-area-inset-bottom)"),
            )
            .unwrap();
        padded
            .set(
                "marginLeft",
                StyleValue::Str("calc(0.5rem + env(safe-area-inset-left))"),
            )
            .unwrap();
        apply_style(&mut c, 2, padded);
        c.apply(Op::Create {
            id: 3,
            tag: Tag::View,
        })
        .unwrap();
        apply_style(&mut c, 3, sized(10.0, 10.0));
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.apply(Op::Create {
            id: 4,
            tag: Tag::View,
        })
        .unwrap();
        apply_style(&mut c, 4, sized(10.0, 10.0));
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 4,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        // 8 + 20 top, 34 bottom, around a 10pt child. Margin left is 8 + 12.
        assert_eq!(frame(&batch, 2).size.height, px(72.0));
        assert_eq!(frame(&batch, 2).origin.x, px(20.0));
        assert_eq!(frame(&batch, 4).origin.y, px(72.0));

        c.set_safe_area(Edges {
            top: Points::ZERO,
            bottom: px(34.0),
            left: px(12.0),
            ..Edges::default()
        });
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 2).size.height, px(52.0));
        assert_eq!(frame(&batch, 4).origin.y, px(52.0));
    }

    #[test]
    fn max_env_uses_the_larger_inset() {
        let mut c = core();
        c.set_safe_area(Edges {
            bottom: px(34.0),
            ..Edges::default()
        });
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        let mut padded = Style::default();
        padded
            .set(
                "paddingBottom",
                StyleValue::Str(
                    "calc(max(env(safe-area-inset-bottom), env(keyboard-inset-height)) + 8px)",
                ),
            )
            .unwrap();
        apply_style(&mut c, 2, padded);
        c.apply(Op::Create {
            id: 3,
            tag: Tag::View,
        })
        .unwrap();
        apply_style(&mut c, 3, sized(10.0, 10.0));
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        // Keyboard hidden: safe area 34 + 8, around a 10pt child.
        assert_eq!(frame(&batch, 2).size.height, px(52.0));

        c.set_keyboard_inset(px(300.0));
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 2).size.height, px(318.0));

        c.set_safe_area(Edges {
            bottom: px(400.0),
            ..Edges::default()
        });
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 2).size.height, px(418.0));

        // A shorter keyboard does not shrink below the safe area.
        c.set_safe_area(Edges {
            bottom: px(34.0),
            ..Edges::default()
        });
        c.set_keyboard_inset(px(10.0));
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 2).size.height, px(52.0));
    }

    #[test]
    fn scroll_padding_follows_the_safe_area() {
        let mut c = core();
        c.set_safe_area(Edges {
            top: px(20.0),
            ..Edges::default()
        });
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Scroll,
        })
        .unwrap();
        let mut scroll = sized(100.0, 80.0);
        scroll
            .set("paddingTop", StyleValue::Str("env(safe-area-inset-top)"))
            .unwrap();
        apply_style(&mut c, 2, scroll);
        c.apply(Op::Create {
            id: 3,
            tag: Tag::View,
        })
        .unwrap();
        apply_style(&mut c, 3, sized(100.0, 50.0));
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 3).origin.y, px(20.0));

        c.set_safe_area(Edges {
            top: px(8.0),
            ..Edges::default()
        });
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 3).origin.y, px(8.0));
    }

    #[test]
    fn scroll_content_follows_safe_area_padding() {
        let mut c = core();
        c.set_safe_area(Edges {
            bottom: px(34.0),
            ..Edges::default()
        });
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Scroll,
        })
        .unwrap();
        let mut scroll = sized(100.0, 40.0);
        scroll
            .set(
                "paddingBottom",
                StyleValue::Str("env(safe-area-inset-bottom)"),
            )
            .unwrap();
        apply_style(&mut c, 2, scroll);
        c.apply(Op::Create {
            id: 3,
            tag: Tag::View,
        })
        .unwrap();
        apply_style(&mut c, 3, sized(100.0, 50.0));
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert_eq!(scroll_content(&batch, 2), size_pt(100.0, 84.0));

        c.set_safe_area(Edges {
            bottom: px(10.0),
            ..Edges::default()
        });
        let batch = c.commit().unwrap();
        assert_eq!(scroll_content(&batch, 2), size_pt(100.0, 60.0));
    }

    #[test]
    fn a_text_keeps_env_padding_after_leaving_a_span() {
        let mut c = core();
        c.set_safe_area(Edges {
            top: px(20.0),
            ..Edges::default()
        });
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Text,
        })
        .unwrap();
        let mut padded = Style::default();
        padded
            .set("paddingTop", StyleValue::Str("env(safe-area-inset-top)"))
            .unwrap();
        apply_style(&mut c, 2, padded);
        c.apply(Op::CreateText {
            id: 3,
            text: "ab".into(),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 2).size.height, px(36.0));

        // Nested: the layout node goes away. The inset changes while it is a span.
        c.apply(Op::Create {
            id: 4,
            tag: Tag::Text,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 4,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 4,
            child: 2,
            before: None,
        })
        .unwrap();
        c.set_safe_area(Edges {
            top: px(8.0),
            ..Edges::default()
        });
        c.commit();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 2).size.height, px(24.0));
    }

    #[test]
    fn horizontal_rtl_scroll_keeps_safe_area_on_the_physical_left() {
        let mut c = core();
        c.set_safe_area(Edges {
            left: px(20.0),
            ..Edges::default()
        });
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Scroll,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::Horizontal(true),
        })
        .unwrap();
        let mut scroll = sized(150.0, 50.0);
        scroll.direction = Some(true);
        scroll
            .set("paddingLeft", StyleValue::Str("env(safe-area-inset-left)"))
            .unwrap();
        apply_style(&mut c, 2, scroll);
        for id in [3, 4] {
            c.apply(Op::Create { id, tag: Tag::View }).unwrap();
            apply_style(&mut c, id, sized(100.0, 50.0));
            c.apply(Op::Insert {
                parent: 2,
                child: id,
                before: None,
            })
            .unwrap();
        }
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        // End padding after the row is mirrored back to the physical left.
        assert_eq!(scroll_content(&batch, 2), size_pt(220.0, 50.0));
        assert_eq!(frame(&batch, 3).origin.x, px(120.0));
        assert_eq!(frame(&batch, 4).origin.x, px(20.0));
    }

    #[test]
    fn state_styles_paint_without_changing_layout() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        apply_style(&mut c, 2, sized(80.0, 40.0));
        let mut active = Style::default();
        active.set("width", StyleValue::Number(10.0)).unwrap();
        active
            .set("backgroundColor", StyleValue::Str("#ff0000"))
            .unwrap();
        active.text.color = Some(Color(0x00ff00ff));
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::StateStyle {
                interaction: Interaction::Active,
                paint: Some(Box::new((active.view, active.text))),
            },
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert_eq!(frame(&batch, 2).size.width, px(80.0));
        let (view, text) = batch
            .items
            .iter()
            .find_map(|item| match item {
                MountItem::StateStyle {
                    id: 2,
                    interaction: Interaction::Active,
                    view,
                    text,
                } => Some((view, text)),
                _ => None,
            })
            .expect("active paint");
        assert_eq!(view.background_color, Color::rgba(255, 0, 0, 255));
        assert_eq!(text.color, Some(Color(0x00ff00ff)));

        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::StateStyle {
                interaction: Interaction::Active,
                paint: None,
            },
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::StateStyle {
                id: 2,
                interaction: Interaction::Active,
                view,
                text,
            } if **view == ViewStyle::default() && *text == TextStyle::default()
        )));
        assert!(
            batch
                .items
                .iter()
                .all(|item| !matches!(item, MountItem::Frame { id: 2, .. }))
        );
    }

    #[test]
    fn a_view_mounts_its_test_id_and_a_clear() {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::View,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::TestId(Some("box".into())),
        })
        .unwrap();
        apply_style(&mut c, 2, sized(10.0, 10.0));
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::TestId { id: 2, test_id } if test_id.as_deref() == Some("box")
        )));
        c.apply(Op::SetProp {
            id: 2,
            prop: Prop::TestId(None),
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert!(batch.items.iter().any(|item| matches!(
            item,
            MountItem::TestId {
                id: 2,
                test_id: None
            }
        )));
        assert!(
            batch
                .items
                .iter()
                .all(|item| !matches!(item, MountItem::Frame { .. }))
        );
    }

    /// 8pt per character, and counts typesetting so a test id can be shown
    /// not to redo it.
    struct Counted {
        prepares: std::cell::Cell<u32>,
        layouts: std::cell::Cell<u32>,
        measures: std::cell::Cell<u32>,
    }

    impl TextEngine for Counted {
        type Prepared = usize;

        fn prepare(&self, text: &AttributedText) -> usize {
            self.prepares.set(self.prepares.get() + 1);
            text.text.chars().count()
        }

        fn measure(&self, chars: &usize, max_width: Option<Points>) -> Size<Points> {
            self.measures.set(self.measures.get() + 1);
            let w = *chars as f32 * 8.0;
            match max_width.map(|m| m.0) {
                Some(max) if w > max && max > 0.0 => size_pt(max, (w / max).ceil() * 16.0),
                _ => size_pt(w, 16.0),
            }
        }

        fn layout(&self, chars: &usize, _: Size<Points>) -> TextLayout {
            self.layouts.set(self.layouts.get() + 1);
            TextLayout(Arc::new(*chars))
        }
    }

    #[test]
    fn a_spans_test_id_is_metadata_on_its_block() {
        let mut c = Core::new(
            Counted {
                prepares: std::cell::Cell::new(0),
                layouts: std::cell::Cell::new(0),
                measures: std::cell::Cell::new(0),
            },
            size(px(320.0), px(480.0)),
        );
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Text,
        })
        .unwrap();
        c.apply(Op::CreateText {
            id: 3,
            text: "see ".into(),
        })
        .unwrap();
        c.apply(Op::Create {
            id: 4,
            tag: Tag::Text,
        })
        .unwrap();
        c.apply(Op::CreateText {
            id: 5,
            text: "docs".into(),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 4,
            child: 5,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 2,
            child: 4,
            before: None,
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        let batch = c.commit().unwrap();
        assert!(mounted_text(&batch, 2).test_ids.is_empty());
        let (prepares, layouts, measures) = (
            c.text_engine.prepares.get(),
            c.text_engine.layouts.get(),
            c.text_engine.measures.get(),
        );
        c.apply(Op::SetProp {
            id: 4,
            prop: Prop::TestId(Some("docs".into())),
        })
        .unwrap();
        let text = mounted_text(&c.commit().unwrap(), 2);
        assert_eq!(
            text.test_ids,
            [crate::text::SpanTestId {
                start: 4,
                end: 8,
                test_id: "docs".into(),
            }]
        );
        assert_eq!(c.text_engine.prepares.get(), prepares);
        assert_eq!(c.text_engine.layouts.get(), layouts);
        assert_eq!(c.text_engine.measures.get(), measures);
        c.apply(Op::SetProp {
            id: 4,
            prop: Prop::TestId(None),
        })
        .unwrap();
        assert!(mounted_text(&c.commit().unwrap(), 2).test_ids.is_empty());
        assert_eq!(c.text_engine.layouts.get(), layouts);
        c.apply(Op::Listen {
            id: 4,
            event: EventKind::Press,
            enabled: true,
        })
        .unwrap();
        let text = mounted_text(&c.commit().unwrap(), 2);
        assert_eq!(text.links.len(), 1);
        assert_eq!(text.links[0].node, 4);
        assert_eq!(text.links[0].start, 4);
        assert_eq!(text.links[0].end, 8);
    }
}
