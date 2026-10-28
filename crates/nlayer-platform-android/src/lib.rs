//! Android host for one nlayer surface.
//!
//! The engine thread lays the app out. Each [`nlayer_core::MountBatch`] is
//! [`encode`]d into one little-endian word stream plus an object table.
//! The UI thread applies that stream.
//!
//! Extracted from `nsh-platform-android/src/encode.rs` and cut to views,
//! text, images, one-axis scroll and single-line inputs.

mod encode;

pub use encode::{Encoded, Object, encode};
