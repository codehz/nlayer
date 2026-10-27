//! Platform-independent core of nlayer.
//!
//! Reactive graph, geometry, the style IR, text layout and the shadow tree
//! (views, text and images) are extracted from nsh. Scroll and input hosts
//! follow in the same crate.

pub mod animation;
pub mod color;
mod core;
mod css;
mod css_named;
pub mod effects;
pub mod geometry;
mod grid;
mod mount;
mod ops;
mod props;
pub mod reactive;
pub mod style;
mod tailwind;

pub use ops::{EventMask, NodeId, ROOT_ID, ResizeMode};
pub use transform::{MAX_TRANSFORMS, TransformOp};
pub mod text;
pub mod transform;

pub use color::{Hsla, Rgba};
pub use core::{ApplyError, Core};
pub use mount::{MountBatch, MountItem, ViewKind};
pub use ops::{EventKind, IMAGE_SCALES, ImageScales, ImageSource, Op, Prop, Tag};
pub use style::Color;
