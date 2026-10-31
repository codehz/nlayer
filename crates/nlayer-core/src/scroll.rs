//! Scroll view rubber-band resistance.
//!
//! Extracted from `nsh-core/src/scroll.rs` (ca59368b, lines 197-225 and tests
//! at 1496-1508). The rubber band formula applies iOS-style resistance to
//! drags past content edges: `(1 - 1/(over*0.55/dim + 1)) * dim`, capping
//! overscroll at 55% of the viewport dimension.

use crate::geometry::Points;

/// The rubber band of iOS: how far content dragged `over` points past an
/// edge of a `dim`-point viewport moves.
fn rubber(over: f32, dim: f32) -> f32 {
    let dim = dim.max(1.0);
    (1.0 - 1.0 / (over * 0.55 / dim + 1.0)) * dim
}

/// Position on a drag that went to `raw`, with `[min, max]` the limits, in
/// a `dim`-point viewport: past them it rubber-bands when `bounces`.
pub fn dragged(raw: Points, min: Points, max: Points, dim: Points, bounces: bool) -> Points {
    Points(drag_position(raw.0, min.0, max.0, dim.0, bounces))
}

fn drag_position(raw: f32, min: f32, max: f32, dim: f32, bounces: bool) -> f32 {
    if raw < min {
        if bounces {
            min - rubber(min - raw, dim)
        } else {
            min
        }
    } else if raw > max {
        if bounces {
            max + rubber(raw - max, dim)
        } else {
            max
        }
    } else {
        raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rubber_band_resists() {
        assert_eq!(drag_position(50.0, 0.0, 100.0, 400.0, true), 50.0);
        let over = drag_position(-100.0, 0.0, 100.0, 400.0, true);
        assert!(over < 0.0 && over > -100.0, "{over}");
        let more = drag_position(-400.0, 0.0, 100.0, 400.0, true);
        assert!(more < over && more > -400.0 * 0.55);
        assert_eq!(drag_position(-100.0, 0.0, 100.0, 400.0, false), 0.0);
        assert_eq!(drag_position(300.0, 0.0, 100.0, 400.0, false), 100.0);
        assert!(drag_position(150.0, 0.0, 100.0, 400.0, true) > 100.0);
        let p = Points;
        assert_eq!(dragged(p(50.0), p(0.0), p(100.0), p(400.0), true), p(50.0));
        // Never more than 55% of the viewport.
        assert!(rubber(1e6, 400.0) < 400.0);
    }
}
