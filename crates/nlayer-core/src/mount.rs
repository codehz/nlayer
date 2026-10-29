//! Instructions a platform applies to native views.
//!
//! Extracted from `nsh_core::mount`, cut to views, text, images, scroll
//! views and single-line inputs.

use std::sync::Arc;

use crate::{
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
    /// A single-line text field ([`MountItem::Input`]).
    Input,
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
    /// Configure a single-line text field. The text itself is
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
    /// An imperative call (`scrollTo`).
    Command {
        /// View.
        id: NodeId,
        /// The call.
        command: Command,
    },
}

/// Everything about a single-line input except its text.
///
/// Copied from `nsh_core::mount::InputConfig` and cut to the v1 fields:
/// one line, no accessory, no submit behavior.
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
        }
    }
}

/// All mount items produced by one commit.
#[derive(Debug, Default)]
pub struct MountBatch {
    /// Items, to be applied in order within one UI frame.
    pub items: Vec<MountItem>,
}
