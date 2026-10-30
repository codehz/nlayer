//! What JS asks the animation graph, and what it reports back.
//!
//! The v2 graph drives view properties only: [opacity](AnimationProp::Opacity),
//! [background](AnimationProp::BackgroundColor),
//! [border](AnimationProp::BorderColor) and the steps of a static
//! [`transform`](AnimationProp::Transform). Values are numbers or colors;
//! timing and spring move a number, interpolation derives both.
//!
//! Extracted from `nsh-core/src/animation/ops.rs` and cut to the v2 graph:
//! no composites, decay, math nodes, offsets, canvas or native component
//! slots.

/// Identifier of an animation node, assigned by JS.
pub type NodeId = u32;

/// Identifier of one started animation, assigned by JS; reported back when
/// the animation finishes (or is interrupted).
pub type AnimationId = u32;

/// A view property an animation node can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationProp {
    /// `opacity`, 0..1.
    Opacity,
    /// `backgroundColor`, from a color interpolation.
    BackgroundColor,
    /// `borderColor`: the color of every side.
    BorderColor,
    /// The value of entry `n` of the view's static `transform` list
    /// (translations in points, rotations in degrees, scales as factors).
    Transform(u8),
}

impl AnimationProp {
    /// Parses a style key. Transform entries are addressed by position.
    pub fn from_name(name: &str) -> Option<AnimationProp> {
        Some(match name {
            "opacity" => AnimationProp::Opacity,
            "backgroundColor" => AnimationProp::BackgroundColor,
            "borderColor" => AnimationProp::BorderColor,
            _ => return None,
        })
    }
}

/// Where a [`Easing::Steps`] curve jumps (CSS's `<step-position>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepPosition {
    /// The first jump happens at the start (`jump-start`, `start`).
    JumpStart,
    /// The last jump happens at the end (`jump-end`, `end`; the default).
    #[default]
    JumpEnd,
    /// No jump at either end (`jump-none`).
    JumpNone,
    /// Jumps at both ends (`jump-both`).
    JumpBoth,
}

impl StepPosition {
    /// Parses a CSS `<step-position>`.
    pub fn parse(name: &str) -> Option<StepPosition> {
        Some(match name {
            "jump-start" | "start" => StepPosition::JumpStart,
            "jump-end" | "end" => StepPosition::JumpEnd,
            "jump-none" => StepPosition::JumpNone,
            "jump-both" => StepPosition::JumpBoth,
            _ => return None,
        })
    }
}

/// Easing curve of a timing animation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    /// Constant speed.
    Linear,
    /// CSS `ease`.
    Ease,
    /// CSS `ease-in`.
    EaseIn,
    /// CSS `ease-out`.
    EaseOut,
    /// CSS `ease-in-out`.
    EaseInOut,
    /// A cubic Bézier `(x1, y1, x2, y2)`, like CSS `cubic-bezier()`.
    Bezier(f32, f32, f32, f32),
    /// CSS `steps(count, position)`.
    Steps(u32, StepPosition),
}

impl Easing {
    /// CSS `ease`.
    pub const EASE: Easing = Easing::Bezier(0.25, 0.1, 0.25, 1.0);
}

/// How an animation moves a value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Driver {
    /// Fixed duration along an easing curve.
    Timing {
        /// Target value.
        to: f32,
        /// Duration in milliseconds.
        duration_ms: f32,
        /// Curve.
        easing: Easing,
        /// Delay before starting, in milliseconds.
        delay_ms: f32,
    },
    /// Damped spring.
    Spring {
        /// Target value.
        to: f32,
        /// Spring stiffness `k`.
        stiffness: f32,
        /// Damping `c`.
        damping: f32,
        /// Mass `m`.
        mass: f32,
        /// Initial velocity (units per second); `None` keeps the current one.
        velocity: Option<f32>,
    },
}

/// Output range of an interpolation.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Numbers.
    Numbers(Vec<f32>),
    /// Colors packed as `0xRRGGBBAA`.
    Colors(Vec<u32>),
}

/// What an interpolation does outside its input range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extrapolate {
    /// Continue the first / last segment.
    Extend,
    /// Stick to the first / last output.
    Clamp,
}

/// A change to the animation graph, sent by JS.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimationOp {
    /// A settable, animatable value.
    CreateValue {
        /// New node.
        node: NodeId,
        /// Initial value.
        value: f32,
    },
    /// Maps `input` through piecewise-linear ranges.
    CreateInterpolate {
        /// New node.
        node: NodeId,
        /// Source node.
        input: NodeId,
        /// Increasing input breakpoints.
        input_range: Vec<f32>,
        /// Outputs, same length as `input_range`.
        output: Output,
        /// Behaviour below the input range.
        extrapolate_left: Extrapolate,
        /// Behaviour above the input range.
        extrapolate_right: Extrapolate,
        /// Curve within each segment.
        easing: Easing,
    },
    /// Sets a value immediately, stopping its animation.
    Set {
        /// Value node.
        node: NodeId,
        /// New value.
        value: f32,
    },
    /// Starts an animation on a value, replacing any running one.
    Start {
        /// Value node.
        node: NodeId,
        /// Reported back on completion.
        animation: AnimationId,
        /// How to move.
        driver: Driver,
    },
    /// Stops the animation of a value where it is.
    Stop {
        /// Value node.
        node: NodeId,
    },
    /// Frees a node.
    Release {
        /// Node.
        node: NodeId,
    },
    /// Reports the current value of any node as a [`Finished`] of
    /// `request`, so JS can read it back.
    Read {
        /// The node.
        node: NodeId,
        /// Reported back with the value.
        request: AnimationId,
    },
}

/// The evaluated value of a node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationValue {
    /// A number.
    Number(f32),
    /// A color, `0xRRGGBBAA`.
    Color(u32),
}

impl AnimationValue {
    /// The number, or 0 for colors.
    pub fn number(self) -> f32 {
        match self {
            AnimationValue::Number(n) => n,
            AnimationValue::Color(_) => 0.0,
        }
    }
}

/// A finished (or interrupted) animation, to report back to JS; also the
/// answer to an [`AnimationOp::Read`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Finished {
    /// The animation (or the read request).
    pub animation: AnimationId,
    /// False if it was stopped or replaced before reaching its end.
    pub finished: bool,
    /// The node's value, for a read.
    pub value: Option<f32>,
}

impl Finished {
    /// The end of `animation`.
    pub fn new(animation: AnimationId, finished: bool) -> Self {
        Finished {
            animation,
            finished,
            value: None,
        }
    }
}
