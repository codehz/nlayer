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
mod layout_tree;
mod list;

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

    /// Sets the height of the platform's screen header bar, normal and with
    /// a large title (status bar excluded). Platforms without a native header
    /// set [`NO_NATIVE_HEADER`].
    pub fn set_header_heights(&mut self, normal: Points, large: Points) {
        if self.header_heights != (normal, large) {
            self.header_heights = (normal, large);
            self.needs_layout = true;
            // A header appearing or leaving changes which top inset a
            // screen's content is owed.
            self.sync_env();
        }
    }

    /// Class warnings for this core, in order. A class string warns once.
    pub fn class_warnings(&self) -> &[String] {
        &self.class_warnings
    }

    /// The platform measured where the content of screen `id` starts below
    /// the header it shows (`None`: estimate it from the bar's height and
    /// the safe area). Used for screens that lay their content out below
    /// the header (not transparent headers).
    pub fn set_screen_header(&mut self, id: NodeId, height: Option<Points>) {
        let Ok(key) = self.key(id) else { return };
        let height = height.map(|h| h.0).filter(|h| h.is_finite() && *h >= 0.0);
        if let Some(root) = self.nodes[key as usize].root.as_mut()
            && root.header != height
        {
            root.header = height;
            self.needs_layout = true;
        }
    }

    /// The platform presents root `id` (a modal) at `size`, with `insets`
    /// of it covered by system UI. Those insets are what `env()` reads
    /// inside the modal.
    pub fn set_root_size(&mut self, id: NodeId, size: Size<Points>, insets: Edges<Points>) {
        let Ok(key) = self.key(id) else { return };
        let insets_changed = {
            let Some(root) = self.nodes[key as usize].root.as_mut() else {
                return;
            };
            let transparent = root.screen.header_transparent;
            let insets_changed =
                !root.insets_set || root.insets != insets || root.insets_transparent != transparent;
            let size_changed = root.reported != Some(size);
            if !insets_changed && !size_changed {
                return;
            }
            root.reported = Some(size);
            root.insets = insets;
            root.insets_set = true;
            root.insets_transparent = transparent;
            insets_changed
        };
        self.needs_layout = true;
        if insets_changed {
            self.sync_env();
        }
    }

    /// The platform reported the insets of screen or modal `id` (`onInsets`):
    /// what `env(safe-area-inset-*)` reads inside it. For an opaque header
    /// the top is what the header does not already cover. For a transparent
    /// header the top includes the bar: the content starts underneath it,
    /// and iOS's safe area counts that bar.
    pub fn set_root_insets(&mut self, id: NodeId, insets: Edges<Points>) {
        let Ok(key) = self.key(id) else { return };
        let changed = {
            let Some(root) = self.nodes[key as usize].root.as_mut() else {
                return;
            };
            let transparent = root.screen.header_transparent;
            if root.insets_set && root.insets == insets && root.insets_transparent == transparent {
                return;
            }
            root.insets = insets;
            root.insets_set = true;
            root.insets_transparent = transparent;
            true
        };
        if changed {
            self.sync_env();
        }
    }

    /// Handles back navigation. Returns `true` when a modal receives a close
    /// request or a stack can pop. Emits `onBackPress` first, then
    /// `onRequestClose` for modals or `onDismiss` for a popped screen.
    /// The engine tries `BackHandler` and platform behavior only when this
    /// returns `false`.
    pub fn handle_back(&mut self) -> bool {
        // Priority: topmost modal (last created), then topmost screen (last in its stack).
        let mut candidate: Option<(Key, bool)> = None; // (key, is_modal)

        // Find the last modal still mounted. A removed one stays in `roots`
        // until release; detached, it is not on screen and must not take back.
        for &key in self.roots.iter().rev() {
            let mounted = {
                let node = &self.nodes[key as usize];
                node.created && node.kind == Kind::Modal
            };
            if mounted && self.is_ancestor(self.root, key) {
                candidate = Some((key, true));
                break;
            }
        }

        // If no modal, find the topmost screen of the last stack in creation order.
        if candidate.is_none() {
            for &key in self.roots.iter().rev() {
                let node = &self.nodes[key as usize];
                if !node.created || node.kind != Kind::Screen {
                    continue;
                }
                let stack_key = node.parent;
                if stack_key == NO_KEY {
                    continue;
                }
                let stack = &self.nodes[stack_key as usize];
                if stack.kind != Kind::Stack {
                    continue;
                }
                // Last child of the stack is the topmost screen.
                if let Some(&topmost_key) = stack.children.last() {
                    let topmost = &self.nodes[topmost_key as usize];
                    if topmost.created && topmost.kind == Kind::Screen {
                        candidate = Some((topmost_key, false));
                        break;
                    }
                }
            }
        }

        let Some((key, is_modal)) = candidate else {
            return false;
        };

        let node = &self.nodes[key as usize];
        let id = node.id;
        let back_listens = node.events.contains(EventKind::BackRequest);
        let can_act = if is_modal {
            node.root.as_ref().is_none_or(|r| r.modal.close_on_escape)
        } else {
            let stack_key = node.parent;
            if stack_key == NO_KEY {
                false
            } else {
                let stack = &self.nodes[stack_key as usize];
                stack.children.len() > 1
                    && !node.root.as_ref().is_some_and(|r| r.screen.prevent_back)
            }
        };
        if back_listens {
            self.events.push((id, EventKind::BackRequest));
        }
        if is_modal {
            if node.events.contains(EventKind::RequestClose) {
                self.events.push((id, EventKind::RequestClose));
            }
            // Always consume back for a modal (return true), even when closeOnEscape=false.
            // With closeOnEscape=false, the app decides via onRequestClose; must not fall
            // through to BackHandler/platform default (which would move app to background).
            true
        } else {
            // For screens: return true if non-root (even if popOnBack=false).
            // Emit Dismiss only if can_act is true.
            let stack_key = node.parent;
            let is_root = stack_key != NO_KEY && self.nodes[stack_key as usize].children.len() == 1;
            if is_root {
                false
            } else {
                if can_act && node.events.contains(EventKind::Dismiss) {
                    self.events.push((id, EventKind::Dismiss));
                }
                true
            }
        }
    }

    /// The platform scrolled a list to `offset`. A report from before a
    /// scroll the core issued is replayed onto it.
    pub fn set_scroll_offset(&mut self, id: NodeId, offset: Point<Points>) {
        let Ok(key) = self.key(id) else { return };

        // Update list's own offset if this is a list node
        if let Some(list) = self.nodes[key as usize].list.as_mut() {
            list.reported(offset.y.0);
            self.mark(key, dirty::LIST);
        }

        // If this is a scroll node, recompute list offsets, sticky, and viewport_parent
        if let Some(scroll) = self.nodes[key as usize].scroll.as_mut() {
            scroll.offset = offset;
            self.recompute_scroll_lists(key, offset);
        }
    }

    /// The platform scrolled a scroll view to `offset`. Checks for pull-to-refresh
    /// and updates pull distance animation value.
    pub fn update_scroll_position(&mut self, id: NodeId, offset: Point<Points>) {
        let Ok(key) = self.key(id) else { return };
        let Some(scroll) = self.nodes[key as usize].scroll.as_mut() else {
            return;
        };
        scroll.offset = offset;

        // Calculate pull distance (negative offset means pulling down)
        let pull_distance = if offset.y.0 < 0.0 { -offset.y.0 } else { 0.0 };

        // Update pullValue animation node
        if let Some(pull_node) = scroll.pull_value_node {
            let value = if scroll.refreshing {
                // When refreshing, clamp to indicator height
                scroll.refresh_indicator.map(|h| h.0).unwrap_or(0.0)
            } else {
                pull_distance
            };
            self.mount
                .push(MountItem::Animation(crate::animation::AnimationOp::Set {
                    node: pull_node,
                    value,
                }));
        }

        // Check for pull-to-refresh trigger
        if let Some(indicator_height) = scroll.refresh_indicator
            && pull_distance >= indicator_height.0
        {
            // Fire refresh event only once per cycle
            if !scroll.refresh_reported && !scroll.refreshing {
                scroll.refresh_reported = true;
                self.events.push((id, EventKind::Refresh));
            }
        }

        // Recompute list offsets, sticky positions, and viewport_parent children
        self.recompute_scroll_lists(key, offset);
    }

    /// Recompute list offsets, sticky positions, and windowing when a scroll view moves.
    /// Used by both `set_scroll_offset` (platform events) and `update_scroll_position` (messages).
    fn recompute_scroll_lists(&mut self, key: Key, offset: Point<Points>) {
        let Some(scroll) = self.nodes[key as usize].scroll.as_ref() else {
            return;
        };
        let horizontal = scroll.horizontal;

        // Update list offset if this scroll view has a list (for scroll+list nodes)
        if let Some(list) = self.nodes[key as usize].list.as_mut() {
            let new_offset = if horizontal { offset.x.0 } else { offset.y.0 };
            list.note_platform_offset(new_offset);
            if list.dirty {
                self.mark(key, dirty::LIST);
            }
        }

        // Update child lists that use viewport="parent"
        let children = self.nodes[key as usize].children.clone();
        for &child_key in &children {
            self.update_viewport_parent_list_offset(child_key, offset, horizontal);
        }
    }

    fn update_viewport_parent_list_offset(
        &mut self,
        key: Key,
        offset: Point<Points>,
        horizontal: bool,
    ) {
        // Check if this node has a viewport_parent list
        let is_viewport_parent = self.nodes[key as usize]
            .list
            .as_ref()
            .is_some_and(|l| l.viewport_parent);

        if is_viewport_parent {
            // Calculate cumulative position from list up to parent scroll
            let mut cumulative_pos = 0.0;
            let mut current_key = key;

            // Walk up the parent chain until we find the scroll node
            loop {
                // Add this node's layout position; the list's own border
                // and padding come before its content.
                if let Some(layout) = self.nodes[current_key as usize].layout {
                    let l = self.rounded_layout(layout);
                    cumulative_pos += if horizontal {
                        l.location.x
                    } else {
                        l.location.y
                    };
                    if current_key == key {
                        cumulative_pos += if horizontal {
                            l.border.left + l.padding.left
                        } else {
                            l.border.top + l.padding.top
                        };
                    }
                }

                // Move to parent
                let parent_key = self.nodes[current_key as usize].parent;
                if parent_key == NO_KEY {
                    break;
                }

                // Stop if parent is a scroll node (this is our viewport parent)
                if self.nodes[parent_key as usize].kind == Kind::Scroll {
                    break;
                }

                current_key = parent_key;
            }

            // List offset = parent scroll offset - list's cumulative position in scroll content
            let parent_offset = if horizontal { offset.x.0 } else { offset.y.0 };
            let new_offset = parent_offset - cumulative_pos;

            if let Some(list) = self.nodes[key as usize].list.as_mut() {
                list.note_platform_offset(new_offset);
                if list.dirty {
                    self.mark(key, dirty::LIST);
                }
            }
        }

        // Recursively check children
        let children = self.nodes[key as usize].children.clone();
        for &child_key in &children {
            self.update_viewport_parent_list_offset(child_key, offset, horizontal);
        }
    }

    /// `viewport` of list `key`: `Parent` virtualizes it against the scroll
    /// view around it and gives that view the list's full length. The mode
    /// is fixed when the list's native view is made; a later change is
    /// ignored, as in nsh.
    fn set_list_viewport(&mut self, key: Key, parent: bool) {
        let Some(list) = self.nodes[key as usize].list.as_ref() else {
            return;
        };
        if list.viewport_parent == parent {
            return;
        }
        if self.nodes[key as usize].created {
            log::warn!(
                "List {}: `viewport` is fixed when the list is made; the change is ignored (render a new list)",
                self.nodes[key as usize].id
            );
            return;
        }
        self.nodes[key as usize]
            .list
            .as_mut()
            .expect("list")
            .viewport_parent = parent;
        let Some(layout) = self.nodes[key as usize].layout else {
            return;
        };
        if parent {
            let len = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .content_len();
            let spacer = self.layout.new_leaf(spacer_style(len));
            self.layout.add_child(layout, spacer);
            self.nodes[key as usize].list.as_mut().expect("list").spacer = Some((spacer, len));
        } else if let Some((spacer, _)) = self.nodes[key as usize]
            .list
            .as_mut()
            .expect("list")
            .spacer
            .take()
        {
            self.layout.remove(spacer);
        }
        self.sync_scroll_style(key);
        self.needs_layout = true;
        self.mark(key, dirty::LIST);
    }

    /// Gives a parent-viewport list's spacer the new content length: the
    /// scroll view around it scrolls over the whole list.
    fn sync_list_spacer(&mut self, key: Key) {
        let Some(list) = self.nodes[key as usize].list.as_ref() else {
            return;
        };
        let Some((spacer, was)) = list.spacer else {
            return;
        };
        let len = list.content_len();
        if len == was {
            return;
        }
        self.nodes[key as usize].list.as_mut().expect("list").spacer = Some((spacer, len));
        self.layout.set_style(spacer, spacer_style(len));
        self.needs_layout = true;
        self.spacer_changed = true;
    }

    /// The offset the scroll view around `key` is at, and its axis.
    fn outer_scroll_offset(&self, key: Key) -> Option<(Point<Points>, bool)> {
        let mut at = self.nodes[key as usize].parent;
        while at != NO_KEY {
            let node = &self.nodes[at as usize];
            if node.kind == Kind::Scroll || node.kind == Kind::List {
                let scroll = node.scroll.as_ref()?;
                return Some((scroll.offset, scroll.horizontal));
            }
            at = node.parent;
        }
        None
    }

    /// Moves the cells the runtime must render before the frame shows.
    pub fn take_list_binds(&mut self, out: &mut Vec<ListBind>) {
        out.append(&mut self.list_binds);
    }

    /// Whether lists have overscan cells waiting to bind.
    pub fn has_paced_list_binds(&self) -> bool {
        !self.paced_lists.is_empty()
    }

    /// Hands out up to `cells` of the overscan cells waiting to bind, one
    /// per list in turn (a list is not starved by the one before it), each
    /// list's nearest its leading edge first. What a list assigns now is
    /// what waits: cells it left or whose items went are never handed out.
    pub fn take_paced_list_binds(&mut self, cells: usize, out: &mut Vec<ListBind>) {
        let mut left = cells;
        let mut lists = std::mem::take(&mut self.paced_lists);
        let mut one = std::mem::take(&mut self.paced_scratch);
        let start = out.len();
        while left > 0 && !lists.is_empty() {
            lists.retain(|&id| {
                if left == 0 {
                    return true;
                }
                let Some(list) = self
                    .key(id)
                    .ok()
                    .and_then(|key| self.nodes[key as usize].list.as_mut())
                else {
                    return false;
                };
                one.clear();
                left -= list.take_paced(1, &mut one);
                if let Some((slot, index)) = one.pop() {
                    match out[start..].iter_mut().find(|b| b.list == id) {
                        Some(bind) => bind.binds.push((slot, index)),
                        None => out.push(ListBind {
                            list: id,
                            binds: vec![(slot, index)],
                        }),
                    }
                }
                list.has_paced()
            });
        }
        self.paced_lists = lists;
        self.paced_scratch = one;
    }

    /// The events the core raised for JS.
    pub fn take_events(&mut self, out: &mut Vec<(NodeId, EventKind)>) {
        out.append(&mut self.events);
    }

    /// A short press on `id`: the innermost `href` at or around it is
    /// checked ([`crate::open_url::check_url`]) and, when accepted, mounted
    /// as [`Command::OpenUrl`] on the root. Returns the URL it opened.
    /// The press's own listeners still run; this does not change hit
    /// testing.
    pub fn press(&mut self, id: NodeId) -> Option<String> {
        let key = self.key(id).ok()?;
        let block = self.text_block_of(key)?;
        let layout = self.nodes[block as usize].layout?;
        let mut nodes = vec![id];
        let mut k = key;
        while k != block {
            k = self.nodes[k as usize].parent;
            if k == NO_KEY {
                break;
            }
            nodes.push(self.nodes[k as usize].id);
        }
        let url = {
            let LeafContext::Text(cx) = self.layout.get_node_context_mut(layout)? else {
                return None;
            };
            nodes
                .iter()
                .find_map(|&node| cx.text.href_of_node(node).map(|h| h.url.clone()))
        }?;
        self.open_checked(&url)
    }

    /// A short press at `point` (the text's content box, y down) on the
    /// text node `id`: the `href` at the offset the point hits, innermost
    /// first, then the ones around the node. Checked and mounted as
    /// [`Command::OpenUrl`] when accepted; returns the URL it opened.
    pub fn press_at(&mut self, id: NodeId, point: Point<Points>) -> Option<String> {
        let key = self.key(id).ok()?;
        let block = self.text_block_of(key)?;
        let block_id = self.nodes[block as usize].id;
        let layout = self.nodes[block as usize].layout?;
        let engine = &self.text_engine;
        let url = {
            let LeafContext::Text(cx) = self.layout.get_node_context_mut(layout)? else {
                return None;
            };
            let offset = cx
                .laid_out
                .as_ref()
                .map_or(0, |laid| engine.hit_test(&laid.layout, point).offset);
            cx.text
                .href_at(offset)
                .or_else(|| cx.text.href_of_node(id))
                .or_else(|| cx.text.href_of_node(block_id))
                .map(|h| h.url.clone())
        }?;
        self.open_checked(&url)
    }

    /// Checks `url` and mounts [`Command::OpenUrl`] for it. A refused URL
    /// is not opened, and why is logged; the press still reports itself.
    fn open_checked(&mut self, url: &Arc<str>) -> Option<String> {
        match crate::open_url::check_url(url) {
            Ok(()) => {
                let url = url.to_string();
                self.mount.push(MountItem::Command {
                    id: ROOT_ID,
                    command: Command::OpenUrl { url: url.clone() },
                });
                Some(url)
            }
            Err(reason) => {
                log::warn!("{reason}");
                None
            }
        }
    }

    /// The text block (not a span) around `key`.
    fn text_block_of(&self, mut key: Key) -> Option<Key> {
        loop {
            let node = &self.nodes[key as usize];
            if !node.inline {
                return (node.kind == Kind::Text).then_some(key);
            }
            if node.parent == NO_KEY {
                return None;
            }
            key = node.parent;
        }
    }

    /// Gets the viewable item range for a list (for ViewableItemsChange event payload).
    pub fn viewable_range(&self, id: NodeId) -> Option<(u32, u32)> {
        let key = self.key(id).ok()?;
        self.nodes[key as usize].list.as_ref()?.viewable_range
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
            Op::SetStyle { id, style, patch } => self.apply_authored_style(id, *style, patch),
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
        if self.focus_moved {
            self.update_focus_within();
        }
        self.flush_dirty();
        self.layout_round();
        self.update_lists();
        // A parent-viewport list gives the page its content length after
        // measuring it: lay the page out again with that length, until it
        // settles.
        for _ in 0..4 {
            if !mem::take(&mut self.spacer_changed) {
                break;
            }
            self.needs_layout = true;
            self.layout_round();
            self.update_lists();
        }
        self.flush_text();
        if self.mount.is_empty() {
            None
        } else {
            Some(MountBatch {
                items: mem::take(&mut self.mount),
            })
        }
    }

    /// Lays the changed trees out and emits their frames.
    fn layout_round(&mut self) {
        if mem::take(&mut self.needs_layout) {
            self.layout_once();
            // Rounding can make a text's box narrower than the width it was
            // measured at. One more pass gives that line a row, then stops.
            if self.grow_clipped_text() {
                self.layout_once();
            }
        }
        self.layout_cells();
    }

    /// Lays out what is dirty and emits the frames that moved.
    fn layout_once(&mut self) {
        let relaid = self.relayout_boundaries();
        if self.layout_main() {
            let root = self.root;
            let children = mem::take(&mut self.nodes[root as usize].children);
            for &child in &children {
                if !self.nodes[child as usize].kind.is_layout_root() {
                    self.emit_frames(child);
                }
            }
            self.nodes[root as usize].children = children;
        }
        self.layout_roots();
        for key in relaid {
            self.emit_frames(key);
        }
    }

    /// Texts whose drawn box is narrower than the width they were measured
    /// at, and whose line at that box is taller than the frame. Gives each
    /// a minimum height and marks it dirty. A fixed height is left alone.
    fn grow_clipped_text(&mut self) -> bool {
        let texts: Vec<Key> = self
            .nodes
            .iter()
            .filter_map(|(index, node)| {
                (node.kind == Kind::Text && node.created && node.layout.is_some())
                    .then_some(index as Key)
            })
            .collect();
        let mut grew = false;
        for key in texts {
            if self.displays_none(key) {
                continue;
            }
            let Some(layout_id) = self.nodes[key as usize].layout else {
                continue;
            };
            let content = content_box(&self.rounded_layout(layout_id));
            let measured = {
                let Some(LeafContext::Text(cx)) = self.layout.get_node_context_mut(layout_id)
                else {
                    continue;
                };
                cx.cache
            };
            let Some((measured_at, measured_size)) = measured else {
                continue;
            };
            // Flex measures the line once at the border box (one line) and
            // again at the content box (wrapped). The used height stays the
            // first, and the cache only keeps the second width, so a matching
            // width is not proof the frame holds the line.
            let width_matches =
                measured_at.is_some_and(|width| content.size.width.0 + 0.05 >= width);
            if width_matches
                && measured_size.height.0 <= content.size.height.0 + 0.5
                && self.nodes[key as usize].line_floor.is_none()
            {
                continue;
            }
            if !self.layout.style(layout_id).size.height.is_auto() {
                continue;
            }
            let height = if width_matches {
                measured_size.height
            } else {
                let engine = &self.text_engine;
                let cache = &mut self.measure_cache;
                let Some(LeafContext::Text(cx)) = self.layout.get_node_context_mut(layout_id)
                else {
                    continue;
                };
                measure_text(engine, cache, cx, Some(content.size.width.0)).height
            };
            let unrounded = *self.layout.unrounded_layout(layout_id);
            let pad = unrounded.padding.top
                + unrounded.padding.bottom
                + unrounded.border.top
                + unrounded.border.bottom;
            let needed = height.0 + pad;
            let floor = self.nodes[key as usize].line_floor;
            let mut style = self.layout.style(layout_id).clone();
            let (applied, restored) = match floor {
                Some((applied, previous))
                    if style.min_size.height.expand()
                        == taffy::LengthPercentageAuto::length(applied).expand() =>
                {
                    (Some(applied), previous)
                }
                _ => (None, style.min_size.height),
            };
            if needed > unrounded.size.height + 0.5 {
                let min = match restored.expand() {
                    taffy::ExpandedLengthPercentageAuto::Length(len) => len.max(needed),
                    _ => needed,
                };
                style.min_size.height = taffy::LengthPercentageAuto::length(min);
                self.layout.set_style(layout_id, style);
                self.layout.mark_dirty(layout_id);
                self.nodes[key as usize].line_floor = Some((min, restored));
                grew = true;
            } else if applied.is_some_and(|applied| needed + 0.5 < applied) {
                style.min_size.height = restored;
                self.layout.set_style(layout_id, style);
                self.layout.mark_dirty(layout_id);
                self.nodes[key as usize].line_floor = None;
                grew = true;
            } else if floor.is_some() && applied.is_none() {
                self.nodes[key as usize].line_floor = None;
            }
        }
        grew
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
        let root_base = matches!(kind, Kind::Screen | Kind::Modal).then(|| layout_style.clone());
        let layout = match kind {
            Kind::Image => self
                .layout
                .new_leaf_with_context(layout_style, LeafContext::Image(None)),
            Kind::Text | Kind::Input => self.layout.new_leaf_with_context(
                layout_style,
                LeafContext::Text(TextContext::new(Arc::default())),
            ),
            Kind::View | Kind::Scroll | Kind::List | Kind::Cell | Kind::Stack => {
                self.layout.new_leaf(layout_style)
            }
            Kind::Screen | Kind::Modal | Kind::HeaderItem => {
                // Layout roots: laid out on their own, not in parent.
                self.layout.new_leaf(layout_style)
            }
            Kind::RawText => unreachable!("raw text is CreateText, not a tag"),
        };
        self.layout.set_tag(layout, key);
        self.nodes[key as usize].layout = Some(layout);
        if kind == Kind::Text {
            self.nodes[key as usize].dirty |= dirty::TEXT_BUILD;
        }
        if let Some(base) = scroll_base {
            self.nodes[key as usize].base_layout = Some(Box::new(base));
        }
        if let Some(base) = root_base {
            self.nodes[key as usize].base_layout = Some(Box::new(base));
        }
        if matches!(kind, Kind::Scroll | Kind::List) {
            self.sync_scroll_style(key);
        }
        if kind == Kind::List {
            self.lists.push(key);
        }
        if matches!(kind, Kind::Screen | Kind::Modal | Kind::HeaderItem) {
            self.roots.push(key);
            // Mark dirty to emit initial config.
            if kind == Kind::Screen {
                self.nodes[key as usize].dirty |= dirty::SCREEN;
            } else if kind == Kind::Modal {
                self.nodes[key as usize].dirty |= dirty::MODAL;
            }
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
            vertical_align,
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
        let applied = (!env_edges.is_empty()).then(|| self.env_insets(key, &env_edges));
        let node = &mut self.nodes[key as usize];
        let span_background_changed = node.inline
            && node.kind == Kind::Text
            && span_background(&node.view_style, node.id) != span_background(&view, node.id);
        let lines_changed = node.kind == Kind::Text
            && (node.number_of_lines != pending.number_of_lines
                || node.ellipsize != pending.text_overflow);
        let select_changed = node.user_select != pending.user_select;
        let align_changed = node.vertical_align != vertical_align;
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
        node.user_select = pending.user_select;
        let pointer = pending.pointer_events.filter(|p| *p != PointerEvents::Auto);
        let pointer_changed = node.explicit_pointer != pointer;
        node.explicit_pointer = pointer;
        node.vertical_align = vertical_align;
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
        if matches!(
            kind,
            Kind::Text | Kind::Scroll | Kind::List | Kind::Screen | Kind::Modal
        ) {
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
        if pointer_changed {
            self.mark(key, dirty::POINTER);
        }
        if fit_changed {
            self.mark(key, dirty::IMAGE);
        }
        if text_changed || flipped || select_changed {
            match self.nodes[key as usize].kind {
                Kind::Text => self.mark_text_block(key),
                Kind::Input => {
                    self.measure_input(key);
                    self.mark(key, dirty::INPUT);
                }
                _ if text_changed || select_changed => self.mark_inheriting_text(key),
                _ => {}
            }
        }
        if align_changed && self.nodes[key as usize].inline {
            // Its line reserves its new place.
            self.mark_text_block(key);
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
            Prop::Refreshing(refreshing) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let was_refreshing = scroll.refreshing;
                        scroll.refreshing = refreshing;
                        // Reset reported flag on transitions
                        if was_refreshing != refreshing {
                            scroll.refresh_reported = false;
                        }
                        was_refreshing != refreshing
                    });
                if changed {
                    self.mark(key, dirty::REFRESHING);
                }
            }
            Prop::RefreshIndicator(height) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.refresh_indicator != height;
                        scroll.refresh_indicator = height;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::PullValue(node) => {
                if let Some(scroll) = self.nodes[key as usize].scroll.as_mut() {
                    scroll.pull_value_node = node;
                }
            }
            Prop::PagingEnabled(paging) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.paging != paging;
                        scroll.paging = paging;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::SnapToInterval(interval) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.snap_interval != interval;
                        scroll.snap_interval = interval;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::SnapToOffsets(offsets) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.snap_offsets != offsets;
                        scroll.snap_offsets = offsets;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::SnapToAlignment(alignment) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.snap_alignment != alignment;
                        scroll.snap_alignment = alignment;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::DisableIntervalMomentum(disable) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let changed = scroll.disable_interval_momentum != disable;
                        scroll.disable_interval_momentum = disable;
                        changed
                    });
                if changed {
                    self.mark(key, dirty::SCROLL);
                }
            }
            Prop::DecelerationRate(rate) => {
                let changed = self.nodes[key as usize]
                    .scroll
                    .as_mut()
                    .is_some_and(|scroll| {
                        let value = rate.value();
                        let changed = (scroll.deceleration - value).abs() > 0.001;
                        scroll.deceleration = value;
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
            Prop::StickyHeaderIndices(indices) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_sticky(indices.map(|b| b.into_vec()).unwrap_or_default());
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Inverted(inverted) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.set_inverted(inverted);
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::MaintainVisibleContentPosition(config) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.maintain_position = config;
                    self.mark(key, dirty::LIST);
                }
            }
            Prop::Viewport(viewport) => {
                self.set_list_viewport(key, matches!(viewport, crate::ops::ListViewport::Parent));
            }
            Prop::ViewabilityConfig(config) => {
                if let Some(list) = self.nodes[key as usize].list.as_mut() {
                    list.viewability = config;
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
            Prop::LongPressDelay(delay) => {
                if self.nodes[key as usize].long_press_delay != delay {
                    self.nodes[key as usize].long_press_delay = delay;
                    self.mark(key, dirty::EVENTS);
                }
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
                self.set_disabled(key, disabled);
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
            Prop::Screen(screen_prop) => {
                use crate::ops::ScreenProp;
                let mut cover_changed = false;
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    cover_changed = match &screen_prop {
                        ScreenProp::HeaderVisible(visible) => {
                            root.screen.header_visible != *visible
                        }
                        ScreenProp::HeaderTransparent(transparent) => {
                            root.screen.header_transparent != *transparent
                        }
                        _ => false,
                    };
                    let config = &mut root.screen;
                    match screen_prop {
                        ScreenProp::Title(title) => config.title = title,
                        ScreenProp::HeaderVisible(visible) => config.header_visible = visible,
                        ScreenProp::HeaderTransparent(transparent) => {
                            config.header_transparent = transparent
                        }
                        ScreenProp::HeaderBackVisible(visible) => config.back_visible = visible,
                        ScreenProp::PopOnSwipe(enabled) => config.gesture_enabled = enabled,
                        ScreenProp::PopOnBack(pop) => config.prevent_back = !pop,
                        ScreenProp::Animation(animation) => config.animation = animation,
                        ScreenProp::HeaderStyle {
                            background_color,
                            title_color,
                            tint_color,
                        } => {
                            config.background_color = background_color;
                            config.title_color = title_color;
                            config.tint_color = tint_color;
                        }
                    }
                    self.nodes[key as usize]
                        .root
                        .as_mut()
                        .expect("root")
                        .laid_out = None;
                    self.nodes[key as usize].dirty |= dirty::SCREEN;
                    self.needs_layout = true;
                }
                // Showing, hiding or making the header transparent changes
                // whether the content still owes the surface's top inset.
                if cover_changed {
                    self.sync_env();
                }
            }
            Prop::Modal(modal_prop) => {
                use crate::ops::ModalProp;
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    let config = &mut root.modal;
                    match modal_prop {
                        ModalProp::Animation(animation) => config.animation = animation,
                        ModalProp::Presentation(presentation) => config.presentation = presentation,
                        ModalProp::CloseOnEscape(close) => config.close_on_escape = close,
                        ModalProp::CloseOnBackdropPress(close) => {
                            config.close_on_backdrop_press = close
                        }
                        ModalProp::CloseOnSwipe(close) => config.close_on_swipe = close,
                    }
                    self.nodes[key as usize].dirty |= dirty::MODAL;
                }
            }
            Prop::Pan { x, y } => {
                self.nodes[key as usize].gestures.pan_x = x;
                self.nodes[key as usize].gestures.pan_y = y;
                self.mark(key, dirty::PAN);
            }
            Prop::Pinch { scale } => {
                self.nodes[key as usize].gestures.pinch = scale;
                self.mark(key, dirty::PAN);
            }
            Prop::Rotation { angle } => {
                self.nodes[key as usize].gestures.rotation = angle;
                self.mark(key, dirty::PAN);
            }
            Prop::Selectable(selectable) => {
                if self.nodes[key as usize].selectable != selectable {
                    self.nodes[key as usize].selectable = selectable;
                    self.mark(key, dirty::SELECTABLE);
                }
            }
            Prop::Href(href) => {
                let href = href.map(Arc::from);
                if self.nodes[key as usize].href != href {
                    self.nodes[key as usize].href = href;
                    self.mark_text_block(key);
                }
            }
            Prop::Class(name) => self.set_class_name(key, name),
            Prop::ClassList(list) => self.set_class_list_names(key, list),
            Prop::HeaderSlot(slot) => {
                if let Some(root) = self.nodes[key as usize].root.as_mut() {
                    root.slot = slot;
                    // Re-layout the parent screen when the slot changes.
                    let parent = self.nodes[key as usize].parent;
                    if parent != NO_KEY {
                        self.needs_layout = true;
                    }
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
        if self.nodes[child as usize].states & crate::tailwind::state::FOCUS_WITHIN != 0 {
            self.focus_moved = true;
        }
        self.spacing_changed(parent, child);
        if self.pointer_none_classes > 0 {
            self.pointer_events_moved(parent);
        }
        self.update_direction(child);
        // Style is applied before insert. `env()` was resolved with no
        // parent then; the screen or modal this lands in may owe different
        // insets.
        self.refresh_env(child);
        // A header slot's options are part of the screen's config.
        if self.nodes[child as usize].kind == Kind::HeaderItem
            && self.nodes[parent as usize].kind == Kind::Screen
        {
            self.mark(parent, dirty::SCREEN);
            self.needs_layout = true;
        }
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
            if !self.nodes[child as usize].kind.is_layout_root() {
                let parent_layout = self.nodes[parent as usize]
                    .layout
                    .expect("block parent has layout");
                // Layout roots (a screen's header items) are children of
                // the screen, not of its layout.
                let layout_pos = self.nodes[parent as usize].children[..pos]
                    .iter()
                    .filter(|&&c| !self.nodes[c as usize].kind.is_layout_root())
                    .count();
                self.layout
                    .insert_child_at_index(parent_layout, layout_pos, child_layout);
            }
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
        if self.nodes[key as usize].states & crate::tailwind::state::FOCUS_WITHIN != 0 {
            self.focus_moved = true;
        }
        self.spacing_changed(parent, key);
        if self.pointer_none_classes > 0 && self.pointer_region(parent).is_some() {
            self.update_pointer_events(key);
            self.pointer_events_moved(parent);
        }
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
        if self.nodes[key as usize].kind == Kind::HeaderItem
            && self.nodes[parent as usize].kind == Kind::Screen
        {
            self.mark(parent, dirty::SCREEN);
            self.needs_layout = true;
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
                if !self.nodes[key as usize].kind.is_layout_root() {
                    let parent_layout = self.nodes[parent as usize]
                        .layout
                        .expect("block parent has layout");
                    self.layout.remove_child(parent_layout, layout);
                }
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
        if self.nodes[key as usize].states & crate::tailwind::state::FOCUS_WITHIN != 0 {
            self.focus_moved = true;
        }
        let node = self.nodes.remove(key as usize);
        if node.class_props.as_ref().and_then(|c| c.pointer_events) == Some(PointerEvents::None) {
            self.pointer_none_classes -= 1;
        }
        self.slots[id as usize] = 0;
        self.lists.retain(|&k| k != key);
        self.roots.retain(|&k| k != key);
        self.attachment_blocks.retain(|&k| k != key);
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
    /// Sticky list cells paint above the rows scrolling under them. Only
    /// the band this sets (`1`) is touched, so a style `zIndex` stays.
    fn set_cell_sticky(&mut self, cell: Key, sticky: bool) {
        const STICKY_Z: i32 = 1;
        let node = &mut self.nodes[cell as usize];
        let want = if sticky { STICKY_Z } else { 0 };
        if node.z_index == want || (!sticky && node.z_index != STICKY_Z) {
            return;
        }
        if sticky && node.z_index != 0 && node.z_index != STICKY_Z {
            return;
        }
        node.z_index = want;
        self.restack(cell);
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
        let has_attachments = self.attachment_blocks.contains(&key);
        let pointer = self.mounted_pointer(key);
        let node = &mut self.nodes[key as usize];
        if node.created || node.layout.is_none() {
            return;
        }
        node.created = true;
        node.frame = Bounds::UNSET;
        let id = node.id;
        let kind = node.kind;
        let parent_list =
            kind == Kind::List && node.list.as_ref().is_some_and(|list| list.viewport_parent);
        let mut flags = 0;
        if node.view_style != ViewStyle::default() {
            flags |= dirty::VIEW;
        }
        if !node.animated.is_empty() {
            flags |= dirty::ANIMATION;
        }
        if !node.gestures.is_empty() {
            flags |= dirty::PAN;
        }
        if !node.events.is_empty() || node.long_press_delay.is_some() {
            flags |= dirty::EVENTS;
        }
        if kind == Kind::Text {
            flags |= dirty::TEXT_MOUNT;
        }
        if node.image.as_ref().is_some_and(|source| source.is_some()) {
            flags |= dirty::IMAGE;
        }
        if matches!(kind, Kind::Scroll | Kind::List) && !parent_list {
            // A new native view has not been told its content size.
            node.scroll.as_mut().expect("scroll").content = None;
            flags |= dirty::SCROLL;
        }
        if kind == Kind::Screen {
            flags |= dirty::SCREEN;
        }
        if kind == Kind::Modal {
            flags |= dirty::MODAL;
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
        if node.selectable {
            flags |= dirty::SELECTABLE;
        }
        if pointer != PointerEvents::Auto {
            flags |= dirty::POINTER;
        }
        if kind == Kind::Text && has_attachments {
            // Its attachments' views go inside the new view.
            node.attached.clear();
            flags |= dirty::TEXT_BUILD;
        }
        if !node.accessibility.is_default() {
            flags |= dirty::ACCESSIBILITY;
        }
        self.mount.push(MountItem::Create {
            id,
            kind: if parent_list {
                ViewKind::View
            } else {
                kind.view_kind()
            },
        });
        if let Some(item) = self.creation_tracking(key) {
            self.mount.push(item);
        }
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
        loop {
            self.flush_dirty_once();
            // Text blocks create the views of new attachments while
            // flushing, which queues them; texts inside attachments built
            // after their block resize them, which builds the block again.
            self.check_attachments();
            if self.queue.is_empty() {
                break;
            }
        }
    }

    fn flush_dirty_once(&mut self) {
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
            if flags & dirty::PAN != 0 {
                let gestures = self.nodes[key as usize].gestures;
                self.mount.push(MountItem::PanBinding {
                    id,
                    x: gestures.pan_x,
                    y: gestures.pan_y,
                });
                self.mount.push(MountItem::PinchBinding {
                    id,
                    scale: gestures.pinch,
                });
                self.mount.push(MountItem::RotationBinding {
                    id,
                    angle: gestures.rotation,
                });
            }
            if flags & dirty::EVENTS != 0 {
                let events = self.nodes[key as usize].events;
                self.mount.push(MountItem::Events { id, events });
                let delay = self.nodes[key as usize].long_press_delay;
                self.mount.push(MountItem::LongPressDelay { id, delay });
            }
            if flags & dirty::SELECTABLE != 0 {
                let selectable = self.nodes[key as usize].selectable;
                self.mount.push(MountItem::Selectable { id, selectable });
            }
            if flags & dirty::POINTER != 0 {
                let events = self.mounted_pointer(key);
                self.mount.push(MountItem::PointerEvents { id, events });
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
                // Snap, paging and deceleration ride on ScrollContent. A
                // size-only send drops a later toggle (the gallery's
                // snapToInterval switch).
                if scroll.content.is_some() {
                    self.refresh_scroll_content(key);
                }
            }
            if flags & dirty::TEXT_MOUNT != 0 {
                self.queue_text(key);
            }
            if flags & (dirty::INPUT | dirty::INPUT_VALUE) != 0 {
                self.emit_input(key, id, flags);
            }
            if flags & dirty::REFRESHING != 0
                && let Some(scroll) = self.nodes[key as usize].scroll.as_ref()
            {
                self.mount.push(MountItem::Refreshing {
                    id,
                    refreshing: scroll.refreshing,
                });
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
            if flags & dirty::SCREEN != 0
                && let Some(config) = self.screen_config(key)
            {
                // Before the layout that follows, with the header height it
                // measured last (layout_detached emits again if it moves).
                self.mount.push(MountItem::ScreenConfig {
                    id,
                    config: Box::new(config),
                });
            }
            if flags & dirty::MODAL != 0
                && let Some(root) = self.nodes[key as usize].root.as_ref()
            {
                let config = root.modal;
                self.mount.push(MountItem::ModalConfig { id, config });
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
        // An attachment's frame is where `place_attachments` put its
        // placeholder. Its taffy node is its own root, so the location is
        // (0, 0); writing that on a later layout stacks every inline view
        // at the text's origin.
        let follows_text =
            self.nodes[key as usize].inline && self.nodes[key as usize].kind != Kind::Text;
        if !follows_text {
            self.set_frame(key, frame);
        }
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
        let selectable = self.resolved_user_select(key);
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
            selectable,
            selection_color: node
                .text_style
                .selection_color
                .or_else(|| node.class_props.as_ref().and_then(|p| p.selection_color)),
            ..AttributedText::default()
        };
        let base = inherited.inherit(&node.text_style);
        let inserted = self.update_attachments(key);
        let mut alignments = AttachmentAlignments::default();
        self.collect_text(key, base, &mut text, &mut alignments);
        // Its own `href` covers everything it draws.
        if let Some(url) = self.nodes[key as usize].href.clone() {
            text.hrefs.push(TextHref {
                start: 0,
                end: text.len_utf16(),
                node: self.nodes[key as usize].id,
                url,
            });
        }
        let layout = self.nodes[key as usize]
            .layout
            .expect("text block has layout");
        let cx = self.text_cx(layout);
        if *cx.text == text && cx.alignments == alignments {
            if inserted {
                // New attachment views: placed with the next final layout.
                cx.laid_out = None;
            }
            return inserted;
        }
        // A test id is not a glyph: keep the layout and remount the text.
        if cx.text.test_ids != text.test_ids
            && !inserted
            && cx.alignments == alignments
            && cx.text.rendering_eq(&text)
        {
            cx.text = Arc::new(text);
            cx.metadata_changed = true;
            return true;
        }
        cx.metadata_changed = false;
        let same_layout = cx.text.layout_eq(&text) && cx.alignments == alignments;
        cx.alignments = alignments;
        cx.text = Arc::new(text);
        cx.prepared = None;
        cx.min_content = None;
        cx.laid_out = None;
        if !same_layout {
            let has_alignments = !cx.alignments.is_empty();
            cx.layout_hash = cx.alignments.layout_hash(&cx.text);
            cx.cache = None;
            // A text that measures as before leaves every layout standing:
            // only the text is mounted again. Attachments are placed by its
            // final layout, laid out again with the rest.
            let attachments = has_alignments || !self.nodes[key as usize].attached.is_empty();
            let engine = &self.text_engine;
            let cache = &mut self.measure_cache;
            if attachments
                || !self
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

    /// The `userSelect` of `key` and the views around it: `Some(true)` is
    /// `"text"`, `Some(false)` is `"none"`, `None` is `"auto"` everywhere.
    fn resolved_user_select(&self, key: Key) -> Option<bool> {
        let mut k = key;
        while k != NO_KEY {
            if let Some(selectable) = self.nodes[k as usize].user_select {
                return Some(selectable);
            }
            k = self.nodes[k as usize].parent;
        }
        None
    }

    /// Collects `key`'s characters into `out`: raw text, its spans and
    /// their `href`s, and its attachments' placeholders.
    fn collect_text(
        &self,
        key: Key,
        style: ResolvedTextStyle,
        out: &mut AttributedText,
        alignments: &mut AttachmentAlignments,
    ) {
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
                    let href = child.href.clone();
                    self.collect_text(c, inner, out, alignments);
                    let end = out.len_utf16();
                    if events.contains(EventKind::Press) {
                        out.links.push(crate::text::TextLink {
                            start,
                            end,
                            node: id,
                            events,
                        });
                    }
                    if let Some(url) = href {
                        out.hrefs.push(TextHref {
                            start,
                            end,
                            node: id,
                            url,
                        });
                    }
                    if let Some(test_id) = test_id {
                        out.test_ids.push(SpanTestId {
                            start,
                            end,
                            test_id,
                        });
                    }
                }
                // An attachment, laid out by `update_attachments`: one
                // placeholder of its own size.
                _ => {
                    if let Some(layout) = child.layout {
                        let size = self.rounded_layout(layout).size;
                        alignments.push(
                            out,
                            crate::geometry::size_pt(size.width, size.height),
                            child.vertical_align,
                            style,
                        );
                    }
                }
            }
        }
    }

    /// The attachments of text block `key` (views and images inside it,
    /// also inside its spans), in text order.
    fn attachments_of(&self, key: Key, out: &mut Vec<Key>) {
        for &c in &self.nodes[key as usize].children {
            let child = &self.nodes[c as usize];
            match child.kind {
                Kind::RawText => {}
                Kind::Text => self.attachments_of(c, out),
                _ if child.layout.is_some() => out.push(c),
                _ => {}
            }
        }
    }

    /// The text block an attachment is in, if it is one.
    fn attachment_block(&self, key: Key) -> Option<Key> {
        let node = &self.nodes[key as usize];
        if !node.inline || node.layout.is_none() || node.kind == Kind::Text {
            return None;
        }
        let mut k = node.parent;
        while k != NO_KEY {
            let n = &self.nodes[k as usize];
            if !n.inline {
                return (n.kind == Kind::Text).then_some(k);
            }
            k = n.parent;
        }
        None
    }

    /// Lays out the attachments of text block `key` (each a layout root at
    /// its own content size) and keeps their native views inside the
    /// block's. Returns true if views were inserted (they need placing).
    fn update_attachments(&mut self, key: Key) -> bool {
        let mut keys = Vec::new();
        self.attachments_of(key, &mut keys);
        let block = &self.nodes[key as usize];
        if keys.is_empty() && block.attached.is_empty() {
            return false;
        }
        let (block_id, created) = (block.id, block.created);
        let old = mem::take(&mut self.nodes[key as usize].attached);
        let mut inserted = false;
        for &a in &keys {
            let id = self.nodes[a as usize].id;
            let new = !old.contains(&id);
            if created && new {
                self.ensure_created(a);
                self.mount.push(MountItem::Insert {
                    parent: block_id,
                    child: id,
                    before: None,
                });
                inserted = true;
            }
            let layout = self.nodes[a as usize].layout.expect("attachment layout");
            let dirty = self.layout.dirty(layout);
            if dirty {
                self.layout_root(
                    layout,
                    taffy::Size {
                        width: AvailableSpace::MaxContent,
                        height: AvailableSpace::MaxContent,
                    },
                );
            }
            if created && (dirty || new) {
                // Its own frame follows the text; its subtree's now.
                let children = mem::take(&mut self.nodes[a as usize].children);
                for &child in &children {
                    self.emit_frames(child);
                }
                self.nodes[a as usize].children = children;
            }
        }
        // Views that left the block and were not moved natively elsewhere.
        for id in old {
            let Ok(k) = self.key(id) else { continue };
            let n = &self.nodes[k as usize];
            let moved = n.parent != NO_KEY && !n.inline && self.nodes[n.parent as usize].created;
            if n.created && !moved && self.attachment_block(k).is_none() {
                self.mount.push(MountItem::Remove { id });
            }
        }
        let ids: Vec<NodeId> = keys.iter().map(|&a| self.nodes[a as usize].id).collect();
        let has = !ids.is_empty();
        self.nodes[key as usize].attached = if created { ids } else { Vec::new() };
        match (has, self.attachment_blocks.contains(&key)) {
            (true, false) => self.attachment_blocks.push(key),
            (false, true) => self.attachment_blocks.retain(|&k| k != key),
            _ => {}
        }
        inserted
    }

    /// Text blocks whose attachments' content changed are built again.
    fn check_attachments(&mut self) {
        let mut i = 0;
        while i < self.attachment_blocks.len() {
            let key = self.attachment_blocks[i];
            i += 1;
            let Some(node) = self.nodes.get(key as usize) else {
                continue;
            };
            let dirty = node.attached.iter().any(|&id| {
                self.key(id).is_ok_and(|k| {
                    self.nodes[k as usize]
                        .layout
                        .is_some_and(|l| self.layout.dirty(l))
                })
            });
            if dirty {
                self.mark(key, dirty::TEXT_BUILD);
            }
        }
    }

    /// Moves the views of text block `key`'s attachments onto their
    /// placeholders in `layout` (content box `content`); cut off ones out
    /// of sight.
    fn place_attachments(&mut self, key: Key, layout: &TextLayout, content: Bounds<Points>) {
        let points = self.text_engine.attachments(layout);
        let attached = mem::take(&mut self.nodes[key as usize].attached);
        for (i, &id) in attached.iter().enumerate() {
            let Ok(a) = self.key(id) else { continue };
            let Some(l) = self.nodes[a as usize].layout else {
                continue;
            };
            let size = self.rounded_layout(l).size;
            let (x, y) = match points.get(i).copied().flatten() {
                Some(p) => (content.origin.x.0 + p.x.0, content.origin.y.0 + p.y.0),
                // Far away, like unbound list cells.
                None => (-100_000.0, 0.0),
            };
            self.set_frame(a, rect(x, y, size.width, size.height));
        }
        self.nodes[key as usize].attached = attached;
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
        let mut placements: Vec<(Key, TextLayout, Bounds<Points>)> = Vec::new();
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
                    let prepared = cx
                        .prepared
                        .get_or_insert_with(|| engine.prepare_aligned(&cx.text, &cx.alignments));
                    engine.layout(prepared, size)
                }
            };
            cx.laid_out = Some(LaidText {
                content,
                layout: text_layout.clone(),
            });
            if !cx.text.attachments.is_empty() {
                placements.push((key, text_layout.clone(), content));
            }
            self.mount.push(MountItem::Text {
                id,
                text: cx.text.clone(),
                layout: text_layout,
                content,
            });
        }
        for (key, layout, content) in placements {
            self.place_attachments(key, &layout, content);
        }
        // A layout that did not rebuild the text still walked these views
        // (`display: none` zeroes them). Put back the placeholders of the
        // blocks that are showing.
        self.restore_attachments();
    }

    /// Puts every showing text block's attachments back on the placeholders
    /// of the layout already sent.
    fn restore_attachments(&mut self) {
        let blocks = self.attachment_blocks.clone();
        for key in blocks {
            if self.displays_none(key) {
                continue;
            }
            let Some(layout_id) = self.nodes.get(key as usize).and_then(|node| node.layout) else {
                continue;
            };
            let placed = match self.layout.get_node_context_mut(layout_id) {
                Some(LeafContext::Text(cx)) => cx
                    .laid_out
                    .as_ref()
                    .map(|laid| (laid.layout.clone(), laid.content)),
                _ => None,
            };
            if let Some((layout, content)) = placed {
                self.place_attachments(key, &layout, content);
            }
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
            let prepared = cx
                .prepared
                .get_or_insert_with(|| engine.prepare_aligned(&cx.text, &cx.alignments));
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

/// The layout of a parent-viewport list's spacer: as long as the list's
/// content along the main axis.
fn spacer_style(len: f32) -> taffy::Style {
    taffy::Style {
        size: taffy::Size {
            width: taffy::Dimension::auto(),
            height: taffy::Dimension::length(len.max(0.0)),
        },
        flex_shrink: 0.0,
        ..Default::default()
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
        if self.nodes[key as usize].kind == Kind::List
            && self.nodes[key as usize]
                .list
                .as_ref()
                .is_some_and(|list| list.viewport_parent)
        {
            style.overflow = taffy::Point {
                x: taffy::Overflow::Hidden,
                y: taffy::Overflow::Hidden,
            };
            self.layout.set_style(layout, style);
            self.needs_layout = true;
            return;
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
    /// Number of screens / modals / header items above `key`.
    fn root_depth(&self, mut key: Key) -> u32 {
        let mut depth = 0;
        key = self.nodes[key as usize].parent;
        while key != NO_KEY {
            let n = &self.nodes[key as usize];
            if n.kind.is_layout_root() {
                depth += 1;
            }
            key = n.parent;
        }
        depth
    }

    /// How far the system UI at the top reaches into `key`:
    /// its root's top inset minus its offset in that root.
    fn top_overlap(&self, key: Key) -> f32 {
        let mut y = 0.0;
        let mut k = key;
        loop {
            let n = &self.nodes[k as usize];
            if k == self.root {
                return (self.safe_area.top.0 - y).max(0.0);
            }
            if n.kind.is_layout_root() {
                let inset = match n.kind {
                    Kind::Modal => n.root.as_ref().map_or(0.0, |r| r.insets.top.0),
                    _ => 0.0,
                };
                return (inset - y).max(0.0);
            }
            if !n.frame.origin.y.0.is_nan() {
                y += n.frame.origin.y.0;
            }
            if n.parent == NO_KEY {
                return 0.0;
            }
            k = n.parent;
        }
    }
    /// Where an opaque native header puts the content. A positive
    /// measurement wins; `Some(0)` is a transparent or hidden bar saying it
    /// does not know, not a bar of height zero.
    fn covering_header(&self, parent: Key, root: &RootState) -> f32 {
        root.header
            .filter(|h| *h > 0.0)
            .unwrap_or_else(|| self.header_heights.0.0 + self.top_overlap(parent))
    }

    /// The platform draws a header of its own above screens (iOS, Android);
    /// without one ([`NO_NATIVE_HEADER`]) a screen's `header` slot is placed
    /// at its top.
    fn native_header(&self) -> bool {
        self.header_heights != crate::NO_NATIVE_HEADER
    }

    /// The size of screen `key`: its stack's (`None` outside a stack).
    fn screen_size(&self, key: Key) -> Option<Size<Points>> {
        let parent = self.nodes[key as usize].parent;
        let p = self.nodes.get(parent as usize)?;
        let stack_layout = p.layout.filter(|_| p.kind == Kind::Stack)?;
        let s = self.rounded_layout(stack_layout).size;
        Some(crate::geometry::size_pt(s.width, s.height))
    }

    /// The `header` slot item of screen `key`, once created.
    fn header_slot(&self, key: Key) -> Option<Key> {
        self.nodes[key as usize]
            .children
            .iter()
            .copied()
            .find(|&c| {
                let n = &self.nodes[c as usize];
                n.kind == Kind::HeaderItem
                    && n.created
                    && n.root
                        .as_ref()
                        .is_some_and(|r| r.slot == crate::ops::HeaderSlot::Header)
            })
    }

    /// Without a native header: the height of screen `key`'s `header` slot,
    /// laid out first (as wide as the screen), 0 without one.
    fn drawn_header(&mut self, key: Key) -> f32 {
        let Some(item) = self.header_slot(key) else {
            return 0.0;
        };
        self.layout_detached(item);
        self.nodes[item as usize]
            .layout
            .map_or(0.0, |l| self.rounded_layout(l).size.height)
    }

    /// Lays out one layout root (Screen, Modal, HeaderItem) at the size
    /// its container gives.
    fn layout_detached(&mut self, key: Key) {
        let native_header = self.native_header();
        let drawn_header = if !native_header && self.nodes[key as usize].kind == Kind::Screen {
            self.drawn_header(key)
        } else {
            0.0
        };
        let node = &self.nodes[key as usize];
        let (Some(layout), kind, parent) = (node.layout, node.kind, node.parent) else {
            return;
        };
        let root = node.root.as_ref().expect("root state");
        let (size, header, available) = match kind {
            Kind::Screen => {
                let Some(size) = self.screen_size(key) else {
                    return;
                };
                let c = &root.screen;
                let measured = root.header.filter(|h| *h > 0.0 && !c.header_transparent);
                let header = if !native_header {
                    drawn_header
                } else if !c.header_visible {
                    0.0
                } else if let Some(measured) = measured {
                    measured
                } else {
                    self.header_heights.0.0 + self.top_overlap(parent)
                };
                (
                    size,
                    header,
                    taffy::Size {
                        width: taffy::AvailableSpace::Definite(size.width.0),
                        height: taffy::AvailableSpace::Definite(size.height.0),
                    },
                )
            }
            Kind::HeaderItem if !native_header && root.slot == crate::ops::HeaderSlot::Header => {
                let Some(screen) = self.screen_size(parent) else {
                    return;
                };
                (
                    crate::geometry::size_pt(screen.width.0, 0.0),
                    0.0,
                    taffy::Size {
                        width: taffy::AvailableSpace::Definite(screen.width.0),
                        height: taffy::AvailableSpace::MaxContent,
                    },
                )
            }
            Kind::Modal => {
                let size = root.reported.unwrap_or(self.viewport);
                (
                    size,
                    0.0,
                    taffy::Size {
                        width: taffy::AvailableSpace::Definite(size.width.0),
                        height: taffy::AvailableSpace::Definite(size.height.0),
                    },
                )
            }
            // A header item of a native header: the platform places it,
            // but its content still lays out for the frame it reports.
            _ => (
                crate::geometry::Size::default(),
                0.0,
                taffy::Size {
                    width: taffy::AvailableSpace::MaxContent,
                    height: taffy::AvailableSpace::MaxContent,
                },
            ),
        };
        let changed = root.laid_out != Some((size, header));
        if !changed && !self.layout.dirty(layout) {
            return;
        }
        if kind == Kind::HeaderItem && size.width > Points::ZERO {
            let mut style = self.layout.style(layout).clone();
            style.size.width = taffy::Dimension::length(size.width.0);
            style.size.height = taffy::Dimension::auto();
            self.layout.set_style(layout, style);
        } else if kind != Kind::HeaderItem {
            let config_padding = match kind {
                // A transparent header stays on top: the system bar on
                // iOS and Android, the app's `header` slot on the web.
                // Content starts at the top and shows through it.
                Kind::Screen if root.screen.header_transparent => 0.0,
                _ => header,
            };
            let mut style = self.nodes[key as usize]
                .base_layout
                .as_deref()
                .cloned()
                .unwrap_or_default();
            style.size = taffy::Size {
                width: taffy::Dimension::length(size.width.0),
                height: taffy::Dimension::length(size.height.0),
            };
            if config_padding > 0.0 {
                let base = match style.padding.top.into_raw() {
                    raw if raw.tag() == taffy::CompactLength::LENGTH_TAG => raw.value(),
                    _ => 0.0,
                };
                style.padding.top = taffy::LengthPercentage::length(base + config_padding);
            }
            self.layout.set_style(layout, style);
        }
        let header_changed = root.laid_out.is_none_or(|(_, h)| h != header);
        self.nodes[key as usize]
            .root
            .as_mut()
            .expect("root state")
            .laid_out = Some((size, header));
        self.layout_root(layout, available);
        self.emit_frames(key);
        if kind == Kind::Screen && header_changed {
            let id = self.nodes[key as usize].id;
            if let Some(config) = self.screen_config(key) {
                self.mount.push(MountItem::ScreenConfig {
                    id,
                    config: Box::new(config),
                });
            }
        }
    }

    /// The options of screen `key`, with its header items.
    fn screen_config(&self, key: Key) -> Option<crate::ScreenConfig> {
        let node = &self.nodes[key as usize];
        let root = node.root.as_ref().filter(|_| node.kind == Kind::Screen)?;
        let mut config = root.screen.clone();
        config.header_height = Points(root.laid_out.map_or(0.0, |(_, h)| h));
        for &c in &node.children {
            let child = &self.nodes[c as usize];
            if child.kind != Kind::HeaderItem || !child.created {
                continue;
            }
            let slot = child
                .root
                .as_ref()
                .map_or(crate::ops::HeaderSlot::Left, |r| r.slot);
            let target = match slot {
                crate::ops::HeaderSlot::Left => &mut config.left,
                crate::ops::HeaderSlot::Right => &mut config.right,
                crate::ops::HeaderSlot::Title => &mut config.title_view,
                crate::ops::HeaderSlot::Header => &mut config.header_view,
            };
            *target = Some(child.id);
        }
        Some(config)
    }

    /// Lays out the tree under `root` within `available`.
    fn layout_root(&mut self, root: LayoutId, available: taffy::Size<taffy::AvailableSpace>) {
        let engine = &self.text_engine;
        let cache = &mut self.measure_cache;
        self.layout
            .compute_layout_with_measure(root, available, |input, _, cx, style| {
                measure_leaf(engine, cache, input, cx, style)
            });
    }

    /// Lays out layout roots (screens, modals, header items) in depth order.
    fn layout_roots(&mut self) {
        if self.roots.is_empty() {
            return;
        }
        let mut order: Vec<(u32, Key)> = self
            .roots
            .iter()
            .filter(|&&k| {
                let n = &self.nodes[k as usize];
                n.created && n.parent != NO_KEY
            })
            .map(|&k| (self.root_depth(k), k))
            .collect();
        order.sort_unstable();
        for (_, key) in order {
            self.layout_detached(key);
        }
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
            let viewport = if self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .viewport_parent
            {
                // Find parent scroll view and use its viewport
                let mut parent_key = self.nodes[key as usize].parent;
                let mut parent_viewport = 0.0;

                while parent_key != NO_KEY {
                    if self.nodes[parent_key as usize].kind == Kind::Scroll {
                        parent_viewport = self.nodes[parent_key as usize]
                            .layout
                            .map(|layout| self.rounded_layout(layout).size.height)
                            .unwrap_or(0.0);
                        break;
                    }
                    parent_key = self.nodes[parent_key as usize].parent;
                }

                parent_viewport
            } else {
                self.nodes[key as usize]
                    .layout
                    .map(|layout| self.rounded_layout(layout).size.height)
                    .unwrap_or(0.0)
            };

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
                // Inverted lists start at the end and keep that distance as
                // content or the viewport changes. Item 0 stays in view.
                anchor += list.anchor_end();
                if let Some(target) = list.follow_pin(anchor) {
                    scrolls.push((target, true));
                }
            }

            // Scroll commands for the list, now that sizes are known.
            // A command replaces a pin retarget from this same frame:
            // both would be `ScrollTo`s, and the earlier one would win
            // on the platform after the batch.
            let id = self.nodes[key as usize].id;
            let mut commands = Vec::new();
            for command in &self.commands {
                if command.0 == id {
                    commands.push(command.1.clone());
                }
            }
            self.commands.retain(|c| c.0 != id);
            if !commands.is_empty() {
                scrolls.clear();
            }
            for command in commands {
                match command {
                    Command::ScrollToIndex { index, animated } => {
                        let target = list::PinTarget::Index(index);
                        let list = self.nodes[key as usize].list.as_mut().expect("list");
                        let offset = list.target_offset(target);
                        // The window jumps only when the platform moves
                        // now. An animation stays on the reported offset:
                        // jumping the cells to the destination while the
                        // scroll view is still showing the old rect blanks
                        // a long list (nsh does the same).
                        list.pin(Some(target), offset, animated);
                        if !animated {
                            list.jump(offset);
                        }
                        scrolls.push((offset, animated));
                    }
                    Command::ScrollToEnd { animated } => {
                        // The core owns the content length: the end is an
                        // offset it can name, and it follows it as rows
                        // measure, like a pinned index.
                        let target = list::PinTarget::End;
                        let list = self.nodes[key as usize].list.as_mut().expect("list");
                        let offset = list.target_offset(target);
                        list.pin(Some(target), offset, animated);
                        if !animated {
                            list.jump(offset);
                        }
                        scrolls.push((offset, animated));
                    }
                    _ => {}
                }
            }

            // Content can shrink below the offset (the last page cleared):
            // follow the platform's clamp before placing the window.
            let content_len = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .content_len();
            let parent_mode = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .viewport_parent;
            if parent_mode {
                // The page around it learns the list's length, and the
                // list follows where that page is scrolled to.
                self.sync_list_spacer(key);
                if let Some((offset, horizontal)) = self.outer_scroll_offset(key) {
                    self.update_viewport_parent_list_offset(key, offset, horizontal);
                }
            } else {
                let max_offset = (content_len - viewport).max(0.0);
                if self.nodes[key as usize].list.as_ref().expect("list").offset > max_offset {
                    let animating = self.nodes[key as usize]
                        .list
                        .as_ref()
                        .expect("list")
                        .pin_animating();
                    let list = self.nodes[key as usize].list.as_mut().expect("list");
                    list.jump(max_offset);
                    // An in-flight `scrollTo` already names its end. Another
                    // jump here cancels that animation and stops short.
                    if !animating {
                        scrolls.push((max_offset, false));
                    }
                }
            }

            let mut binds = Vec::new();
            let load_more = {
                let list = self.nodes[key as usize].list.as_mut().expect("list");
                list.update_window(&mut binds)
            };
            if load_more {
                self.events.push((id, EventKind::LoadMore));
            }
            // Check viewable items range
            if self.nodes[key as usize]
                .list
                .as_mut()
                .expect("list")
                .check_viewable_items()
                .is_some()
            {
                self.events.push((id, EventKind::ViewableItemsChange));
            }
            if !binds.is_empty() {
                self.list_binds.push(ListBind { list: id, binds });
            }
            {
                let list = self.nodes[key as usize].list.as_mut().expect("list");
                if list.paced && list.has_paced() && !self.paced_lists.contains(&id) {
                    self.paced_lists.push(id);
                }
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

                let (adjusted_at, sticky) = {
                    let list = self.nodes[key as usize].list.as_ref().expect("list");
                    let base_at = list.to_content(at, size.height);
                    let sticky =
                        list.sticky_enabled() && list.sticky_indices.binary_search(&index).is_ok();
                    (base_at + list.sticky_shift(index, size.height), sticky)
                };

                let frame = rect(column, adjusted_at, width, size.height);
                self.set_frame(cell, frame);
                self.set_cell_sticky(cell, sticky);
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
                    let at = self.nodes[key as usize]
                        .list
                        .as_ref()
                        .expect("list")
                        .to_content(ats[i], size.height);
                    let frame = rect(full_x, at, full_width, size.height);
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
            let viewport_parent = self.nodes[key as usize]
                .list
                .as_ref()
                .expect("list")
                .viewport_parent;

            if !viewport_parent {
                // Only set scroll content if list has its own scroll view
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
        self.write_scroll_content(key, size, false);
    }

    /// Sends the current content again so snap, paging and deceleration
    /// reach the platform when the size did not change.
    fn refresh_scroll_content(&mut self, key: Key) {
        let Some(size) = self.nodes[key as usize]
            .scroll
            .as_ref()
            .and_then(|scroll| scroll.content)
        else {
            return;
        };
        self.write_scroll_content(key, size, true);
    }

    fn write_scroll_content(&mut self, key: Key, size: Size<Points>, force: bool) {
        let (
            id,
            rtl,
            kind,
            viewport,
            horizontal,
            old,
            content_item,
            refresh_indicator,
            paging,
            snap_interval,
            snap_offsets,
            snap_alignment,
            disable_interval_momentum,
            deceleration,
        ) = {
            let node = &mut self.nodes[key as usize];
            let Some(scroll) = node.scroll.as_mut() else {
                return;
            };
            if !force && scroll.content == Some(size) {
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
                scroll.refresh_indicator,
                scroll.paging,
                scroll.snap_interval,
                scroll.snap_offsets.clone(),
                scroll.snap_alignment,
                scroll.disable_interval_momentum,
                scroll.deceleration,
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
            self.mount[j] = MountItem::ScrollContent {
                id,
                size,
                refresh_indicator,
                paging,
                snap_interval,
                snap_offsets: snap_offsets.clone(),
                snap_alignment: snap_alignment as u8,
                disable_interval_momentum,
                deceleration,
            };
        }
        // A scroll queued after it must still come before the new size.
        let scrolled_since = queued.is_some_and(|j| {
            self.mount[j + 1..].iter().any(
                |item| matches!(item, MountItem::ScrollBy { id: scrolled, .. } if *scrolled == id),
            )
        });
        if queued.is_none() || scrolled_since {
            let at = self.mount.len();
            self.mount.push(MountItem::ScrollContent {
                id,
                size,
                refresh_indicator,
                paging,
                snap_interval,
                snap_offsets,
                snap_alignment: snap_alignment as u8,
                disable_interval_momentum,
                deceleration,
            });
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
    /// The insets `env()` uses for `key`: its screen's or modal's, once
    /// reported, else an estimate. A native header covers the top until
    /// the screen reports (nsh `insets_of`), unless it is transparent:
    /// the bar still counts, the way iOS's safe area includes a
    /// transparent navigation bar, estimated as the bar plus the status
    /// inset. A drawn `header` slot (no native header) still reads the
    /// surface's top inset; the screen reports that edge as zero when
    /// the bar is opaque (the slot already covers it) and as the bar's
    /// height when the bar is transparent. Left, right and bottom stay
    /// the screen's.
    fn insets_of(&self, key: Key) -> Edges<Points> {
        let safe = self.safe_area;
        let drawn_header = self.in_drawn_header(key);
        let mut k = key;
        loop {
            let node = &self.nodes[k as usize];
            if matches!(node.kind, Kind::Screen | Kind::Modal)
                && let Some(root) = node.root.as_ref()
            {
                if root.insets_set {
                    let mut insets = root.insets;
                    if drawn_header {
                        insets.top = safe.top;
                        return insets;
                    }
                    // The report still describes the other cover. Use the
                    // bar this commit is leaving or entering, so content
                    // that pads by `env(safe-area-inset-top)` does not jump
                    // while the platform's next report is in flight.
                    if node.kind == Kind::Screen
                        && self.native_header()
                        && root.screen.header_visible
                        && root.screen.header_transparent != root.insets_transparent
                    {
                        insets.top = if root.screen.header_transparent {
                            Points(self.covering_header(node.parent, root))
                        } else {
                            Points::ZERO
                        };
                    }
                    return insets;
                }
                if node.kind == Kind::Screen && self.native_header() && root.screen.header_visible {
                    let top = if root.screen.header_transparent {
                        // The bar plus the surface's top inset: what iOS
                        // reports for a screen under the status bar. Not
                        // the stack's frame; that is not known yet, and
                        // reading it would move this inset on a later layout.
                        Points(self.header_heights.0.0 + safe.top.0)
                    } else {
                        Points::ZERO
                    };
                    return Edges { top, ..safe };
                }
                return safe;
            }
            if node.parent == NO_KEY || k == self.root {
                return safe;
            }
            k = node.parent;
        }
    }

    /// `key` is the app-drawn `header` slot, or inside it. Only when this
    /// platform has no native header: left / right / title slots are not
    /// that bar, and a native header places its items itself.
    fn in_drawn_header(&self, mut key: Key) -> bool {
        if self.native_header() {
            return false;
        }
        loop {
            let node = &self.nodes[key as usize];
            if node.kind == Kind::HeaderItem {
                return node
                    .root
                    .as_ref()
                    .is_some_and(|root| root.slot == crate::ops::HeaderSlot::Header);
            }
            if matches!(node.kind, Kind::Screen | Kind::Modal)
                || node.parent == NO_KEY
                || key == self.root
            {
                return false;
            }
            key = node.parent;
        }
    }

    fn env_insets(&self, key: Key, env: &EnvEdges) -> [Edges<Points>; 2] {
        let safe = self.insets_of(key);
        let edge = |term: Option<style::EnvTerm>| {
            term.map_or(Points::ZERO, |term| {
                env_term(safe, self.keyboard_inset, term)
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
            if self.nodes[key as usize].env.is_none() {
                stale.push(id);
                continue;
            }
            self.sync_env_key(key);
        }
        for id in stale {
            self.env_styled.remove(&id);
        }
    }

    /// Restyles `key` when the insets its `env()` terms come to changed.
    fn sync_env_key(&mut self, key: Key) {
        let Some(env) = self.nodes[key as usize].env.as_deref().map(|(env, _)| *env) else {
            return;
        };
        let now = self.env_insets(key, &env);
        let was = self.nodes[key as usize].env.as_ref().expect("env").1;
        if now == was {
            return;
        }
        self.nodes[key as usize].env.as_mut().expect("env").1 = now;
        if self.nodes[key as usize].kind == Kind::Scroll {
            self.sync_scroll_style(key);
            return;
        }
        // A layout root is laid out from its base style every time.
        if self.nodes[key as usize].kind.is_layout_root()
            && let Some(base) = self.nodes[key as usize].base_layout.as_deref_mut()
        {
            shift_insets(base, was, now);
        }
        let Some(layout) = self.nodes[key as usize].layout else {
            return;
        };
        let mut style = self.layout.style(layout).clone();
        shift_insets(&mut style, was, now);
        self.layout.set_style(layout, style);
        self.needs_layout = true;
    }

    /// `key` moved under a screen or modal. Its `env()` nodes, styled
    /// before they were inserted, follow that root's insets.
    fn refresh_env(&mut self, key: Key) {
        let mut stack = vec![key];
        let mut keys = Vec::new();
        while let Some(k) = stack.pop() {
            if self.nodes[k as usize].env.is_some() {
                keys.push(k);
            }
            stack.extend(self.nodes[k as usize].children.iter().copied());
        }
        for k in keys {
            self.sync_env_key(k);
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
mod tests;
