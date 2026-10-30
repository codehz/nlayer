//! Operations the JS renderer sends the shadow tree.
//!
//! `ResizeMode`, `ImageSource` and `ImageScales` are copied from
//! `nsh_core::ops`. The `PropEnum` derive arrives with the element macros.
//! Scroll ops are the axis lock only: no refresh, zoom or paging. `data:`
//! image bytes are not part of v1. An input is one line or several: `value`,
//! `placeholder`, `disabled`, `secureTextEntry` and `keyboardType`.
//! `activeStyle`, `hoverStyle` and `focusStyle` are paint the platform
//! applies; they are not the node's layout style.

use std::sync::Arc;

use crate::{
    animation::AnimationOp,
    geometry::{Bounds, Point, Points, Size},
    style::{ObjectPosition, Style, StylePatch},
};

/// Identifier of a node, assigned by the JS renderer.
///
/// Ids are small, dense and increasing, so the core indexes them directly.
pub type NodeId = u32;

/// Id of the surface root view. It exists before any JS runs.
pub const ROOT_ID: NodeId = 1;

/// Element tag of a host node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// A container view (`<view>`).
    View,
    /// A text block, or a span when nested in another text (`<text>`).
    Text,
    /// An image (`<image>`).
    Image,
    /// A scroll view (`<scroll>`). One axis: vertical, or horizontal with
    /// [`Prop::Horizontal`].
    Scroll,
    /// A virtualized, recycling list (`<list>`). Its cells are `<cell>`
    /// children, each a layout root of its own.
    List,
    /// A recycled list cell (`<cell>`), created by the list's runtime.
    Cell,
    /// A text field, one line or several (`<input>`).
    Input,
}
impl Tag {
    /// Parses a host element's name (what `nlayer:rt`'s `element` takes).
    #[inline]
    pub fn from_name(name: &str) -> Option<Tag> {
        match name {
            "view" => Some(Tag::View),
            "text" => Some(Tag::Text),
            "image" => Some(Tag::Image),
            "scroll" => Some(Tag::Scroll),
            "list" => Some(Tag::List),
            "cell" => Some(Tag::Cell),
            "input" => Some(Tag::Input),
            _ => None,
        }
    }
}

/// An event a node can listen to.
///
/// Discriminant order is the bit in [`EventMask`]. Append new kinds; do not
/// reorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EventKind {
    /// A tap / click.
    Press,
    /// A scroll view moved, at most once per frame.
    Scroll,
    /// Scrolling came to rest.
    ScrollEnd,
    /// The user started dragging a scroll view.
    ScrollDragStart,
    /// The user let go of a scroll view.
    ScrollDragEnd,
    /// A scroll view started gliding after a fling.
    MomentumStart,
    /// A scroll view stopped gliding.
    MomentumEnd,
    /// The user edited an input (`onChange`).
    Change,
    /// The selection or caret of an input moved (`onSelectionChange`).
    SelectionChange,
    /// An input method's composition started, changed or ended
    /// (`onCompositionChange`).
    CompositionChange,
    /// The list's end came near (`onLoadMore`).
    LoadMore,
    /// The pointer stayed down for `delayLongPress` (`onLongPress`).
    LongPress,
}

impl EventKind {
    /// Parses an event's protocol name such as `onPress`.
    #[inline]
    pub fn from_prop(name: &str) -> Option<EventKind> {
        match name {
            "onPress" => Some(EventKind::Press),
            "onScroll" => Some(EventKind::Scroll),
            "onScrollEnd" => Some(EventKind::ScrollEnd),
            "onScrollDragStart" => Some(EventKind::ScrollDragStart),
            "onScrollDragEnd" => Some(EventKind::ScrollDragEnd),
            "onMomentumStart" => Some(EventKind::MomentumStart),
            "onMomentumEnd" => Some(EventKind::MomentumEnd),
            "onChange" => Some(EventKind::Change),
            "onSelectionChange" => Some(EventKind::SelectionChange),
            "onCompositionChange" => Some(EventKind::CompositionChange),
            "onLoadMore" => Some(EventKind::LoadMore),
            "onLongPress" => Some(EventKind::LongPress),
            _ => None,
        }
    }

    /// The [`EventMask`] bit index. Platforms pass this across JNI.
    /// `None` if `raw` is not a kind.
    #[inline]
    pub const fn from_raw(raw: u8) -> Option<EventKind> {
        match raw {
            0 => Some(EventKind::Press),
            1 => Some(EventKind::Scroll),
            2 => Some(EventKind::ScrollEnd),
            3 => Some(EventKind::ScrollDragStart),
            4 => Some(EventKind::ScrollDragEnd),
            5 => Some(EventKind::MomentumStart),
            6 => Some(EventKind::MomentumEnd),
            7 => Some(EventKind::Change),
            8 => Some(EventKind::SelectionChange),
            9 => Some(EventKind::CompositionChange),
            10 => Some(EventKind::LoadMore),
            11 => Some(EventKind::LongPress),
            _ => None,
        }
    }

    /// The protocol name of this event.
    #[inline]
    pub const fn prop_name(self) -> &'static str {
        match self {
            EventKind::Press => "onPress",
            EventKind::Scroll => "onScroll",
            EventKind::ScrollEnd => "onScrollEnd",
            EventKind::ScrollDragStart => "onScrollDragStart",
            EventKind::ScrollDragEnd => "onScrollDragEnd",
            EventKind::MomentumStart => "onMomentumStart",
            EventKind::MomentumEnd => "onMomentumEnd",
            EventKind::Change => "onChange",
            EventKind::SelectionChange => "onSelectionChange",
            EventKind::CompositionChange => "onCompositionChange",
            EventKind::LoadMore => "onLoadMore",
            EventKind::LongPress => "onLongPress",
        }
    }
}

/// A set of event kinds: one bit per kind, room for 128.
///
/// Two words, not a `u128`: views keep it in Objective-C ivars, which take
/// no 16-byte alignment. Copied from `nsh_core::ops`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EventMask([u64; 2]);

impl std::ops::BitOr for EventMask {
    type Output = EventMask;

    #[inline]
    fn bitor(self, other: EventMask) -> EventMask {
        EventMask([self.0[0] | other.0[0], self.0[1] | other.0[1]])
    }
}

impl EventMask {
    /// Returns true if `event` is in the set.
    #[inline]
    pub const fn contains(self, event: EventKind) -> bool {
        let k = event as usize;
        self.0[k / 64] & (1 << (k % 64)) != 0
    }

    /// The raw bit set, indexed by `EventKind as u8`.
    #[inline]
    pub const fn bits(self) -> u128 {
        self.0[0] as u128 | (self.0[1] as u128) << 64
    }

    /// Adds or removes `event`.
    #[inline]
    pub fn set(&mut self, event: EventKind, enabled: bool) {
        let k = event as usize;
        if enabled {
            self.0[k / 64] |= 1 << (k % 64);
        } else {
            self.0[k / 64] &= !(1 << (k % 64));
        }
    }

    /// Returns true if no event is set.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0[0] == 0 && self.0[1] == 0
    }
}

/// How an image fits its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResizeMode {
    /// Fill the box, cropping (default).
    #[default]
    Cover,
    /// Fit inside the box, letterboxing.
    Contain,
    /// Stretch to the box.
    Stretch,
    /// Center at natural size, cropped where larger than the box (`object-fit: none`).
    Center,
    /// Center at natural size, shrunk to fit the box where larger (`object-fit: scale-down`).
    /// JS name: `scale-down`.
    ScaleDown,
}

impl ResizeMode {
    /// Where content of `natural` size goes in a box of `size`.
    pub fn fit(
        self,
        natural: Size<Points>,
        size: Size<Points>,
        position: ObjectPosition,
    ) -> Bounds<Points> {
        let (nw, nh) = (natural.width.0, natural.height.0);
        if nw <= 0.0 || nh <= 0.0 || self == ResizeMode::Stretch {
            return Bounds {
                origin: Point::default(),
                size,
            };
        }
        let contain = (size.width.0 / nw).min(size.height.0 / nh);
        let scale = match self {
            ResizeMode::Cover => (size.width.0 / nw).max(size.height.0 / nh),
            ResizeMode::Contain => contain,
            ResizeMode::Center => 1.0,
            ResizeMode::ScaleDown => contain.min(1.0),
            ResizeMode::Stretch => 1.0,
        };
        let content = Size {
            width: Points(nw * scale),
            height: Points(nh * scale),
        };
        Bounds {
            origin: position.place(size, content),
            size: content,
        }
    }
}

/// An image source.
///
/// Copied from `nsh_core::ops`. v1 sources are a bundled `asset:` URI or an
/// `http(s)` URL. Bytes carried in the URI (`data:`) are not decoded here.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImageSource {
    /// `https://…` or `asset:/path` (bundled file).
    pub uri: Arc<str>,
    /// Natural size in points, when known up front (bundled assets): used
    /// as the intrinsic layout size.
    pub size: Option<Size<Points>>,
    /// The pixel densities a bundled asset ships in (`a.png`,
    /// `a@2x.png`, ...): the core mounts the one for the screen's pixel
    /// ratio. Empty for a single file.
    pub scales: ImageScales,
}

impl ImageSource {
    /// The URI to load at `pixel_ratio` device pixels per point: the
    /// variant [`ImageScales::pick`] chooses (`asset:/a.png` at 2 →
    /// `asset:/a@2x.png`), or [`Self::uri`] itself when there are no
    /// scales, the pick is the 1x file or the URI is not a plain
    /// `asset:/` path.
    pub(crate) fn uri_for(&self, pixel_ratio: f32) -> Arc<str> {
        match self.variant(pixel_ratio) {
            Some(scale) => variant_uri(&self.uri, scale).into(),
            None => self.uri.clone(),
        }
    }

    /// The scale of the variant loaded at `pixel_ratio`, when it is not
    /// [`Self::uri`] itself. A query or a fragment is no part of a file
    /// name the bundler wrote, so such a URI is loaded as it is.
    pub(crate) fn variant(&self, pixel_ratio: f32) -> Option<f32> {
        if self.scales.is_empty()
            || !self.uri.starts_with("asset:/")
            || self.uri.contains(['?', '#'])
        {
            return None;
        }
        self.scales.pick(pixel_ratio).filter(|s| *s != 1.0)
    }

    /// This source as the platform loads it at `pixel_ratio`: the chosen
    /// variant's URI and the same size in points.
    pub(crate) fn resolved(&self, pixel_ratio: f32) -> ImageSource {
        ImageSource {
            uri: self.uri_for(pixel_ratio),
            size: self.size,
            scales: ImageScales::NONE,
        }
    }
}

/// `asset:/dir/a.png` at 2 → `asset:/dir/a@2x.png` (`@1.5x` at 1.5).
fn variant_uri(uri: &str, scale: f32) -> String {
    let name = uri.rfind('/').map_or(0, |i| i + 1);
    let (stem, ext) = match uri[name..].rfind('.') {
        Some(dot) if dot > 0 => uri.split_at(name + dot),
        _ => (uri, ""),
    };
    format!("{stem}@{scale}x{ext}")
}

/// The pixel densities an image asset can ship in, as the file name
/// suffix says (`@1.5x` ... `@4x`; the file without a suffix is 1x). The
/// bundler looks for these siblings of an imported image.
pub const IMAGE_SCALES: [f32; 5] = [1.0, 1.5, 2.0, 3.0, 4.0];

/// A set of [`IMAGE_SCALES`]: the variants an image asset has. Collected
/// from scales (`[1.0, 2.0].into_iter().collect()`), it keeps those of
/// [`IMAGE_SCALES`] and leaves any other value (NaN, negative, `2.5`)
/// out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct ImageScales(u8);

impl ImageScales {
    /// No variants: the URI is the only file.
    pub const NONE: ImageScales = ImageScales(0);

    /// This set with `scale` added, when it is one of [`IMAGE_SCALES`].
    #[must_use]
    pub(crate) fn with(self, scale: f32) -> ImageScales {
        match IMAGE_SCALES.iter().position(|s| *s == scale) {
            Some(i) => ImageScales(self.0 | 1 << i),
            None => self,
        }
    }

    /// Whether the set has no scale.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The scales in the set, smallest first.
    pub fn iter(self) -> impl Iterator<Item = f32> {
        IMAGE_SCALES
            .into_iter()
            .enumerate()
            .filter(move |(i, _)| self.0 & (1 << i) != 0)
            .map(|(_, s)| s)
    }

    /// The scale to load at `pixel_ratio`, as React Native picks: the
    /// smallest that is at least the ratio, otherwise the largest. `None`
    /// for an empty set.
    pub(crate) fn pick(self, pixel_ratio: f32) -> Option<f32> {
        self.iter()
            .find(|s| *s >= pixel_ratio)
            .or_else(|| self.iter().last())
    }
}

impl FromIterator<f32> for ImageScales {
    fn from_iter<I: IntoIterator<Item = f32>>(values: I) -> ImageScales {
        values.into_iter().fold(Self::NONE, ImageScales::with)
    }
}

/// Keyboard layout of a single-line input (`keyboardType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardType {
    /// Full keyboard (`text`).
    #[default]
    Text,
    /// E-mail addresses (`email`).
    Email,
    /// Numbers (`number`).
    Number,
    /// Phone numbers (`phone`).
    Phone,
}

impl KeyboardType {
    /// Parses a `keyboardType` value.
    #[inline]
    pub fn from_name(name: &str) -> Option<KeyboardType> {
        match name {
            "text" => Some(KeyboardType::Text),
            "email" => Some(KeyboardType::Email),
            "number" => Some(KeyboardType::Number),
            "phone" => Some(KeyboardType::Phone),
            _ => None,
        }
    }
}

/// Whether a tap in a scroll view dismisses the keyboard
/// (`keyboardShouldPersistTaps`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardTaps {
    /// The tap dismisses the keyboard and reaches nothing else (default).
    #[default]
    Never,
    /// The tap never dismisses the keyboard.
    Always,
    /// The tap dismisses the keyboard unless an `onPress` or an input handles it.
    Handled,
}

impl KeyboardTaps {
    /// Parses `never` / `always` / `handled`. A boolean is the caller's.
    #[inline]
    pub fn from_name(name: &str) -> Option<KeyboardTaps> {
        match name {
            "never" => Some(KeyboardTaps::Never),
            "always" => Some(KeyboardTaps::Always),
            "handled" => Some(KeyboardTaps::Handled),
            _ => None,
        }
    }

    /// The prop string.
    #[inline]
    pub fn name(self) -> &'static str {
        match self {
            KeyboardTaps::Never => "never",
            KeyboardTaps::Always => "always",
            KeyboardTaps::Handled => "handled",
        }
    }
}

/// When a drag dismisses the keyboard (`keyboardDismissMode`).
///
/// `on-drag` and `interactive` both dismiss once the drag along this
/// scroll's own axis passes the platform slop. The keyboard does not
/// follow the finger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardDismiss {
    /// A drag leaves the keyboard up (default).
    #[default]
    None,
    /// Hide it when a drag starts along this scroll's axis.
    OnDrag,
    /// Same as [`KeyboardDismiss::OnDrag`] on every v1 host.
    Interactive,
}

impl KeyboardDismiss {
    /// Parses `none` / `on-drag` / `interactive`.
    #[inline]
    pub fn from_name(name: &str) -> Option<KeyboardDismiss> {
        match name {
            "none" => Some(KeyboardDismiss::None),
            "on-drag" => Some(KeyboardDismiss::OnDrag),
            "interactive" => Some(KeyboardDismiss::Interactive),
            _ => None,
        }
    }

    /// The prop string.
    #[inline]
    pub fn name(self) -> &'static str {
        match self {
            KeyboardDismiss::None => "none",
            KeyboardDismiss::OnDrag => "on-drag",
            KeyboardDismiss::Interactive => "interactive",
        }
    }
}

/// Which pointer state a [`Prop::StateStyle`] paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interaction {
    /// `activeStyle`: pressed.
    Active,
    /// `hoverStyle`: a pointer is over the view.
    Hover,
    /// `focusStyle`: the view or a field inside it has focus.
    Focus,
}

impl Interaction {
    /// Index of this state in a node's three slots.
    pub(crate) fn index(self) -> usize {
        match self {
            Interaction::Active => 0,
            Interaction::Hover => 1,
            Interaction::Focus => 2,
        }
    }

    /// The mount name (`"active"`, `"hover"`, `"focus"`).
    pub fn name(self) -> &'static str {
        match self {
            Interaction::Active => "active",
            Interaction::Hover => "hover",
            Interaction::Focus => "focus",
        }
    }

    pub(crate) fn from_index(index: usize) -> Interaction {
        match index {
            0 => Interaction::Active,
            1 => Interaction::Hover,
            _ => Interaction::Focus,
        }
    }
}

/// What an element is, for assistive technologies (`role`).
///
/// Names follow React Native's roles (and nsh's): hosts map them to their
/// own vocabulary. Copied from `nsh_core::props::AccessibilityRole` and cut
/// to the names every v2 host can express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessibilityRole {
    /// No particular role.
    #[default]
    None,
    /// A button.
    Button,
    /// A link.
    Link,
    /// A search field.
    Search,
    /// An image.
    Image,
    /// An image that acts as a button.
    ImageButton,
    /// A key of a keyboard.
    KeyboardKey,
    /// Static text.
    Text,
    /// A value that can be adjusted (slider).
    Adjustable,
    /// A section header.
    Header,
    /// A summary of the app's state.
    Summary,
    /// An alert.
    Alert,
    /// A checkbox.
    Checkbox,
    /// A combo box.
    Combobox,
    /// A menu.
    Menu,
    /// A menu bar.
    Menubar,
    /// An item of a menu.
    Menuitem,
    /// A progress bar.
    Progressbar,
    /// A radio button.
    Radio,
    /// A group of radio buttons.
    Radiogroup,
    /// A scroll bar.
    Scrollbar,
    /// A spin button.
    Spinbutton,
    /// A switch.
    Switch,
    /// A tab.
    Tab,
    /// A list of tabs.
    Tablist,
    /// A timer.
    Timer,
    /// A toolbar.
    Toolbar,
    /// A list.
    List,
    /// A grid.
    Grid,
    /// A table: rows of cells under column headers.
    Table,
    /// A row of a table.
    Row,
    /// A cell of a table's row.
    Cell,
    /// A column's header cell.
    ColumnHeader,
    /// A group of a table's rows: its header, body or footer.
    RowGroup,
    /// A list of options to choose from.
    Listbox,
    /// An option of a listbox; `accessibilityState.selected` marks the chosen
    /// one.
    Option,
    /// A group of related elements that is no landmark.
    Group,
    /// A section of the page a user may want to reach (a landmark).
    Region,
    /// A landmark of links or controls that move around the app.
    Navigation,
    /// A tree of items that expand and collapse.
    Tree,
    /// An item of a tree.
    TreeItem,
    /// An item of a menu that is checked or not.
    MenuItemCheckbox,
    /// An item of a menu that is one choice of a group of them.
    MenuItemRadio,
}

impl AccessibilityRole {
    /// Parses a `role` value (`"button"`, `"imagebutton"`, ...).
    pub fn from_name(name: &str) -> Option<AccessibilityRole> {
        Some(match name {
            "none" => AccessibilityRole::None,
            "button" => AccessibilityRole::Button,
            "link" => AccessibilityRole::Link,
            "search" => AccessibilityRole::Search,
            "image" => AccessibilityRole::Image,
            "imagebutton" => AccessibilityRole::ImageButton,
            "keyboardkey" => AccessibilityRole::KeyboardKey,
            "text" => AccessibilityRole::Text,
            "adjustable" => AccessibilityRole::Adjustable,
            "header" => AccessibilityRole::Header,
            "summary" => AccessibilityRole::Summary,
            "alert" => AccessibilityRole::Alert,
            "checkbox" => AccessibilityRole::Checkbox,
            "combobox" => AccessibilityRole::Combobox,
            "menu" => AccessibilityRole::Menu,
            "menubar" => AccessibilityRole::Menubar,
            "menuitem" => AccessibilityRole::Menuitem,
            "progressbar" => AccessibilityRole::Progressbar,
            "radio" => AccessibilityRole::Radio,
            "radiogroup" => AccessibilityRole::Radiogroup,
            "scrollbar" => AccessibilityRole::Scrollbar,
            "spinbutton" => AccessibilityRole::Spinbutton,
            "switch" => AccessibilityRole::Switch,
            "tab" => AccessibilityRole::Tab,
            "tablist" => AccessibilityRole::Tablist,
            "timer" => AccessibilityRole::Timer,
            "toolbar" => AccessibilityRole::Toolbar,
            "list" => AccessibilityRole::List,
            "grid" => AccessibilityRole::Grid,
            "table" => AccessibilityRole::Table,
            "row" => AccessibilityRole::Row,
            "cell" => AccessibilityRole::Cell,
            "columnheader" => AccessibilityRole::ColumnHeader,
            "rowgroup" => AccessibilityRole::RowGroup,
            "listbox" => AccessibilityRole::Listbox,
            "option" => AccessibilityRole::Option,
            "group" => AccessibilityRole::Group,
            "region" => AccessibilityRole::Region,
            "navigation" => AccessibilityRole::Navigation,
            "tree" => AccessibilityRole::Tree,
            "treeitem" => AccessibilityRole::TreeItem,
            "menuitemcheckbox" => AccessibilityRole::MenuItemCheckbox,
            "menuitemradio" => AccessibilityRole::MenuItemRadio,
            _ => return None,
        })
    }

    /// The protocol name.
    pub fn name(self) -> &'static str {
        match self {
            AccessibilityRole::None => "none",
            AccessibilityRole::Button => "button",
            AccessibilityRole::Link => "link",
            AccessibilityRole::Search => "search",
            AccessibilityRole::Image => "image",
            AccessibilityRole::ImageButton => "imagebutton",
            AccessibilityRole::KeyboardKey => "keyboardkey",
            AccessibilityRole::Text => "text",
            AccessibilityRole::Adjustable => "adjustable",
            AccessibilityRole::Header => "header",
            AccessibilityRole::Summary => "summary",
            AccessibilityRole::Alert => "alert",
            AccessibilityRole::Checkbox => "checkbox",
            AccessibilityRole::Combobox => "combobox",
            AccessibilityRole::Menu => "menu",
            AccessibilityRole::Menubar => "menubar",
            AccessibilityRole::Menuitem => "menuitem",
            AccessibilityRole::Progressbar => "progressbar",
            AccessibilityRole::Radio => "radio",
            AccessibilityRole::Radiogroup => "radiogroup",
            AccessibilityRole::Scrollbar => "scrollbar",
            AccessibilityRole::Spinbutton => "spinbutton",
            AccessibilityRole::Switch => "switch",
            AccessibilityRole::Tab => "tab",
            AccessibilityRole::Tablist => "tablist",
            AccessibilityRole::Timer => "timer",
            AccessibilityRole::Toolbar => "toolbar",
            AccessibilityRole::List => "list",
            AccessibilityRole::Grid => "grid",
            AccessibilityRole::Table => "table",
            AccessibilityRole::Row => "row",
            AccessibilityRole::Cell => "cell",
            AccessibilityRole::ColumnHeader => "columnheader",
            AccessibilityRole::RowGroup => "rowgroup",
            AccessibilityRole::Listbox => "listbox",
            AccessibilityRole::Option => "option",
            AccessibilityRole::Group => "group",
            AccessibilityRole::Region => "region",
            AccessibilityRole::Navigation => "navigation",
            AccessibilityRole::Tree => "tree",
            AccessibilityRole::TreeItem => "treeitem",
            AccessibilityRole::MenuItemCheckbox => "menuitemcheckbox",
            AccessibilityRole::MenuItemRadio => "menuitemradio",
        }
    }
}

/// Whether a checkable element is checked (`accessibilityState.checked`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checked {
    /// Not checked.
    False,
    /// Checked.
    True,
    /// Partially checked.
    Mixed,
}

/// Dynamic state of an element (`accessibilityState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AccessibilityState {
    /// Disabled. Blocks presses on the view and its subtree.
    pub disabled: bool,
    /// Selected.
    pub selected: bool,
    /// Checked state of checkable elements.
    pub checked: Option<Checked>,
    /// Expanded or collapsed, for expandable elements.
    pub expanded: Option<bool>,
}

/// The value of a range or textual element (`accessibilityValue`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AccessibilityValue {
    /// Minimum of the range.
    pub min: Option<f32>,
    /// Maximum of the range.
    pub max: Option<f32>,
    /// Current value in the range.
    pub now: Option<f32>,
    /// A textual description of the value; wins over the range.
    pub text: Option<Arc<str>>,
}

/// Accessibility of one view. v2 names are explicit: nothing is read from
/// the children.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Accessibility {
    /// `accessibilityLabel`: what screen readers read.
    pub label: Option<Arc<str>>,
    /// `accessibilityHint`: what activating the element does.
    pub hint: Option<Arc<str>>,
    /// `role`.
    pub role: AccessibilityRole,
    /// `accessibilityState`.
    pub state: AccessibilityState,
    /// `accessibilityValue`.
    pub value: Option<AccessibilityValue>,
    /// `accessibilityElementsHidden`: hide the view and its subtree.
    pub hidden: bool,
}

impl Accessibility {
    /// True if nothing is set.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The view is an accessibility element: it has something to say.
    pub fn is_element(&self) -> bool {
        !self.hidden && (self.label.is_some() || self.role != AccessibilityRole::None)
    }

    /// Presses from this view do not happen and do not reach an ancestor.
    pub fn blocks_press(&self) -> bool {
        self.state.disabled
    }
}

/// One accessibility property, as set from JS.
#[derive(Debug, Clone, PartialEq)]
pub enum AccessibilityProp {
    /// `accessibilityLabel`.
    Label(Option<Arc<str>>),
    /// `accessibilityHint`.
    Hint(Option<Arc<str>>),
    /// `role`.
    Role(AccessibilityRole),
    /// `accessibilityState`.
    State(AccessibilityState),
    /// `accessibilityValue`.
    Value(Option<AccessibilityValue>),
    /// `accessibilityElementsHidden`.
    Hidden(bool),
}

/// What a list cell holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellSlot {
    /// A recycled item cell in this slot.
    Item(u32),
    /// `header`.
    Header,
    /// `footer`.
    Footer,
    /// `empty` (shown while there are no items).
    Empty,
}

/// A non-style property of a host node.
#[derive(Debug, Clone, PartialEq)]
pub enum Prop {
    /// `src`: a URL or an imported asset.
    Source(Option<ImageSource>),
    /// `horizontal`: the scroll view scrolls along x instead of y.
    Horizontal(bool),
    /// Number of items in a list.
    Count(u32),
    /// `estimatedItemSize`: the main-axis size of items not measured yet.
    EstimatedItemSize(Points),
    /// The recycling slot (or role) of a list cell.
    Slot(CellSlot),
    /// The items changed: `removed` at `at` became `inserted` (set before
    /// the new `Count`).
    Splice {
        /// First replaced item.
        at: u32,
        /// Items removed.
        removed: u32,
        /// Items inserted.
        inserted: u32,
    },
    /// `numColumns`: items per row.
    Columns(u32),
    /// The item index a cell now renders, set by the runtime after a bind.
    CellIndex(u32),
    /// How close to the end `onLoadMore` fires, in viewports.
    LoadMoreThreshold(f32),
    /// `keyboardShouldPersistTaps`: `never` (default), `always` or `handled`.
    KeyboardTaps(KeyboardTaps),
    /// `keyboardDismissMode`: `none` (default), `on-drag` or `interactive`.
    KeyboardDismiss(KeyboardDismiss),
    /// `value`: the text of an input, controlled by JS.
    Value(String),
    /// `placeholder`: shown while an input is empty.
    Placeholder(String),
    /// `secureTextEntry`: the text is hidden (passwords).
    Secure(bool),
    /// `keyboardType`.
    Keyboard(KeyboardType),
    /// `disabled`: the view takes no presses, and an input ignores edits.
    Disabled(bool),
    /// `multiline`: Return inserts a line break, and the text wraps.
    Multiline(bool),
    /// `selectedRange`: the controlled selection of an input, in UTF-16
    /// units. `end` is the caret when it equals `start`.
    Selection {
        /// Start of the range.
        start: u32,
        /// End of the range.
        end: u32,
    },
    /// An accessibility property (`role`, label, hint, state, value, hidden).
    Accessibility(AccessibilityProp),
    /// `delayLongPress`: how long the pointer stays down before
    /// `onLongPress`, in milliseconds. `None` is the platform default (500).
    LongPressDelay(Option<f32>),
    /// `testID`: a name tests use to find the view. The platform draws
    /// nothing from it. `None` clears it.
    TestId(Option<Arc<str>>),
    /// `activeStyle` / `hoverStyle` / `focusStyle`. `None` clears it.
    /// Paint only: the platform applies it, and layout does not change.
    StateStyle {
        /// Which state.
        interaction: Interaction,
        /// The view and text paint. `None` clears the state.
        paint: Option<Box<(crate::style::ViewStyle, crate::style::TextStyle)>>,
    },
}

/// A mutation produced by the JS renderer.
#[derive(Debug)]
pub enum Op {
    /// Creates a detached element node.
    Create {
        /// New node id.
        id: NodeId,
        /// Element tag.
        tag: Tag,
    },
    /// Creates a detached raw text node. It is not a native view: inserted
    /// into a `<text>`, it is a run of that block.
    CreateText {
        /// New node id.
        id: NodeId,
        /// Text content.
        text: String,
    },
    /// Replaces the content of a raw text node.
    SetText {
        /// Text node id.
        id: NodeId,
        /// New content.
        text: String,
    },
    /// Replaces the whole style of a node.
    SetStyle {
        /// Node id.
        id: NodeId,
        /// The new style; boxed because layout styles are large.
        style: Box<Style>,
        /// The declarations `style` was built from, applied again over the
        /// node's class styles. v1 has no class table, so the shadow tree
        /// applies `style` itself.
        patch: StylePatch,
    },
    /// Sets a non-style property.
    SetProp {
        /// Node id.
        id: NodeId,
        /// The property.
        prop: Prop,
    },
    /// Enables or disables an event listener on a node.
    Listen {
        /// Node id.
        id: NodeId,
        /// Event kind.
        event: EventKind,
        /// Whether a handler is attached.
        enabled: bool,
    },
    /// Inserts (or moves) `child` into `parent` before `before`, or appends.
    Insert {
        /// Parent node id.
        parent: NodeId,
        /// Child node id.
        child: NodeId,
        /// Sibling to insert before; `None` appends.
        before: Option<NodeId>,
    },
    /// Detaches `child` from `parent`. The child stays alive until released.
    Remove {
        /// Parent node id.
        parent: NodeId,
        /// Child node id.
        child: NodeId,
    },
    /// The JS node was garbage collected: free the node and its native view.
    Release {
        /// Node id.
        id: NodeId,
    },
    /// A change to the animation graph. A platform applies it before the
    /// bindings in the same batch.
    Animation(AnimationOp),
    /// An imperative call. `Keyboard.dismiss` is [`Command::Blur`] on the root.
    Command {
        /// View the call names. Blur ignores it and resigns the focused field.
        id: NodeId,
        /// The call.
        command: Command,
    },
}

/// An imperative call on a native view.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Scroll a scroll view to a content offset.
    ScrollTo {
        /// Horizontal offset.
        x: Points,
        /// Vertical offset.
        y: Points,
        /// Animate the change.
        animated: bool,
    },
    /// Scroll a list to an item. The core resolves it to a content offset.
    ScrollToIndex {
        /// The item.
        index: u32,
        /// Animate the change.
        animated: bool,
    },
    /// Scroll a list to the end of its content.
    ScrollToEnd {
        /// Animate the change.
        animated: bool,
    },
    /// Resign the focused field and hide the software keyboard.
    Blur,
    /// Select a range of an input (UTF-16 units; equal for a caret).
    SetSelection {
        /// Start.
        start: u32,
        /// End.
        end: u32,
    },
}
