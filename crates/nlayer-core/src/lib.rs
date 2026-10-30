//! Platform-independent core of nlayer.
//!
//! Reactive graph, geometry, the style IR, text layout and the shadow tree
//! (views, text, images, scroll views and inputs) are extracted
//! from nsh.

pub mod animation;
pub mod color;
mod core;
mod css;
mod css_named;
pub mod effects;
pub mod geometry;
mod grid;
mod keyboard;
mod mount;
mod ops;
pub mod platform;
pub mod reactive;
pub mod resource;
pub mod storage;
pub mod style;
mod tailwind;
pub mod typescript;

pub use ops::{EventMask, NodeId, ROOT_ID, ResizeMode};
pub use transform::{MAX_TRANSFORMS, TransformOp};
pub mod text;
pub mod transform;

pub use color::{Hsla, Rgba};
pub use core::{ApplyError, Core};
pub use keyboard::{drag_dismisses, press_allowed, return_resigns, tap_dismisses};
pub use mount::{InputConfig, MountBatch, MountItem, ViewKind};
pub use ops::{
    Accessibility, AccessibilityProp, AccessibilityRole, AccessibilityState, AccessibilityValue,
    Checked, Command, EventKind, IMAGE_SCALES, ImageScales, ImageSource, Interaction,
    KeyboardDismiss, KeyboardTaps, KeyboardType, Op, Prop, Tag,
};
pub use platform::ColorScheme;
pub use style::Color;
pub use tailwind::{class_tokens, unknown_classes_warning};
