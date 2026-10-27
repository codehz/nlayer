//! Platform-independent core of nlayer.
//!
//! Reactive graph, geometry, the style IR, text layout and the shadow tree
//! (views, text, images, scroll views and single-line inputs) are extracted
//! from nsh.

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
pub mod typescript;

pub use ops::{EventMask, NodeId, ROOT_ID, ResizeMode};
pub use transform::{MAX_TRANSFORMS, TransformOp};
pub mod text;
pub mod transform;

pub use color::{Hsla, Rgba};
pub use core::{ApplyError, Core};
pub use mount::{InputConfig, MountBatch, MountItem, ViewKind};
pub use ops::{
    Command, EventKind, IMAGE_SCALES, ImageScales, ImageSource, KeyboardType, Op, Prop, Tag,
};
pub use style::Color;
