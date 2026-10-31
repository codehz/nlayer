//! Instructions a platform applies to native views.
//!
//! Extracted from `nsh_core::mount`, cut to views, text, images, scroll
//! views, text, images, scroll views and inputs (one line or several).

use std::sync::Arc;

use crate::{
    animation::{AnimationOp, AnimationProp, NodeId as AnimationNode},
    geometry::{Bounds, Points, Size},
    ops::{
        Command, EventMask, ImageSource, Interaction, KeyboardDismiss, KeyboardTaps, KeyboardType,
        NodeId, ResizeMode,
    },
    style::{TextAlign, ViewStyle},
    text::{AttributedText, ResolvedTextStyle, TextLayout},
};

/// Kind of native view to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    /// A plain container view.
    View,
    /// A text block.
    Text,
    /// An image.
    Image,
    /// A scroll view. It scrolls on one axis ([`MountItem::ScrollConfig`]).
    Scroll,
    /// A text field, one line or several ([`MountItem::Input`]).
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
    /// [`ScreenConfig`]).
    HeaderItem,
}

/// The header heights a platform without a native header gives the core
/// (the web): nlayer draws no header there. A page's `header` slot
/// is placed at its top instead, as wide as the page and as tall as it lays
/// out, and the content starts below it; the native header's options
/// (`title`, `headerLeft`, `headerStyle`, ...) are ignored.
pub const NO_NATIVE_HEADER: (Points, Points) = (Points::ZERO, Points::ZERO);

/// Options of a screen: its header and transitions.
///
/// The header options are the native header's (iOS, Android); a platform
/// without one ([`NO_NATIVE_HEADER`]) reads only `header_view`,
/// `header_height`, `prevent_back`, `gesture_enabled` and `animation`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenConfig {
    /// Title in the header.
    pub title: Option<Arc<str>>,
    /// Show the native header.
    pub header_visible: bool,
    /// The content extends under the header.
    pub header_transparent: bool,
    /// Color of the back button and bar buttons.
    pub tint_color: Option<crate::Color>,
    /// Header background.
    pub background_color: Option<crate::Color>,
    /// Title color.
    pub title_color: Option<crate::Color>,
    /// Show the back button.
    pub back_visible: bool,
    /// The interactive back gesture (iOS swipe back, Android predictive back
    /// following the finger) is on; the back button pops either way
    /// (`popOnSwipe`).
    pub gesture_enabled: bool,
    /// The user cannot leave the screen (`popOnBack` false: no swipe, the
    /// back buttons do not pop): answered by the platform when the system
    /// asks, without JS. Every back press sends `onBackPress` either way.
    pub prevent_back: bool,
    /// Push / pop transition.
    pub animation: crate::ops::ScreenAnimation,
    /// Leading header item view.
    pub left: Option<NodeId>,
    /// Trailing header item view.
    pub right: Option<NodeId>,
    /// Custom title view.
    pub title_view: Option<NodeId>,
    /// The app's header (`header`): on a platform without a native header
    /// ([`NO_NATIVE_HEADER`]), a view at the top of the screen whose frame
    /// the core sets (as wide as the screen, as tall as it lays out),
    /// above the content.
    pub header_view: Option<NodeId>,
    /// Height of the header area at the top of the screen, as the core laid
    /// the content out below it: the native header's (status bar included),
    /// or the `header` view's without a native header.
    pub header_height: Points,
}

impl Default for ScreenConfig {
    fn default() -> Self {
        Self {
            title: None,
            header_visible: true,
            header_transparent: false,
            tint_color: None,
            background_color: None,
            title_color: None,
            back_visible: true,
            gesture_enabled: true,
            prevent_back: false,
            animation: crate::ops::ScreenAnimation::Default,
            left: None,
            right: None,
            title_view: None,
            header_view: None,
            header_height: Points::ZERO,
        }
    }
}

/// Options of a modal (`Dialog`).
///
/// The modal itself has no look: on the web it is a transparent layer over
/// the whole window (no card, scrim, shadow or background; `presentation` is
/// ignored), and what shows is the app's popup and backdrop, which the
/// runtime renders inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModalConfig {
    /// How it appears.
    pub animation: crate::ops::ModalAnimation,
    /// How it covers the screen (iOS).
    pub presentation: crate::ops::ModalPresentation,
    /// Escape and the Android back button close it natively
    /// (`closeOnEscape`); either way they report `onCancel`.
    pub close_on_escape: bool,
    /// A press on the backdrop closes it (`closeOnBackdropPress`): the
    /// runtime handles presses on the app's backdrop part; a platform
    /// reads it only for a backdrop of its own (a native sheet's).
    pub close_on_backdrop_press: bool,
    /// Swiping a sheet down closes it natively (`closeOnSwipe`).
    pub close_on_swipe: bool,
}

impl Default for ModalConfig {
    fn default() -> Self {
        Self {
            animation: crate::ops::ModalAnimation::default(),
            presentation: crate::ops::ModalPresentation::default(),
            close_on_escape: true,
            close_on_backdrop_press: true,
            close_on_swipe: true,
        }
    }
}

/// One instruction for the platform backend.
///
/// Items must be applied in order: [`MountItem::Insert`] refers to views
/// created by the preceding items.
#[derive(Debug, Clone)]
pub enum MountItem {
    /// Create a native view. It starts detached and invisible.
    Create {
        /// Node id, used to address the view in later items.
        id: NodeId,
        /// View kind.
        kind: ViewKind,
    },
    /// Insert (or move) `child` into `parent`, below sibling `before` (or
    /// on top when `None`). Siblings rather than indices, so platform-owned
    /// subviews cannot shift positions.
    Insert {
        /// Parent view.
        parent: NodeId,
        /// Child view.
        child: NodeId,
        /// The app sibling to insert below.
        before: Option<NodeId>,
    },
    /// Remove a view from its superview, keeping it alive.
    Remove {
        /// View to detach.
        id: NodeId,
    },
    /// Destroy a view.
    Delete {
        /// View to destroy.
        id: NodeId,
    },
    /// Set the frame of a view, relative to its superview.
    Frame {
        /// View.
        id: NodeId,
        /// New frame.
        frame: Bounds<Points>,
    },
    /// Set the visual style of a view.
    ViewStyle {
        /// View.
        id: NodeId,
        /// New style.
        style: Box<ViewStyle>,
    },
    /// Paint for a pointer state (`activeStyle`, `hoverStyle`, `focusStyle`).
    /// A default view style and text style clears it. Layout does not use
    /// this item: the platform paints it over the view's own style.
    StateStyle {
        /// View.
        id: NodeId,
        /// Which state.
        interaction: Interaction,
        /// Visual style. The default clears the view paint.
        view: Box<ViewStyle>,
        /// Text paint. The default clears it.
        text: crate::style::TextStyle,
    },
    /// Set the content of a text view.
    Text {
        /// Text view.
        id: NodeId,
        /// Content, shared and never copied.
        text: Arc<AttributedText>,
        /// The final layout, ready to draw.
        layout: TextLayout,
        /// Where `layout` goes inside the view (inside padding and border).
        content: Bounds<Points>,
    },
    /// Set which events the view must report.
    Events {
        /// View.
        id: NodeId,
        /// Event set.
        events: EventMask,
    },
    /// How long a press must stay down before `onLongPress`, in
    /// milliseconds. `None` is the platform default.
    LongPressDelay {
        /// View.
        id: NodeId,
        /// The delay; `None` clears it.
        delay: Option<f32>,
    },
    /// Set the source of an image view.
    Image {
        /// Image view.
        id: NodeId,
        /// Source; `None` clears the image. A bundled asset is the variant
        /// for the screen's pixel ratio.
        source: Option<ImageSource>,
        /// How the image fits its box (`objectFit`; cover when unset).
        resize_mode: ResizeMode,
    },
    /// Axis and keyboard policy of a scroll view.
    ScrollConfig {
        /// Scroll view.
        id: NodeId,
        /// Scrolls horizontally. Otherwise it scrolls vertically.
        horizontal: bool,
        /// `keyboardShouldPersistTaps`.
        taps: KeyboardTaps,
        /// `keyboardDismissMode`.
        dismiss: KeyboardDismiss,
    },
    /// Set the scrollable content size.
    ScrollContent {
        /// Scroll view.
        id: NodeId,
        /// Content size. The viewport is the view's frame; the platform
        /// scrolls where this is larger.
        size: Size<Points>,
    },
    /// Shift the scroll position, keeping visible content in place when a
    /// right-to-left horizontal scroll view's content width changes.
    ScrollBy {
        /// Scroll view.
        id: NodeId,
        /// Horizontal shift.
        dx: Points,
        /// Vertical shift.
        dy: Points,
    },
    /// Configure a text field. The text itself is
    /// [`MountItem::InputValue`].
    Input {
        /// Input view.
        id: NodeId,
        /// The configuration; boxed because it carries the text style.
        config: Box<InputConfig>,
    },
    /// Set the text of an input (from JS). `count` is how many edits the
    /// platform has reported: a value based on an older edit is stale.
    InputValue {
        /// Input view.
        id: NodeId,
        /// The text.
        text: String,
        /// Edits reported so far.
        count: u32,
    },
    /// Where the text of an input goes (inside padding and border).
    InputContent {
        /// Input view.
        id: NodeId,
        /// Content box relative to the view.
        content: Bounds<Points>,
    },
    /// Accessibility of a view: its label, hint, role, state, value and
    /// whether its subtree is hidden from assistive technologies. The
    /// platform reports these and draws nothing from them.
    Accessibility {
        /// View.
        id: NodeId,
        /// The properties; boxed because they are large.
        accessibility: Box<crate::Accessibility>,
    },
    /// `testID`. The platform draws nothing from it. `None` clears it.
    /// Nested `<Text>` spans do not get this item: their ids are
    /// [`crate::text::AttributedText::test_ids`].
    TestId {
        /// View.
        id: NodeId,
        /// The name, or `None` when it was cleared.
        test_id: Option<Arc<str>>,
    },
    /// A change to the animation graph. It precedes the bindings that use
    /// the nodes it makes.
    Animation(AnimationOp),
    /// Bind visual properties to animation nodes. Replaces the previous
    /// set; an empty set clears it.
    AnimationBindings {
        /// View.
        id: NodeId,
        /// Property bindings.
        bindings: Vec<(AnimationProp, AnimationNode)>,
    },
    /// An imperative call (`scrollTo`).
    Command {
        /// View.
        id: NodeId,
        /// The call.
        command: Command,
    },
    /// Configure a screen (sent before the screen is inserted into its
    /// stack, and on every change).
    ScreenConfig {
        /// Screen.
        id: NodeId,
        /// The options.
        config: Box<ScreenConfig>,
    },
    /// Configure a modal (sent before it is presented, and on changes).
    ModalConfig {
        /// Modal root view.
        id: NodeId,
        /// The options.
        config: ModalConfig,
    },
}

/// Everything about an input except its text.
///
/// Copied from `nsh_core::mount::InputConfig` and cut to these fields: no
/// accessory, placeholder color or submit behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct InputConfig {
    /// Font and color of the text.
    pub style: ResolvedTextStyle,
    /// Alignment of the text.
    pub align: TextAlign,
    /// Shown while the text is empty.
    pub placeholder: String,
    /// Hide the characters (passwords).
    pub secure: bool,
    /// Keyboard layout.
    pub keyboard: KeyboardType,
    /// The user cannot edit the text.
    pub disabled: bool,
    /// The text runs right to left.
    pub rtl: bool,
    /// Several lines: Return inserts a line break and the text wraps.
    pub multiline: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            style: ResolvedTextStyle::default(),
            align: TextAlign::Auto,
            placeholder: String::new(),
            secure: false,
            keyboard: KeyboardType::Text,
            disabled: false,
            rtl: false,
            multiline: false,
        }
    }
}

/// All mount items produced by one commit.
#[derive(Debug, Default)]
pub struct MountBatch {
    /// Items, to be applied in order within one UI frame.
    pub items: Vec<MountItem>,
}
