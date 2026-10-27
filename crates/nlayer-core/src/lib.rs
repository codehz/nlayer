//! Platform-independent core of nlayer.
//!
//! Reactive graph, geometry, the style IR, text layout and the Taffy
//! layout tree are extracted from nsh. The shadow tree and mount batches
//! follow in the same crate.

pub mod animation;
pub mod color;
mod core;
mod css;
mod css_named;
pub mod effects;
pub mod geometry;
mod grid;
mod props;
pub mod reactive;
pub mod style;
mod tailwind;
mod ops;

pub use ops::{EventMask, NodeId, ROOT_ID, ResizeMode};
pub use transform::{MAX_TRANSFORMS, TransformOp};
pub mod transform;
pub mod text;

pub use color::{Hsla, Rgba};
pub use style::Color;
