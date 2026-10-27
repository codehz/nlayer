//! Operations the style IR names before the rest of the op protocol is extracted.
//!
//! `ResizeMode` is copied from `nsh_core::ops`. The `PropEnum` derive arrives
//! with the element macros.

use crate::{
    geometry::{Bounds, Point, Points, Size},
    style::ObjectPosition,
};

/// Identifier of a node, assigned by the JS renderer.
///
/// Ids are small, dense and increasing, so the core indexes them directly.
pub type NodeId = u32;

/// Id of the surface root view. It exists before any JS runs.
pub const ROOT_ID: NodeId = 1;

/// A set of event kinds: one bit per kind, room for 128.
///
/// Two words, not a `u128`: views keep it in Objective-C ivars, which take
/// no 16-byte alignment. Copied from `nsh_core::ops`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EventMask([u64; 2]);

impl std::ops::BitOr for EventMask {
    type Output = EventMask;

    #[inline]
    fn bitor(self, other: EventMask) -> EventMask {
        EventMask([self.0[0] | other.0[0], self.0[1] | other.0[1]])
    }
}

impl EventMask {
    /// The raw bit set.
    #[inline]
    pub const fn bits(self) -> u128 {
        self.0[0] as u128 | (self.0[1] as u128) << 64
    }

    /// Returns true if no event is set.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0[0] == 0 && self.0[1] == 0
    }
}

/// How an image fits its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResizeMode {
    /// Fill the box, cropping (default).
    #[default]
    Cover,
    /// Fit inside the box, letterboxing.
    Contain,
    /// Stretch to the box.
    Stretch,
    /// Center at natural size, cropped where larger than the box (`object-fit: none`).
    Center,
    /// Center at natural size, shrunk to fit the box where larger (`object-fit: scale-down`).
    /// JS name: `scale-down`.
    ScaleDown,
}

impl ResizeMode {
    /// Where content of `natural` size goes in a box of `size`.
    pub fn fit(
        self,
        natural: Size<Points>,
        size: Size<Points>,
        position: ObjectPosition,
    ) -> Bounds<Points> {
        let (nw, nh) = (natural.width.0, natural.height.0);
        if nw <= 0.0 || nh <= 0.0 || self == ResizeMode::Stretch {
            return Bounds {
                origin: Point::default(),
                size,
            };
        }
        let contain = (size.width.0 / nw).min(size.height.0 / nh);
        let scale = match self {
            ResizeMode::Cover => (size.width.0 / nw).max(size.height.0 / nh),
            ResizeMode::Contain => contain,
            ResizeMode::Center => 1.0,
            ResizeMode::ScaleDown => contain.min(1.0),
            ResizeMode::Stretch => 1.0,
        };
        let content = Size {
            width: Points(nw * scale),
            height: Points(nh * scale),
        };
        Bounds {
            origin: position.place(size, content),
            size: content,
        }
    }
}
