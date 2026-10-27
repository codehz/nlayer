//! Instructions a platform applies to native views.
//!
//! Extracted from `nsh_core::mount`, cut to views, text, images and scroll
//! views. Input items follow with that host.

use std::sync::Arc;

use crate::{
    geometry::{Bounds, Points, Size},
    ops::{Command, EventMask, ImageSource, NodeId, ResizeMode},
    style::ViewStyle,
    text::{AttributedText, TextLayout},
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
    /// Which axis a scroll view scrolls. The platform locks the other.
    ScrollConfig {
        /// Scroll view.
        id: NodeId,
        /// Scrolls horizontally. Otherwise it scrolls vertically.
        horizontal: bool,
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
    /// An imperative call (`scrollTo`).
    Command {
        /// View.
        id: NodeId,
        /// The call.
        command: Command,
    },
}

/// All mount items produced by one commit.
#[derive(Debug, Default)]
pub struct MountBatch {
    /// Items, to be applied in order within one UI frame.
    pub items: Vec<MountItem>,
}
