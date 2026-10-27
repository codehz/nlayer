//! Props the style IR stores before the rest of the prop model is extracted.
//!
//! `PointerEvents` is copied from `nsh_core::props`. The `PropEnum` derive
//! (JS string names) arrives with the element macros.

/// Which touches a view and its children receive (`pointerEvents`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)] // applied when the shadow tree reads styles
pub enum PointerEvents {
    /// The view and its children receive touches (default).
    #[default]
    Auto,
    /// Neither the view nor its children receive touches.
    None,
    /// Only the children receive touches; the view itself is transparent to them.
    /// JS name: `box-none`.
    BoxNone,
    /// Only the view receives touches, never its children.
    /// JS name: `box-only`.
    BoxOnly,
}
