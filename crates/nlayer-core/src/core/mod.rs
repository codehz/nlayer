//! Shadow tree.
//!
//! Apply, commit and frame emission for views, text, images, scroll views,
//! virtualized lists and inputs, extracted from `nsh_core::core` and cut to those
//! hosts. `env()` on padding and margin follows the surface safe area and
//! the keyboard inset. `activeStyle`, `hoverStyle` and `focusStyle` are
//! stored and sent to the platform; they do not change layout. `testID` is
//! a name for tests: a view's is mounted, a nested span's is recorded on
//! its block, and neither is drawn. A pressable nested span is a link in
//! that block.
//!
//! v4 adds what the text is read through. `userSelect` resolves into a
//! text's [`AttributedText::selectable`]: `"text"` asks for selection,
//! `"none"` keeps it out, and `"auto"` (unset) follows the views around it
//! and is not selectable when none says so. `selectable` on a `View` is one
//! selection across its texts, in reading order, owned by the outermost
//! such view; it is mounted as [`MountItem::Selectable`]. `selectionColor`
//! is the block's highlight. Elements with layout inside a `<Text>` (views,
//! images, scroll views, inputs, lists) are attachments: each is its own
//! layout root, measured at its content size, with a U+FFFC placeholder in
//! the text, placed from the final text layout and never a normal child of
//! the text's parent. `href` on a `Text` (block or span) opens through
//! [`crate::open_url::check_url`] as [`Command::OpenUrl`], one way, after
//! the press's own listeners still run.
//!
//! `class` and `classList` resolve through [`crate::tailwind`] (`classes`):
//! later classes win, `style` wins per key, and state classes can change
//! layout. `activeStyle`, `hoverStyle` and `focusStyle` stay paint-only.

mod classes;
mod fenwick;
mod input;
mod layout;
mod layout_tree;
mod list;
mod lists;
mod mount;
mod nav;
mod props;
mod scroll;
mod text;
mod tree;

use std::{mem, rc::Rc, sync::Arc};

use layout_tree::LayoutTree;
use rustc_hash::FxHashSet;
use taffy::{AvailableSpace, Layout, Position, Style as TaffyStyle};

use crate::{
    geometry::{Bounds, Edges, Percentage, Point, Points, Size, rect},
    mount::{InputConfig, MountBatch, MountItem, ViewKind},
    ops::{
        Accessibility, AccessibilityProp, CellSlot, Command, EventKind, EventMask, ImageSource,
        Interaction, KeyboardDismiss, KeyboardTaps, NodeId, Op, Prop, ROOT_ID, ResizeMode, Tag,
    },
    style::{
        self, EllipsizeMode, EnvEdges, PointerEvents, Style, StylePatch, TextAlign, TextStyle,
        VerticalAlign, ViewStyle,
    },
    text::{
        AttachmentAlignments, AttributedText, MeasureCache, ResolvedTextStyle, SpanTestId,
        TextEngine, TextHref, TextLayout,
    },
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
    /// Emit [`crate::MountItem::ScreenConfig`].
    pub(super) const SCREEN: u32 = 1 << 14;
    /// Emit [`crate::MountItem::ModalConfig`].
    pub(super) const MODAL: u32 = 1 << 15;
    /// Emit [`crate::MountItem::Refreshing`].
    pub(super) const REFRESHING: u32 = 1 << 16;
    /// Emit [`crate::MountItem::PanBinding`] and its pinch and rotation
    /// siblings.
    pub(super) const PAN: u32 = 1 << 17;
    /// Emit [`crate::MountItem::Selectable`].
    pub(super) const SELECTABLE: u32 = 1 << 18;
    /// Emit [`crate::MountItem::PointerEvents`].
    pub(super) const POINTER: u32 = 1 << 19;
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
    /// A navigation stack: its children are screens, pushed and popped
    /// natively.
    Stack,
    /// A screen of a stack (its root view; the platform also makes its
    /// header).
    Screen,
    /// The root view of a modal: inserting it presents the modal, removing
    /// it dismisses it.
    Modal,
    /// A view shown in a screen's header (its slot is in the screen's
    /// config).
    HeaderItem,
}

impl Kind {
    fn is_container(self) -> bool {
        matches!(
            self,
            Kind::View
                | Kind::Image
                | Kind::Scroll
                | Kind::List
                | Kind::Cell
                | Kind::Stack
                | Kind::Screen
                | Kind::Modal
                | Kind::HeaderItem
        )
    }

    fn is_layout_root(self) -> bool {
        matches!(self, Kind::Screen | Kind::Modal | Kind::HeaderItem)
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
            Tag::Stack => Kind::Stack,
            Tag::Screen => Kind::Screen,
            Tag::Modal => Kind::Modal,
            Tag::HeaderItem => Kind::HeaderItem,
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
            Kind::Stack => ViewKind::Stack,
            Kind::Screen => ViewKind::Screen,
            Kind::Modal => ViewKind::Modal,
            Kind::HeaderItem => ViewKind::HeaderItem,
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
    /// The offset the platform last reported.
    offset: Point<Points>,
    taps: KeyboardTaps,
    dismiss: KeyboardDismiss,
    /// Last content size sent.
    content: Option<Size<Points>>,
    /// Index of that [`MountItem::ScrollContent`] in the current mount
    /// batch. A later size in the same batch replaces it, so the platform
    /// never sees a transient size (which would clamp the offset before the
    /// content grows back).
    content_item: Option<usize>,
    /// Refresh state.
    refreshing: bool,
    refresh_indicator: Option<Points>,
    refresh_reported: bool,
    pull_value_node: Option<crate::animation::NodeId>,
    /// Snap configuration.
    paging: bool,
    snap_interval: Option<f32>,
    snap_offsets: Option<Box<[f32]>>,
    snap_alignment: crate::ops::SnapAlignment,
    disable_interval_momentum: bool,
    deceleration: f32,
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

/// A layout root other than the surface's: screens, modals and header
/// items are laid out on their own, at a size their container gives them.
#[derive(Debug, Default)]
struct RootState {
    /// Size the platform reported for the root (modals).
    reported: Option<Size<Points>>,
    /// Safe-area insets the platform reported for this root. `env()` inside
    /// it reads them once `insets_set` is set; until then a screen whose
    /// native header covers the top uses no top inset.
    insets: crate::geometry::Edges<Points>,
    /// The platform has reported [`Self::insets`] (a screen's `onInsets`,
    /// or a modal's size report).
    insets_set: bool,
    /// `headerTransparent` at that report. A toggle lays out before the
    /// platform reports the new cover, and the old top inset must not be
    /// used for the new one.
    insets_transparent: bool,
    /// Height of the header the platform measured above the content
    /// (screens; until then, estimated from the bar's height and the safe
    /// area). `Some(0)` is not a measurement: a transparent or hidden bar
    /// reports no height.
    header: Option<f32>,
    /// The size and header height the root was last laid out with.
    laid_out: Option<(Size<Points>, f32)>,
    /// Screen options (screens).
    screen: crate::ScreenConfig,
    /// Modal options (modals).
    modal: crate::ModalConfig,
    /// Where a header item goes (header items).
    slot: crate::ops::HeaderSlot,
    /// Group index (0 at the outer edge).
    group: u32,
    /// Index inside the group (0 at the outer edge).
    index: u32,
    /// A bar button, not a custom view.
    button: bool,
    /// Button title.
    bar_title: Option<Arc<str>>,
    /// Button image.
    bar_image: Option<crate::ops::ImageSource>,
    /// Button enabled.
    bar_enabled: bool,
    /// Button tint.
    bar_tint: Option<crate::Color>,
}

/// Animation nodes the platform's gestures drive on a view: pan X and Y,
/// pinch scale and rotation angle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct GestureValues {
    pan_x: Option<NodeId>,
    pan_y: Option<NodeId>,
    pinch: Option<NodeId>,
    rotation: Option<NodeId>,
}

impl GestureValues {
    fn is_empty(self) -> bool {
        self.pan_x.is_none()
            && self.pan_y.is_none()
            && self.pinch.is_none()
            && self.rotation.is_none()
    }
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
    /// Animation nodes the platform's gestures drive.
    gestures: GestureValues,
    text_style: TextStyle,
    /// `numberOfLines` (`Some(0)`: no clamp).
    number_of_lines: Option<u32>,
    /// `textOverflow`.
    ellipsize: Option<EllipsizeMode>,
    /// `verticalAlign`: where it sits in its line as an attachment.
    vertical_align: VerticalAlign,
    /// `userSelect` as written: `Some(true)` is `"text"`, `Some(false)` is
    /// `"none"`, `None` is `"auto"` (follow the views around it).
    user_select: Option<bool>,
    /// `selectable` on a `View`: one selection across its texts.
    selectable: bool,
    /// `href` on a `Text` (block or span): the address a short press opens.
    href: Option<Arc<str>>,
    /// A text block's attachments (inline views and images, in text order)
    /// whose native views are inside its own.
    attached: Vec<NodeId>,
    /// Raw text content.
    text: String,
    /// Start / end properties, if any, so a direction change can place them
    /// again.
    logical: Option<Box<style::Logical>>,
    events: EventMask,
    /// `delayLongPress` in milliseconds; `None` uses the platform default.
    long_press_delay: Option<f32>,
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
    /// Screens, modals and header items.
    root: Option<Box<RootState>>,
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
    /// `class` and the truthy `classList` keys.
    class_names: Option<Box<classes::ClassNames>>,
    /// Parsed `class` / `classList`.
    classes: Option<Rc<crate::tailwind::ClassList>>,
    /// The `style` prop's declarations, replayed over the classes.
    inline_style: StylePatch,
    /// The last `style` prop. Clearing classes restores it whole.
    authored: Option<Box<Style>>,
    /// Node props the classes set (`space-*`, `selection:`).
    class_props: Option<Box<crate::tailwind::ClassProps>>,
    /// `pointerEvents` from the style, when set and not `auto`.
    explicit_pointer: Option<PointerEvents>,
    /// What the `pointer-events-*` classes mount as.
    class_pointer: PointerEvents,
    /// A `pointer-events-auto` sits under a `pointer-events-none` inside.
    pointer_auto_inside: bool,
    /// Interaction states (`tailwind::state` bits).
    states: u16,
    /// Bits the host should report ([`crate::MountItem::TrackStates`]):
    /// press, hover and focus. `focus-within` is not one of them.
    tracked: u16,
    /// Styled with the margin its parent's `space-*` gives it.
    spaced: bool,
    /// Minimum height written so a rounded text box holds its wrapped line,
    /// and the `min-height` it replaced. Cleared when the line fits again.
    line_floor: Option<(f32, taffy::LengthPercentageAuto)>,
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
            gestures: GestureValues::default(),
            text_style: TextStyle::default(),
            number_of_lines: None,
            ellipsize: None,
            vertical_align: VerticalAlign::Baseline,
            user_select: None,
            selectable: false,
            href: None,
            attached: Vec::new(),
            text: String::new(),
            logical: None,
            events: EventMask::default(),
            long_press_delay: None,
            object_fit: None,
            image: matches!(kind, Kind::Image).then_some(None),
            scroll: matches!(kind, Kind::Scroll | Kind::List).then(Box::default),
            list: matches!(kind, Kind::List).then(|| {
                let mut list = Box::new(list::ListState::new());
                // The runtime's lists pace their overscan; nlayer has no
                // components whose lists would bind in the core.
                list.paced = true;
                list
            }),
            slot: None,
            input: matches!(kind, Kind::Input).then(Box::default),
            root: matches!(kind, Kind::Screen | Kind::Modal | Kind::HeaderItem).then(Box::default),
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
            class_names: None,
            classes: None,
            inline_style: StylePatch::default(),
            authored: None,
            class_props: None,
            explicit_pointer: None,
            class_pointer: PointerEvents::Auto,
            pointer_auto_inside: false,
            states: 0,
            tracked: 0,
            spaced: false,
            line_floor: None,
        }
    }
}

/// Taffy context of a text block.
struct TextContext<P> {
    text: Arc<AttributedText>,
    /// Optional `verticalAlign` of its attachments, beside the text.
    alignments: AttachmentAlignments,
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
            alignments: AttachmentAlignments::default(),
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
    /// Text blocks with attachments, whose rebuild waits for their size.
    attachment_blocks: Vec<Key>,
    needs_layout: bool,
    /// A parent-viewport list's spacer changed length this round.
    spacer_changed: bool,
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
    /// Nodes whose classes say `pointer-events-none`.
    pointer_none_classes: u32,
    /// Height of the platform's screen header bar, normal and with a large
    /// title (status bar excluded). Screens without a native header use
    /// [`NO_NATIVE_HEADER`].
    header_heights: (Points, Points),
    /// Live screens, modals and header items (layout roots).
    roots: Vec<Key>,
    /// Live lists.
    lists: Vec<Key>,
    /// List scroll commands waiting for this commit's list update.
    commands: Vec<(NodeId, Command)>,
    /// What the runtime must bind before the frame shows.
    list_binds: Vec<ListBind>,
    /// The lists with overscan cells waiting to bind
    /// ([`Core::take_paced_list_binds`]), in the order they asked.
    paced_lists: Vec<NodeId>,
    /// [`Core::take_paced_list_binds`]'s buffer, reused.
    paced_scratch: Vec<(u32, u32)>,
    /// Events the core raises for JS (a list's end came near).
    events: Vec<(NodeId, EventKind)>,
    /// Class cache. The theme is Tailwind's default.
    tailwind: crate::tailwind::Tailwind,
    /// The focused view, by key and id. The root is never the focus.
    focus: Option<(Key, NodeId)>,
    /// The focus and its parents, innermost first (`focus-within`).
    focus_chain: Vec<(Key, NodeId)>,
    /// The chain before, reused.
    focus_scratch: Vec<(Key, NodeId)>,
    /// The focus was reported, or a view of its chain moved, since the
    /// last commit.
    focus_moved: bool,
    /// `unknown classes` and `unsupported classes` lines, once per string.
    class_warnings: Vec<String>,
}

impl<M: TextEngine> Core<M> {
    /// Creates a core whose root view (id [`ROOT_ID`]) fills `viewport`.
    /// The platform owns that view; it is not in the mount batch. `text_engine`
    /// measures and lays out text on the engine thread.
    pub fn new(text_engine: M, viewport: Size<Points>) -> Self {
        let mut layout = LayoutTree::with_capacity(256);
        let root_layout = layout.new_leaf(layout::root_style(viewport));
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
            attachment_blocks: Vec::new(),
            needs_layout: true,
            spacer_changed: false,
            pixel_ratio: 1.0,
            safe_area: Edges::default(),
            keyboard_inset: Points::ZERO,
            env_styled: FxHashSet::default(),
            pointer_none_classes: 0,
            header_heights: (Points(44.0), Points(96.0)),
            roots: Vec::new(),
            lists: Vec::new(),
            commands: Vec::new(),
            list_binds: Vec::new(),
            paced_lists: Vec::new(),
            paced_scratch: Vec::new(),
            events: Vec::new(),
            tailwind: crate::tailwind::Tailwind::new(),
            focus: None,
            focus_chain: Vec::new(),
            focus_scratch: Vec::new(),
            focus_moved: false,
            class_warnings: Vec::new(),
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
            self.layout.set_style(root, layout::root_style(viewport));
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

    /// Safe-area insets of the surface. `env(safe-area-inset-*)` outside a
    /// screen or modal follows them. Inside one, `env()` uses the insets
    /// that root reported; a native header covers the top until then.
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

    /// Class warnings for this core, in order. A class string warns once.
    pub fn class_warnings(&self) -> &[String] {
        &self.class_warnings
    }
}

#[cfg(test)]
mod tests;
