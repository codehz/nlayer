//! Platform-independent core of nlayer.
//!
//! Reactive graph, geometry, the style IR, text layout, the Taffy layout
//! tree and the view shadow tree are extracted from nsh. Text, scroll and
//! input hosts follow in the same crate.

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
mod mount;
pub mod style;
mod tailwind;
mod ops;

pub use ops::{EventMask, NodeId, ROOT_ID, ResizeMode};
pub use transform::{MAX_TRANSFORMS, TransformOp};
pub mod transform;
pub mod text;

pub use core::{ApplyError, Core};
pub use mount::{MountBatch, MountItem, ViewKind};
pub use ops::{EventKind, IMAGE_SCALES, ImageScales, ImageSource, Op, Prop, Tag};
pub use color::{Hsla, Rgba};
pub use style::Color;
