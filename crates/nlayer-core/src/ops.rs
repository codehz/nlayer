//! Operations the style IR names before the rest of the op protocol is extracted.
//!
//! `ResizeMode` is copied from `nsh_core::ops`. The `PropEnum` derive arrives
//! with the element macros.

use crate::{
    geometry::{Bounds, Point, Points, Size},
    style::ObjectPosition,
};

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
