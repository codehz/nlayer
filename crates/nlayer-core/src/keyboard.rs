//! When a tap or a drag hides the software keyboard.
//!
//! The inset is `env(keyboard-inset-height)`. A height of 0 (the keyboard is
//! hidden, a hardware keyboard is attached, or a test pin holds it at 0)
//! leaves the tap alone.

use crate::geometry::Points;
use crate::ops::{KeyboardDismiss, KeyboardTaps, KeyboardType};

/// Whether a tap hides the software keyboard.
///
/// `taps` is the nearest scroll's `keyboardShouldPersistTaps`. `None` means
/// the tap is not inside a scroll. `on_input` is an input on the walk from
/// the hit view up to that scroll; a disabled input counts. `handled` is an
/// `onPress` (including a pressable span) or an input on that walk.
pub fn tap_dismisses(
    inset: Points,
    taps: Option<KeyboardTaps>,
    on_input: bool,
    handled: bool,
) -> bool {
    if inset.0 > 0.0 {
        match taps {
            None | Some(KeyboardTaps::Always) => false,
            Some(KeyboardTaps::Never) => !on_input,
            Some(KeyboardTaps::Handled) => !handled && !on_input,
        }
    } else {
        false
    }
}

/// Whether a press should start. `never` swallows it unless the touch is
/// on an input, so `onPress` and `activeStyle` do not run.
pub fn press_allowed(inset: Points, taps: Option<KeyboardTaps>, on_input: bool) -> bool {
    if inset.0 > 0.0 {
        !matches!(taps, Some(KeyboardTaps::Never)) || on_input
    } else {
        true
    }
}

/// Whether a drag along this scroll's own axis, past the platform slop,
/// hides the keyboard. `on-drag` and `interactive` are the same: the
/// keyboard does not follow the finger. A wheel does not drag.
pub fn drag_dismisses(inset: Points, mode: KeyboardDismiss) -> bool {
    inset.0 > 0.0 && mode != KeyboardDismiss::None
}

/// text and email have a Return key that resigns. number and phone do not.
pub fn return_resigns(keyboard: KeyboardType) -> bool {
    matches!(keyboard, KeyboardType::Text | KeyboardType::Email)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidden_keyboard_lets_the_tap_through() {
        for taps in [
            None,
            Some(KeyboardTaps::Never),
            Some(KeyboardTaps::Always),
            Some(KeyboardTaps::Handled),
        ] {
            assert!(!tap_dismisses(Points(0.0), taps, false, false));
            assert!(press_allowed(Points(0.0), taps, false));
            assert!(!drag_dismisses(Points(0.0), KeyboardDismiss::OnDrag));
        }
    }

    #[test]
    fn the_nearest_scroll_mode_decides_the_tap() {
        assert!(!tap_dismisses(Points(300.0), None, false, false));
        assert!(!tap_dismisses(
            Points(300.0),
            Some(KeyboardTaps::Always),
            false,
            false
        ));
        assert!(tap_dismisses(
            Points(300.0),
            Some(KeyboardTaps::Never),
            false,
            false
        ));
        assert!(!tap_dismisses(
            Points(300.0),
            Some(KeyboardTaps::Never),
            true,
            false
        ));
        assert!(tap_dismisses(
            Points(300.0),
            Some(KeyboardTaps::Handled),
            false,
            false
        ));
        assert!(!tap_dismisses(
            Points(300.0),
            Some(KeyboardTaps::Handled),
            false,
            true
        ));
        assert!(!tap_dismisses(
            Points(300.0),
            Some(KeyboardTaps::Handled),
            true,
            true
        ));
    }

    #[test]
    fn never_swallows_a_press_that_is_not_on_an_input() {
        assert!(!press_allowed(
            Points(300.0),
            Some(KeyboardTaps::Never),
            false
        ));
        assert!(press_allowed(
            Points(300.0),
            Some(KeyboardTaps::Never),
            true
        ));
        assert!(press_allowed(
            Points(300.0),
            Some(KeyboardTaps::Handled),
            false
        ));
        assert!(press_allowed(
            Points(300.0),
            Some(KeyboardTaps::Always),
            false
        ));
        assert!(press_allowed(Points(300.0), None, false));
    }

    #[test]
    fn a_drag_on_the_scroll_axis_hides_unless_the_mode_is_none() {
        assert!(!drag_dismisses(Points(300.0), KeyboardDismiss::None));
        assert!(drag_dismisses(Points(300.0), KeyboardDismiss::OnDrag));
        assert!(drag_dismisses(Points(300.0), KeyboardDismiss::Interactive));
    }

    #[test]
    fn only_text_and_email_resign_on_return() {
        assert!(return_resigns(KeyboardType::Text));
        assert!(return_resigns(KeyboardType::Email));
        assert!(!return_resigns(KeyboardType::Number));
        assert!(!return_resigns(KeyboardType::Phone));
    }
}
