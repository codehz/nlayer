//! The animation graph: JS-declared values and interpolations, moved by
//! timing and spring drivers on the host's frame clock.
//!
//! A platform owns a [`Graph`], applies [`AnimationOp`]s from its mount
//! batch, evaluates the bindings of each view every frame and reports
//! [`Finished`] back to the engine. JS never runs a frame.
//!
//! Extracted from `nsh-core/src/animation/` and cut to the v2 graph.

mod driver;
mod graph;
mod ops;

pub use graph::Graph;
pub use ops::{
    AnimationId, AnimationOp, AnimationProp, AnimationValue, Driver, Easing, Extrapolate, Finished,
    NodeId, Output, StepPosition,
};
