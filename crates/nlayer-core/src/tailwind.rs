//! The class side-channel a [`crate::style::Style`] fills.
//!
//! v1 does not parse Tailwind utilities; the table is later, and the
//! parser stays empty. `ClassProps` and `Space` are the fields the
//! extracted style IR already writes, copied from `nsh_core::tailwind`.
#![allow(dead_code)]

use crate::{
    props::PointerEvents,
    style::{Color, EllipsizeMode, PropertyValue, Style, StyleKey, StyleValue, TextShadow},
};

/// Values classes and the style itself contribute besides the layout style.
#[derive(Debug, Clone, Default, PartialEq)]
#[allow(dead_code)] // the class parser and the shadow tree are not extracted yet
pub(crate) struct ClassProps {
    /// `numberOfLines` (`line-clamp-*`, `truncate`); `Some(0)` removes a clamp.
    pub number_of_lines: Option<u32>,
    /// `textOverflow` (`text-ellipsis`, `text-clip`, `truncate`).
    pub ellipsize: Option<EllipsizeMode>,
    /// `whitespace-nowrap`, `text-nowrap`, `truncate`: lines never wrap.
    pub no_wrap: Option<bool>,
    /// `pointerEvents` (`pointer-events-none` / `-auto`).
    pub pointer_events: Option<PointerEvents>,
    /// `userSelect` (`select-none` / `select-text`; `auto`: none).
    pub selectable: Option<bool>,
    /// `placeholder:text-*` (inputs).
    pub placeholder_color: Option<Color>,
    /// `selection:bg-*`: the highlight of a selection.
    pub selection_color: Option<Color>,
    /// `caret-*` (inputs).
    pub caret_color: Option<Color>,
    /// `text-shadow-*` layers beyond the first.
    pub text_shadows: Option<Vec<TextShadow>>,
    /// `space-x-*` / `space-y-*`: margins the view gives its children.
    pub space: Option<Space>,
}

/// `space-x-*` / `space-y-*` (and `-reverse`): margins a view gives each of
/// its children but the last.
#[derive(Debug, Clone, Default, PartialEq)]
#[allow(dead_code)]
#[non_exhaustive]
pub(crate) struct Space {
    /// `space-x-*`: the end margin.
    pub x: Option<PropertyValue>,
    /// `space-y-*`: the bottom margin.
    pub y: Option<PropertyValue>,
    /// `space-x-reverse`.
    pub reverse_x: bool,
    /// `space-y-reverse`.
    pub reverse_y: bool,
    /// The horizontal margins override normal child declarations.
    pub important_x: bool,
    /// The vertical margins override normal child declarations.
    pub important_y: bool,
}

impl Space {
    /// The margins of a child laid out in direction `rtl`.
    pub(crate) fn apply(&self, style: &mut Style, important: bool, rtl: bool) {
        use StyleKey as K;
        if let Some(v) = &self.x
            && self.important_x == important
        {
            let (end, start) = if rtl {
                (K::MarginLeft, K::MarginRight)
            } else {
                (K::MarginRight, K::MarginLeft)
            };
            let (key, other) = if self.reverse_x { (start, end) } else { (end, start) };
            let _ = style.set_key(other, StyleValue::Number(0.0));
            let _ = style.apply_value(key, v);
        }
        if let Some(v) = &self.y
            && self.important_y == important
        {
            let key = if self.reverse_y { K::MarginTop } else { K::MarginBottom };
            let other = if self.reverse_y { K::MarginBottom } else { K::MarginTop };
            let _ = style.set_key(other, StyleValue::Number(0.0));
            let _ = style.apply_value(key, v);
        }
    }
}
