//! Width a vertical scroll keeps when the engine's content width is wider
//! than the viewport and nothing inside reaches past it.
//!
//! Extracted from `nsh-platform-ios/src/mount.rs` (`settled_content_width`,
//! `narrowed_width`, `still_judged`). The walk that decides `reaches` stays
//! on the UIKit host: a first mount sends the content size before its
//! children's frames, so the caller asks only after the batch has applied
//! them. v1 has no zoom, so the judgement is only "still vertical".

/// The content width a vertical scroll view keeps. `reaches` says whether
/// anything inside it is wider than `viewport`.
pub(crate) fn settled_content_width(viewport: f64, content: f64, reaches: bool) -> f64 {
    if viewport > 0.0 && content > viewport && !reaches {
        viewport
    } else {
        content
    }
}

/// The width a narrowed scroll view keeps. Its content was narrowed to the
/// viewport; the engine's width comes back once something inside reaches past
/// the viewport again. The engine sends no new content size for that batch.
pub(crate) fn narrowed_width(viewport: f64, core: f64, reaches: bool) -> f64 {
    if reaches { core } else { viewport }
}

/// Whether the width judgement applies at all. A node switched to horizontal
/// holds an extent the vertical width never described, so the width narrowed
/// while it was vertical is stale — restoring it would cut the content short.
pub(crate) fn still_judged(horizontal: bool) -> bool {
    !horizontal
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A first mount's children arrive after the content size does; judged
    /// once they are in place, a row wider than the viewport keeps its width.
    #[test]
    fn a_row_wider_than_the_viewport_keeps_its_width() {
        assert_eq!(settled_content_width(320.0, 400.0, true), 400.0);
    }

    /// A width left behind by an earlier layout, with nothing inside reaching
    /// it, narrows to the viewport: the page would pan over nothing.
    #[test]
    fn a_stale_extent_narrows_to_the_viewport() {
        assert_eq!(settled_content_width(320.0, 500.0, false), 320.0);
    }

    /// A narrowed scroll view keeps the viewport while nothing reaches it, and
    /// takes the engine's width back once a later batch widens a row.
    #[test]
    fn a_narrowed_view_takes_the_core_width_back_once_a_row_grows() {
        assert_eq!(narrowed_width(320.0, 500.0, false), 320.0);
        assert_eq!(narrowed_width(320.0, 500.0, true), 500.0);
    }

    /// A node switched to horizontal is no longer judged: the width narrowed
    /// while it was vertical describes nothing it holds now, and giving 500
    /// back to 600 points of horizontal content would leave the last 100
    /// unreachable.
    #[test]
    fn a_scroll_view_that_stops_being_vertical_is_not_judged() {
        assert!(still_judged(false));
        assert!(!still_judged(true));
    }

    /// Content that fits is left alone. A zero viewport is not a real frame
    /// yet, so it is not a reason to narrow.
    #[test]
    fn content_within_the_viewport_is_left_alone() {
        assert_eq!(settled_content_width(320.0, 100.0, false), 100.0);
        assert_eq!(settled_content_width(0.0, 500.0, false), 500.0);
    }
}
