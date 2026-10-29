//! Height of the software keyboard over a view.
//!
//! The overlap is copied from `nsh-platform-ios` `keyboard_changed`. The
//! animation duration and curve are not: v1 applies the end height at once.

/// Height, in points, of a keyboard over a view of `view_width` by
/// `view_height`.
///
/// `frame` is the keyboard's end frame in the view's coordinates
/// `(x, y, width, height)`. A hide, a missing frame, or a keyboard that does
/// not cross the view horizontally (a floating keyboard off to the side, or
/// one on another screen) is zero. The overlap is clamped to the view.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(crate) fn overlap(
    view_width: f64,
    view_height: f64,
    frame: Option<(f64, f64, f64, f64)>,
    hiding: bool,
) -> f32 {
    let height = match frame {
        Some((x, y, width, _)) if !hiding => {
            let overlaps = x < view_width && x + width > 0.0;
            if overlaps {
                (view_height - y).clamp(0.0, view_height)
            } else {
                0.0
            }
        }
        _ => 0.0,
    };
    height as f32
}

#[cfg(test)]
mod tests {
    use super::overlap;

    #[test]
    fn a_docked_keyboard_is_the_overlap() {
        assert_eq!(
            overlap(390.0, 844.0, Some((0.0, 544.0, 390.0, 300.0)), false),
            300.0
        );
    }

    #[test]
    fn a_keyboard_past_the_view_or_beside_it_is_zero() {
        assert_eq!(
            overlap(390.0, 844.0, Some((0.0, 844.0, 390.0, 300.0)), false),
            0.0
        );
        assert_eq!(
            overlap(390.0, 844.0, Some((400.0, 400.0, 200.0, 200.0)), false),
            0.0
        );
        assert_eq!(
            overlap(390.0, 844.0, Some((-200.0, 400.0, 200.0, 200.0)), false),
            0.0
        );
    }

    #[test]
    fn a_covering_keyboard_is_clamped_to_the_view() {
        assert_eq!(
            overlap(390.0, 844.0, Some((0.0, -20.0, 390.0, 900.0)), false),
            844.0
        );
    }

    #[test]
    fn hiding_or_a_missing_frame_is_zero() {
        assert_eq!(
            overlap(390.0, 844.0, Some((0.0, 544.0, 390.0, 300.0)), true),
            0.0
        );
        assert_eq!(overlap(390.0, 844.0, None, false), 0.0);
    }
}
