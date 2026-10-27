//! Animation prop names stored on a [`crate::style::Style`].
//!
//! The animation graph is later, not in v1. These names stay so the
//! extracted style IR still type-checks; v1 does not expose them.

/// Identifier of an animation node, assigned by JS.
pub type NodeId = u32;

/// A view property that an animation node can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationProp {
    /// `opacity`, 0..1.
    Opacity,
    /// `backgroundColor`, from a color interpolation.
    BackgroundColor,
    /// `borderColor`: the color of every side.
    BorderColor,
    /// `color` of a text: the whole text takes it.
    Color,
    /// The value of entry `n` of the view's `transform` list.
    Transform(u8),
    /// Animated value slot `n` of a canvas drawing or a shader layer.
    CanvasArg(u16),
    /// `top` of a `position: sticky` view.
    StickyTop,
    /// `left` of a `position: sticky` view.
    StickyLeft,
    /// Animated prop slot `n` of a registered native component.
    ComponentSlot(u16),
}
