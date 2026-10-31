//! Scroll view snapping and paging behavior.
//!
//! Extracted from nsh-core (ca59368b):
//! - ScrollBehavior and snap_target: src/mount.rs lines ~770-910
//! - rate and projected: src/scroll.rs lines ~150-180
//! - Tests: src/mount.rs lines ~2900-3000, src/scroll.rs lines ~1475-1490
//!
//! Snapping targets are computed from paging/interval/offsets with alignment
//! and momentum control. The deceleration rate affects projected glide distance.

use crate::geometry::Points;

/// Snap alignment: which point of a snap page aligns with the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SnapAlignment {
    /// Item start aligns with viewport start.
    Start = 0,
    /// Item center aligns with viewport center.
    Center = 1,
    /// Item end aligns with viewport end.
    End = 2,
}

/// Scroll behavior configuration for snapping.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapBehavior {
    /// Stops at multiples of the viewport (pagingEnabled).
    pub paging: bool,
    /// Stops at multiples of this length (snapToInterval, Points::ZERO: off).
    pub snap_interval: Points,
    /// Stops at these offsets (snapToOffsets).
    pub snap_offsets: Vec<Points>,
    /// Which point of a snap page aligns with the viewport (snapToAlignment).
    pub snap_alignment: SnapAlignment,
    /// Velocity kept per millisecond after a fling (decelerationRate, 0: platform default).
    pub deceleration: f32,
    /// A fling moves one snap point at most (disableIntervalMomentum).
    pub one_step: bool,
}

impl SnapBehavior {
    fn snaps(&self) -> bool {
        self.snap_interval.0 > 0.0 || !self.snap_offsets.is_empty()
    }
}

/// The deceleration rate: velocity kept per ms.
pub fn rate(behavior: &SnapBehavior) -> f32 {
    if behavior.deceleration > 0.0 && behavior.deceleration < 1.0 {
        behavior.deceleration
    } else if behavior.snaps() || behavior.paging {
        0.99
    } else {
        0.998
    }
}

/// Where a glide at `velocity` (points per ms) from `offset` comes to rest,
/// decelerating by `rate` ([`rate`]).
pub fn projected(offset: Points, velocity: f32, rate: f32) -> Points {
    Points(offset.0 - velocity / rate.ln())
}

/// Snap target for a scroll at `offset` that would glide to `projected`
/// (both along the main axis, points) and `velocity` its speed (points
/// per millisecond), for a viewport of `viewport` and content of
/// `content` points. Pages (and `one_step`) move one snap point; other
/// snapping rounds the projected offset in the fling's direction.
/// `None` without snapping.
pub fn snap_target(
    behavior: &SnapBehavior,
    offset: Points,
    projected: Points,
    velocity: f32,
    viewport: Points,
    content: Points,
) -> Option<Points> {
    let (offset, projected, viewport) = (offset.0, projected.0, viewport.0);
    let max = (content.0 - viewport).max(0.0);
    let points: Vec<f32> = if behavior.snap_interval.0 > 0.0 {
        let interval = behavior.snap_interval.0;
        // Aligns the item at `k * interval` with the viewport.
        let shift = match behavior.snap_alignment {
            SnapAlignment::Start => 0.0,
            SnapAlignment::Center => (viewport - interval) / 2.0,
            SnapAlignment::End => viewport - interval,
        };
        let n = ((max + shift) / interval).ceil().max(0.0) as usize;
        (0..=n)
            .map(|i| (i as f32 * interval - shift).clamp(0.0, max))
            .collect()
    } else if !behavior.snap_offsets.is_empty() {
        behavior
            .snap_offsets
            .iter()
            .map(|o| o.0.clamp(0.0, max))
            .collect()
    } else if behavior.paging {
        let page = viewport.max(1.0);
        (0..=(max / page).ceil() as usize)
            .map(|i| (i as f32 * page).min(max))
            .collect()
    } else {
        return None;
    };
    // Slower releases (points per ms) settle at the nearest point.
    const SLOW: f32 = 0.3;
    let step = behavior.one_step || (behavior.paging && !behavior.snaps());
    let target = if step {
        if velocity > SLOW {
            points.iter().copied().find(|&p| p > offset + 0.5)
        } else if velocity < -SLOW {
            points.iter().rev().copied().find(|&p| p < offset - 0.5)
        } else {
            nearest(&points, offset)
        }
    } else if velocity > SLOW {
        points.iter().copied().find(|&p| p >= projected - 0.5)
    } else if velocity < -SLOW {
        points.iter().rev().copied().find(|&p| p <= projected + 0.5)
    } else {
        nearest(&points, projected)
    };
    Some(Points(target.unwrap_or(if velocity > 0.0 {
        max
    } else {
        0.0
    })))
}

fn nearest(points: &[f32], to: f32) -> Option<f32> {
    points
        .iter()
        .copied()
        .min_by(|a, b| (a - to).abs().total_cmp(&(b - to).abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_follow_behavior() {
        let mut b = SnapBehavior {
            paging: false,
            snap_interval: Points(0.0),
            snap_offsets: vec![],
            snap_alignment: SnapAlignment::Start,
            deceleration: 0.0,
            one_step: false,
        };
        assert_eq!(rate(&b), 0.998);
        b.paging = true;
        assert_eq!(rate(&b), 0.99);
        b.deceleration = 0.95;
        assert_eq!(rate(&b), 0.95);
    }

    #[test]
    fn snapping_and_paging_targets() {
        // Paging in a 400 point viewport over 1600 points of content: a
        // slow release past half a page moves on, a fling moves one page.
        let b = SnapBehavior {
            paging: true,
            snap_interval: Points(0.0),
            snap_offsets: vec![],
            snap_alignment: SnapAlignment::Start,
            deceleration: 0.0,
            one_step: false,
        };
        let r = rate(&b);
        let pt = Points;
        let t = snap_target(
            &b,
            pt(250.0),
            projected(pt(250.0), 0.0, r),
            0.0,
            pt(400.0),
            pt(1600.0),
        );
        assert_eq!(t, Some(pt(400.0)));
        let t = snap_target(
            &b,
            pt(100.0),
            projected(pt(100.0), 1.0, r),
            1.0,
            pt(400.0),
            pt(1600.0),
        );
        assert_eq!(t, Some(pt(400.0)));
        let t = snap_target(
            &b,
            pt(300.0),
            projected(pt(300.0), -1.0, r),
            -1.0,
            pt(400.0),
            pt(1600.0),
        );
        assert_eq!(t, Some(pt(0.0)));
    }

    #[test]
    fn snap_targets() {
        let px = Points;
        let paging = SnapBehavior {
            paging: true,
            snap_interval: Points(0.0),
            snap_offsets: vec![],
            snap_alignment: SnapAlignment::Start,
            deceleration: 0.0,
            one_step: false,
        };
        // Pages of 300 in 1000 points of content (the last page is short):
        // one page per fling, wherever the fling would glide.
        assert_eq!(
            snap_target(&paging, px(100.0), px(100.0), 0.0, px(300.0), px(1000.0)),
            Some(px(0.0))
        );
        assert_eq!(
            snap_target(&paging, px(100.0), px(900.0), 1.0, px(300.0), px(1000.0)),
            Some(px(300.0))
        );
        assert_eq!(
            snap_target(&paging, px(650.0), px(900.0), 1.0, px(300.0), px(1000.0)),
            Some(px(700.0))
        );
        assert_eq!(
            snap_target(&paging, px(250.0), px(0.0), -1.0, px(300.0), px(1000.0)),
            Some(px(0.0))
        );
        let interval = SnapBehavior {
            paging: false,
            snap_interval: Points(120.0),
            snap_offsets: vec![],
            snap_alignment: SnapAlignment::Center,
            deceleration: 0.0,
            one_step: false,
        };
        // Items of 120 centered in a 300 viewport: k * 120 - 90.
        assert_eq!(
            snap_target(&interval, px(200.0), px(200.0), 0.0, px(300.0), px(2000.0)),
            Some(px(150.0))
        );
        // A fast fling glides past several items, rounded forward.
        assert_eq!(
            snap_target(&interval, px(200.0), px(700.0), 2.0, px(300.0), px(2000.0)),
            Some(px(750.0))
        );
        let one = SnapBehavior {
            one_step: true,
            ..interval.clone()
        };
        assert_eq!(
            snap_target(&one, px(200.0), px(700.0), 2.0, px(300.0), px(2000.0)),
            Some(px(270.0))
        );
        let offsets = SnapBehavior {
            paging: false,
            snap_interval: Points(0.0),
            snap_offsets: vec![Points(0.0), Points(500.0), Points(900.0)],
            snap_alignment: SnapAlignment::Start,
            deceleration: 0.0,
            one_step: false,
        };
        assert_eq!(
            snap_target(&offsets, px(520.0), px(600.0), 2.0, px(300.0), px(2000.0)),
            Some(px(900.0))
        );
        assert_eq!(
            snap_target(&offsets, px(520.0), px(520.0), 0.0, px(300.0), px(2000.0)),
            Some(px(500.0))
        );
        let default = SnapBehavior {
            paging: false,
            snap_interval: Points(0.0),
            snap_offsets: vec![],
            snap_alignment: SnapAlignment::Start,
            deceleration: 0.0,
            one_step: false,
        };
        assert_eq!(
            snap_target(&default, px(10.0), px(10.0), 1.0, px(300.0), px(2000.0)),
            None
        );
    }
}
