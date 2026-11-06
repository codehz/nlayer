use super::*;
use crate::{
    effects::{Filter, Keyframes, TransitionProps},
    grid,
};
use taffy::{
    AlignContent, AlignItems, Dimension, FlexDirection, LengthPercentage, LengthPercentageAuto,
    Position,
};

use crate::geometry::{relative, size};

/// `fill`, `stroke`, `strokeWidth` and `strokeDasharray` take what CSS
/// takes for them.
#[test]
fn svg_paint_properties() {
    let mut s = Style::default();
    s.set("fill", StyleValue::Str("#ff0000")).unwrap();
    s.set("stroke", StyleValue::Str("currentColor")).unwrap();
    s.set("strokeWidth", StyleValue::Str("2px")).unwrap();
    s.set("strokeDasharray", StyleValue::Str("4, 2 1")).unwrap();
    assert_eq!(s.paint.fill, Some(SvgPaint::Color(Color(0xff00_00ff))));
    assert_eq!(s.paint.stroke, Some(SvgPaint::CurrentColor));
    assert_eq!(s.paint.stroke_width, Some(px(2.0)));
    assert_eq!(
        s.paint.stroke_dasharray.as_deref(),
        Some(&[px(4.0), px(2.0), px(1.0)][..])
    );
    s.set("strokeDasharray", StyleValue::Number(3.0)).unwrap();
    assert_eq!(s.paint.stroke_dasharray.as_deref(), Some(&[px(3.0)][..]));
    // `none`, and dashes that add up to nothing: a solid line.
    for none in ["none", "0 0"] {
        s.set("strokeDasharray", StyleValue::Str(none)).unwrap();
        assert_eq!(s.paint.stroke_dasharray.as_deref(), Some(&[][..]));
    }
    s.set("fill", StyleValue::Str("none")).unwrap();
    assert_eq!(s.paint.fill, Some(SvgPaint::None));
    // Negative lengths are invalid, as in CSS.
    assert!(s.set("strokeWidth", StyleValue::Number(-1.0)).is_err());
    assert!(s.set("strokeDasharray", StyleValue::Str("4 -2")).is_err());
    assert!(s.set("fill", StyleValue::Str("nope")).is_err());
    s.set("stroke", StyleValue::Null).unwrap();
    assert_eq!(s.paint.stroke, None);
}

#[test]
fn transform_properties_keep_css_order_and_their_bindings() {
    use crate::transform::TransformKind::*;

    use crate::animation::AnimationProp::Transform as Step;
    let kinds = |s: &Style| {
        s.view
            .transform
            .ops()
            .iter()
            .map(|o| o.kind)
            .collect::<Vec<_>>()
    };
    let mut s = Style::default();
    s.set_transform(TransformProperty::Scale, &[(Scale, 1.0, Some(7))])
        .unwrap();
    // A `transform` list with an animated step.
    s.set_transform(TransformProperty::Transform, &[(RotateY, 0.0, Some(9))])
        .unwrap();
    assert_eq!(s.animated, vec![(Step(0), 7), (Step(1), 9)]);
    // `translate` goes before them, moving their bindings.
    s.set_key(StyleKey::Translate, StyleValue::Str("10px 20px"))
        .unwrap();
    assert_eq!(kinds(&s), [TranslateX, TranslateY, Scale, RotateY]);
    assert_eq!(s.animated, vec![(Step(2), 7), (Step(3), 9)]);
    // Unset, a property's steps and bindings go.
    s.set_key(StyleKey::Scale, StyleValue::Null).unwrap();
    assert_eq!(kinds(&s), [TranslateX, TranslateY, RotateY]);
    assert_eq!(s.animated, vec![(Step(2), 9)]);
    // A Rust component's bound step goes after them all.
    StyleDecl::Transform(Rotate, 0.0, Some(11))
        .apply(&mut s)
        .unwrap();
    s.set_key(StyleKey::Rotate, StyleValue::Number(30.0))
        .unwrap();
    assert_eq!(kinds(&s), [TranslateX, TranslateY, Rotate, RotateY, Rotate]);
    assert_eq!(s.animated, vec![(Step(3), 9), (Step(4), 11)]);
    // CSS's string replaces the list, its binding with it.
    s.set_key(StyleKey::Transform, StyleValue::Str("rotateX(5deg)"))
        .unwrap();
    assert_eq!(kinds(&s), [TranslateX, TranslateY, Rotate, RotateX, Rotate]);
    assert_eq!(s.animated, vec![(Step(4), 11)]);
    // Skew is not a v1 transform: a view on Android cannot paint it.
    assert_eq!(
        s.set_key(StyleKey::Transform, StyleValue::Str("skewX(5deg)")),
        Err(StyleError::InvalidValue)
    );
    assert_eq!(
        s.set_key(StyleKey::Transform, StyleValue::Str("rotate(45)")),
        Err(StyleError::InvalidValue)
    );
}

#[test]
fn aspect_ratios_and_font_variants_take_css_strings() {
    let ratio = |v| {
        let mut s = Style::default();
        s.set("aspectRatio", v).map(|_| s.layout.aspect_ratio)
    };
    assert_eq!(ratio(StyleValue::Str("16 / 9")), Ok(Some(16.0 / 9.0)));
    assert_eq!(ratio(StyleValue::Str("2/1")), Ok(Some(2.0)));
    assert_eq!(ratio(StyleValue::Str("1.5")), Ok(Some(1.5)));
    assert_eq!(ratio(StyleValue::Number(1.5)), Ok(Some(1.5)));
    assert_eq!(ratio(StyleValue::Str("auto")), Ok(None));
    assert_eq!(
        ratio(StyleValue::Str("1 / 0")),
        Err(StyleError::InvalidValue)
    );
    let mut s = Style::default();
    s.set("fontVariant", StyleValue::Str("tabular-nums small-caps"))
        .unwrap();
    let v = s.text.font_variant.unwrap();
    assert!(v.has(FontVariant::TABULAR_NUMS) && v.has(FontVariant::SMALL_CAPS));
}

#[test]
fn line_height_numbers_are_points() {
    let mut s = Style::default();
    s.set("lineHeight", StyleValue::Number(20.0)).unwrap();
    assert_eq!(s.text.line_height, Some(px(20.0)));
    // Strings are what classes write: a multiple, or a length.
    s.set("lineHeight", StyleValue::Str("1.5")).unwrap();
    assert_eq!(
        (s.text.line_height, s.text.line_height_multiple),
        (None, Some(1.5))
    );
    s.set("lineHeight", StyleValue::Str("150%")).unwrap();
    assert_eq!(s.text.line_height_em, Some(1.5));
    s.set("lineHeight", StyleValue::Str("20px")).unwrap();
    assert_eq!(s.text.line_height, Some(px(20.0)));
    s.set("lineHeight", StyleValue::Null).unwrap();
    assert_eq!(
        (
            s.text.line_height,
            s.text.line_height_multiple,
            s.text.line_height_em
        ),
        (None, None, None)
    );
}

#[test]
fn values_parse_once_and_apply_typed() {
    let key = StyleKey::PaddingTop;
    let value = key.parse_value(StyleValue::Str("25%")).unwrap();
    assert_eq!(value, PropertyValue::Lp(LengthPercentage::percent(0.25)));
    let mut s = Style::default();
    s.apply_value(key, &value).unwrap();
    assert_eq!(s.layout.padding.top, LengthPercentage::percent(0.25));
    // A value of another property's kind is rejected.
    assert_eq!(
        s.apply_value(key, &PropertyValue::Color(None)),
        Err(StyleError::InvalidValue)
    );
    // `null` is the property's default.
    s.apply_value(key, &PropertyValue::Default).unwrap();
    assert_eq!(s.layout.padding.top, LengthPercentage::length(0.0));
    // Equal values hash alike, signed zeros included.
    let hash = |v: &PropertyValue| {
        use std::hash::{BuildHasher, BuildHasherDefault};
        BuildHasherDefault::<rustc_hash::FxHasher>::default().hash_one(v)
    };
    let (a, b) = (
        PropertyValue::Points(px(0.0)),
        PropertyValue::Points(px(-0.0)),
    );
    assert_eq!(a, b);
    assert_eq!(hash(&a), hash(&b));
}

#[test]
fn typed_lengths_are_the_numbers_and_percentages_of_styles() {
    use crate::geometry::{Length, percentage};
    for key in [
        StyleKey::Width,
        StyleKey::MarginLeft,
        StyleKey::Gap,
        StyleKey::LineHeight,
    ] {
        assert_eq!(
            key.length_value(px(12.0).into()),
            key.parse_value(StyleValue::Number(12.0))
        );
    }
    assert_eq!(
        StyleKey::Width.length_value(percentage(0.5).into()),
        StyleKey::Width.parse_value(StyleValue::Str("50%"))
    );
    assert_eq!(
        StyleKey::Top.length_value(Length::Auto),
        StyleKey::Top.parse_value(StyleValue::Str("auto"))
    );
    assert!(StyleKey::Padding.length_value(Length::Auto).is_err());
}

#[test]
fn parse_colors() {
    assert_eq!(Color::parse("#fff"), Some(Color(0xffff_ffff)));
    assert_eq!(Color::parse("#f008"), Some(Color(0xff00_0088)));
    assert_eq!(Color::parse("#123456"), Some(Color(0x1234_56ff)));
    assert_eq!(Color::parse("#12345678"), Some(Color(0x1234_5678)));
    assert_eq!(Color::parse("rgb(255, 0, 10)"), Some(Color(0xff00_0aff)));
    assert_eq!(Color::parse("rgba(0,0,0,0.5)"), Some(Color(0x0000_0080)));
    assert_eq!(Color::parse("white"), Some(Color(0xffff_ffff)));
    assert_eq!(Color::parse("#12"), None);
    assert_eq!(Color::parse("nope"), None);
}

#[test]
fn env_on_padding_and_margin() {
    use EnvVar::*;
    let var = |v| Some(EnvTerm::Var(v));
    let mut s = Style::default();
    let k = StyleValue::Str;
    s.set("paddingBottom", k("env(keyboard-inset-height)"))
        .unwrap();
    assert_eq!(
        s.pending.env.padding,
        [None, None, var(KeyboardInsetHeight), None]
    );
    assert_eq!(s.layout.padding.bottom, LengthPercentage::length(0.0));
    // `calc()` adds a length, on either side.
    s.set("paddingTop", k("calc(env(safe-area-inset-top) + 8px)"))
        .unwrap();
    assert_eq!(s.pending.env.padding[0], var(SafeAreaInsetTop));
    assert_eq!(s.layout.padding.top, LengthPercentage::length(8.0));
    s.set("marginLeft", k("calc(0.5rem + env(safe-area-inset-left))"))
        .unwrap();
    assert_eq!(s.pending.env.margin[3], var(SafeAreaInsetLeft));
    assert_eq!(s.layout.margin.left, LengthPercentageAuto::length(8.0));
    // Margins may go below zero.
    s.set("marginBottom", k("calc(env(keyboard-inset-height) + -4px)"))
        .unwrap();
    assert_eq!(s.layout.margin.bottom, LengthPercentageAuto::length(-4.0));
    // Shorthands set every edge they cover.
    s.set("paddingHorizontal", k("env(safe-area-inset-right)"))
        .unwrap();
    assert_eq!(s.pending.env.padding[1], var(SafeAreaInsetRight));
    assert_eq!(s.pending.env.padding[3], var(SafeAreaInsetRight));
    s.set("margin", k("env(safe-area-inset-top)")).unwrap();
    assert_eq!(s.pending.env.margin, [var(SafeAreaInsetTop); 4]);
    // An unknown variable is its fallback.
    s.set("paddingRight", k("env(titlebar-area-x, 12px)"))
        .unwrap();
    assert_eq!(s.pending.env.padding[1], None);
    assert_eq!(s.layout.padding.right, LengthPercentage::length(12.0));
    // A known one ignores its fallback.
    s.set("paddingRight", k("env(safe-area-inset-right, 12px)"))
        .unwrap();
    assert_eq!(s.pending.env.padding[1], var(SafeAreaInsetRight));
    assert_eq!(s.layout.padding.right, LengthPercentage::length(0.0));
    // The larger of two known variables. The length stays outside.
    s.set(
        "paddingBottom",
        k("max(env(safe-area-inset-bottom), env(keyboard-inset-height))"),
    )
    .unwrap();
    assert_eq!(
        s.pending.env.padding[2],
        Some(EnvTerm::Max(SafeAreaInsetBottom, KeyboardInsetHeight))
    );
    assert_eq!(s.layout.padding.bottom, LengthPercentage::length(0.0));
    s.set(
        "paddingBottom",
        k("calc(max( env(keyboard-inset-height) , env(safe-area-inset-bottom) ) + 32px)"),
    )
    .unwrap();
    assert_eq!(
        s.pending.env.padding[2],
        Some(EnvTerm::Max(KeyboardInsetHeight, SafeAreaInsetBottom))
    );
    assert_eq!(s.layout.padding.bottom, LengthPercentage::length(32.0));
    s.set(
        "marginBottom",
        k("calc(8px + max(env(safe-area-inset-bottom), env(keyboard-inset-height)))"),
    )
    .unwrap();
    assert_eq!(s.layout.margin.bottom, LengthPercentageAuto::length(8.0));
    s.set(
        "marginBottom",
        k("calc(max(env(keyboard-inset-height), env(safe-area-inset-bottom)) + -4px)"),
    )
    .unwrap();
    assert_eq!(s.layout.margin.bottom, LengthPercentageAuto::length(-4.0));
    s.set(
        "paddingLeft",
        k("max(env(safe-area-inset-left), env(safe-area-inset-left))"),
    )
    .unwrap();
    assert_eq!(
        s.pending.env.padding[3],
        Some(EnvTerm::Max(SafeAreaInsetLeft, SafeAreaInsetLeft))
    );
    // A plain value or a reset drops the term.
    s.set("paddingTop", StyleValue::Number(4.0)).unwrap();
    s.set("paddingBottom", StyleValue::Null).unwrap();
    s.set("margin", StyleValue::Null).unwrap();
    assert_eq!(s.pending.env.padding[0], None);
    assert_eq!(s.pending.env.padding[2], None);
    assert_eq!(s.pending.env.margin, [None; 4]);
    // Anything else is rejected.
    for v in [
        "env(titlebar-area-x)",
        "env(safe-area-inset-top) + 8px",
        "calc(env(safe-area-inset-top)+8px)",
        "calc(env(safe-area-inset-top) - 8px)",
        "calc(env(safe-area-inset-top) + env(keyboard-inset-height))",
        "calc(env(safe-area-inset-top) + 10%)",
        "calc(env(safe-area-inset-top) + 1px + 2px)",
        "max(env(safe-area-inset-top), 8px)",
        "min(env(safe-area-inset-top), env(keyboard-inset-height))",
        "max(env(safe-area-inset-top, 8px), env(keyboard-inset-height))",
        "max(env(nope), env(keyboard-inset-height))",
        "max(env(safe-area-inset-top), env(keyboard-inset-height), env(safe-area-inset-left))",
        "max(calc(env(safe-area-inset-top) + 8px), env(keyboard-inset-height))",
        "calc(max(env(safe-area-inset-top), env(keyboard-inset-height)))",
        "calc(max(env(safe-area-inset-top), env(keyboard-inset-height)) + env(safe-area-inset-left))",
        "calc(max(env(safe-area-inset-top), env(keyboard-inset-height))+8px)",
    ] {
        assert_eq!(
            s.set("paddingTop", k(v)),
            Err(StyleError::InvalidValue),
            "{v}"
        );
    }
    // Padding is never negative, including around a max.
    assert_eq!(
        s.set("paddingTop", k("calc(env(safe-area-inset-top) + -8px)")),
        Err(StyleError::InvalidValue)
    );
    assert_eq!(
        s.set(
            "paddingTop",
            k("calc(max(env(safe-area-inset-top), env(keyboard-inset-height)) + -8px)"),
        ),
        Err(StyleError::InvalidValue)
    );
    // Only padding and margin's physical edges take `env()`.
    for key in ["paddingStart", "marginEnd", "top", "width", "gap"] {
        assert!(s.set(key, k("env(safe-area-inset-top)")).is_err(), "{key}");
        assert!(
            s.set(
                key,
                k("max(env(safe-area-inset-top), env(keyboard-inset-height))")
            )
            .is_err(),
            "{key}"
        );
    }
}

#[test]
fn react_native_defaults() {
    let s = Style::default();
    assert_eq!(s.layout.flex_direction, FlexDirection::Column);
    assert_eq!(s.layout.flex_shrink, 0.0);
    assert_eq!(s.layout.position, Position::Relative);
    // Unset: `flex-start` once laid out outside grids.
    assert_eq!(s.layout.align_content, AlignContent::NORMAL);
    let mut layout = s.layout.clone();
    resolve_normal_alignment(&mut layout);
    assert_eq!(layout.align_content, AlignContent::FLEX_START);
    assert_eq!(s.layout.min_size.width, LengthPercentageAuto::length(0.0));
}

#[test]
fn auto_minimum_sizes_are_css_automatic_minimums() {
    let mut s = Style::default();
    s.set("minWidth", StyleValue::Str("auto")).unwrap();
    s.set("minHeight", StyleValue::Str("auto")).unwrap();
    assert_eq!(s.layout.min_size.width, LengthPercentageAuto::auto());
    assert_eq!(s.layout.min_size.height, LengthPercentageAuto::auto());
    // Unset again: back to Yoga's 0.
    s.set("minWidth", StyleValue::Null).unwrap();
    assert_eq!(s.layout.min_size.width, LengthPercentageAuto::length(0.0));
}

#[test]
fn css_alignment_keywords() {
    let mut s = Style::default();
    let k = StyleValue::Str;
    for key in ["alignItems", "alignSelf", "justifyItems", "justifySelf"] {
        for v in ["start", "end", "self-start", "self-end"] {
            s.set(key, k(v)).unwrap();
        }
    }
    assert_eq!(s.layout.justify_self, Some(AlignItems::SELF_END));
    for key in ["alignContent", "justifyContent"] {
        s.set(key, k("end")).unwrap();
        assert_eq!(s.set(key, k("self-end")), Err(StyleError::InvalidValue));
    }
    assert_eq!(s.layout.align_content, AlignContent::END);
    s.set("alignContent", k("normal")).unwrap();
    assert_eq!(s.layout.align_content, AlignContent::NORMAL);
    s.set("justifyContent", StyleValue::Null).unwrap();
    assert_eq!(s.layout.justify_content, AlignContent::NORMAL);
    // A grid keeps CSS's `normal`; other containers take `flex-start`.
    s.set("display", k("grid")).unwrap();
    let mut grid = s.layout.clone();
    resolve_normal_alignment(&mut grid);
    assert_eq!(grid.align_content, AlignContent::NORMAL);
    assert_eq!(
        s.set("gridTemplateColumns", k("[a] 40px [b")),
        Err(StyleError::InvalidValue)
    );
    s.set("gridTemplateRows", k("[a] 40px [b] 1fr")).unwrap();
    assert_eq!(s.layout.grid_template_row_names.len(), 3);
    s.set("gridTemplateRows", StyleValue::Null).unwrap();
    assert!(s.layout.grid_template_row_names.is_empty());
}

#[test]
fn transform_origins() {
    let p = |s| TransformOrigin::parse(s).unwrap();
    assert_eq!(p("top left").x, relative(0.0));
    assert_eq!(p("top left").y, relative(0.0));
    assert_eq!(p("bottom").y, relative(1.0));
    assert_eq!(p("bottom").x, relative(0.5));
    assert_eq!(p("10px 25%").x, DefiniteLength::from(px(10.0)));
    assert_eq!(p("10px 25%").y, relative(0.25));
    assert_eq!(p("right 5 3").z, px(3.0));
    assert_eq!(p("center top").y, relative(0.0));
    assert_eq!(
        p("0 0").offset(size(px(100.0), px(50.0))),
        (point(px(-50.0), px(-25.0)), Points::ZERO)
    );
    assert!(TransformOrigin::parse("sideways").is_none());
}

#[test]
fn font_variants() {
    let v = FontVariant::parse("small-caps tabular-nums").unwrap();
    assert_eq!(v.tags().collect::<Vec<_>>(), ["smcp", "tnum"]);
    assert!(FontVariant::parse("wide").is_none());
}

#[test]
fn text_align_vertical_places_text_down_its_box() {
    let at = |v: &str| {
        let mut s = Style::default();
        s.set("textAlignVertical", StyleValue::Str(v)).unwrap();
        s.text.align_vertical
    };
    assert_eq!(at("top"), Some(percentage(0.0)));
    assert_eq!(at("center"), Some(percentage(0.5)));
    assert_eq!(at("bottom"), Some(percentage(1.0)));
    // `alignContent` is layout only.
    let mut s = Style::default();
    s.set("alignContent", StyleValue::Str("center")).unwrap();
    assert_eq!(s.text.align_vertical, None);
}

#[test]
fn flex_shorthand() {
    // CSS's: grow, shrink and basis.
    let flex = |v: StyleValue<'_>| {
        let mut s = Style::default();
        s.set("flex", v).map(|()| {
            (
                s.layout.flex_grow,
                s.layout.flex_shrink,
                s.layout.flex_basis,
            )
        })
    };
    let (n, t) = (StyleValue::Number, StyleValue::Str);
    let (zero, auto) = (Dimension::length(0.0), Dimension::auto());
    assert_eq!(flex(n(2.0)), Ok((2.0, 1.0, zero)));
    // `0` shrinks as CSS's does (React Native's did not).
    assert_eq!(flex(n(0.0)), Ok((0.0, 1.0, zero)));
    assert_eq!(flex(t("none")), Ok((0.0, 0.0, auto)));
    assert_eq!(flex(t("auto")), Ok((1.0, 1.0, auto)));
    assert_eq!(flex(t("initial")), Ok((0.0, 1.0, auto)));
    assert_eq!(flex(t("2 0 50%")), Ok((2.0, 0.0, Dimension::percent(0.5))));
    assert_eq!(flex(t("30px 2")), Ok((2.0, 1.0, Dimension::length(30.0))));
    assert_eq!(flex(t("1 0")), Ok((1.0, 0.0, zero)));
    assert_eq!(flex(t("0 0 0")), Ok((0.0, 0.0, zero)));
    assert_eq!(flex(StyleValue::Null), Ok((0.0, 0.0, auto)));
    // Negative factors (React Native's `flex: -1`) are no CSS.
    for v in [n(-1.0), t("-1"), t("1 -1"), t("1 2 3 4"), t("nope")] {
        assert_eq!(flex(v), Err(StyleError::InvalidValue), "{v:?}");
    }
}

/// Every order of `decls`.
fn permutations<T: Clone>(decls: &[T]) -> Vec<Vec<T>> {
    if decls.len() <= 1 {
        return vec![decls.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..decls.len() {
        let mut rest = decls.to_vec();
        let first = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, first.clone());
            out.push(tail);
        }
    }
    out
}

/// Checks the style of one style object with `decls` in every order:
/// as recorded, and replayed from the patch (over classes).
fn in_any_order(decls: &[(&str, StyleValue<'_>)], check: impl Fn(&Style)) {
    for order in permutations(decls) {
        let mut style = Style::default();
        let mut patch = StylePatch::default();
        for &(key, value) in &order {
            patch.set(&mut style, key, value).unwrap();
        }
        let keys: Vec<_> = order.iter().map(|(k, _)| *k).collect();
        println!("{keys:?}");
        check(&style);
        let mut replayed = Style::default();
        patch.apply(&mut replayed);
        check(&replayed);
    }
}

#[test]
fn border_longhands_override_the_shorthand_in_any_order() {
    use StyleValue::{Null, Number, Str};
    in_any_order(
        &[
            ("borderTopWidth", Number(5.0)),
            ("borderWidth", Number(2.0)),
            ("borderLeftColor", Str("red")),
            ("borderColor", Str("blue")),
        ],
        |s| {
            assert_eq!(
                s.view.borders.widths.to_array().map(|w| w.0),
                [5.0, 2.0, 2.0, 2.0]
            );
            assert_eq!(s.layout.border.top, LengthPercentage::length(5.0));
            assert_eq!(s.layout.border.left, LengthPercentage::length(2.0));
            let (red, blue) = (Color(0xff00_00ff), Color(0x0000_ffff));
            assert_eq!(s.view.borders.colors, [blue, blue, blue, red]);
        },
    );
    // A null longhand is unset: the shorthand shows.
    in_any_order(
        &[("borderTopWidth", Null), ("borderWidth", Number(2.0))],
        |s| assert_eq!(s.view.borders.widths.to_array().map(|w| w.0), [2.0; 4]),
    );
    // Without its shorthand, null still resets the longhand (a class's).
    let mut s = Style::default();
    s.set("borderTopWidth", Number(4.0)).unwrap();
    StylePatch::default()
        .set(&mut s, "borderTopWidth", Null)
        .unwrap();
    assert_eq!(s.view.borders.widths.top, Points::ZERO);
}

#[test]
fn edge_longhands_override_their_shorthands_in_any_order() {
    use StyleValue::Number;
    let len = LengthPercentageAuto::length;
    in_any_order(
        &[
            ("marginTop", Number(1.0)),
            ("marginVertical", Number(2.0)),
            ("margin", Number(3.0)),
            ("marginLeft", Number(4.0)),
            ("marginHorizontal", Number(5.0)),
        ],
        |s| {
            let m = s.layout.margin;
            assert_eq!(
                [m.top, m.bottom, m.left, m.right],
                [len(1.0), len(2.0), len(4.0), len(5.0)]
            );
        },
    );
    let len = LengthPercentage::length;
    in_any_order(
        &[
            ("paddingRight", Number(1.0)),
            ("padding", Number(3.0)),
            ("paddingVertical", Number(2.0)),
        ],
        |s| {
            let p = s.layout.padding;
            assert_eq!(
                [p.top, p.bottom, p.left, p.right],
                [len(2.0), len(2.0), len(3.0), len(1.0)]
            );
        },
    );
    in_any_order(&[("rowGap", Number(4.0)), ("gap", Number(8.0))], |s| {
        assert_eq!(s.layout.gap.height, len(4.0));
        assert_eq!(s.layout.gap.width, len(8.0));
    });
}

#[test]
fn flex_and_grid_longhands_override_their_shorthands_in_any_order() {
    use StyleValue::{Number, Str};
    in_any_order(&[("flexGrow", Number(2.0)), ("flex", Number(1.0))], |s| {
        assert_eq!(s.layout.flex_grow, 2.0);
        assert_eq!(s.layout.flex_shrink, 1.0);
        assert_eq!(s.layout.flex_basis, Dimension::length(0.0));
    });
    in_any_order(
        &[
            ("gridColumnStart", Number(2.0)),
            ("gridColumn", Str("1 / 4")),
        ],
        |s| {
            assert_eq!(
                Some(&s.layout.grid_column.start),
                grid::placement("2").as_ref()
            );
            assert_eq!(
                Some(&s.layout.grid_column.end),
                grid::placement("4").as_ref()
            );
        },
    );
}

#[test]
fn outline_style_none_hides_the_outline_in_any_order() {
    use StyleValue::{Null, Number, Str};
    in_any_order(
        &[("outlineStyle", Str("none")), ("outlineWidth", Number(3.0))],
        |s| assert!(!s.view.outline.is_visible()),
    );
    in_any_order(
        &[
            ("outlineStyle", Str("dashed")),
            ("outlineWidth", Number(3.0)),
        ],
        |s| {
            assert_eq!(s.view.outline.width, px(3.0));
            assert_eq!(s.view.outline.style, BorderStyle::Dashed);
        },
    );
    // A later style (inline over classes) showing it again gets the
    // width back.
    let mut s = Style::default();
    s.set("outlineWidth", Number(3.0)).unwrap();
    s.set("outlineStyle", Str("none")).unwrap();
    s.set("outlineStyle", Null).unwrap();
    assert_eq!(s.view.outline.width, px(3.0));
}

#[test]
fn border_style_none_hides_the_border_in_any_order() {
    use StyleValue::{Null, Number, Str};
    for none in ["none", "hidden"] {
        in_any_order(
            &[
                ("borderStyle", Str(none)),
                ("borderWidth", Number(3.0)),
                ("borderTopWidth", Number(4.0)),
                ("borderStartWidth", Number(5.0)),
            ],
            |s| {
                assert!(!s.view.borders.is_visible());
                assert_eq!(s.view.borders.widths.to_array(), [Points::ZERO; 4]);
                assert_eq!(s.layout.border.top, LengthPercentage::length(0.0));
                assert_eq!(s.layout.border.left, LengthPercentage::length(0.0));
                assert_eq!(s.logical.border_width, [None; 2]);
            },
        );
    }
    // A later style showing it again gets the widths back.
    let mut s = Style::default();
    s.set("borderWidth", Number(3.0)).unwrap();
    s.set("borderEndWidth", Number(1.0)).unwrap();
    s.set("borderStyle", Str("none")).unwrap();
    s.set("borderStyle", Str("dashed")).unwrap();
    assert_eq!(s.view.borders.widths.to_array(), [px(3.0); 4]);
    assert_eq!(s.layout.border.right, LengthPercentage::length(3.0));
    assert_eq!(s.logical.border_width, [None, Some(px(1.0))]);
    assert_eq!(s.view.borders.style, BorderStyle::Dashed);
    s.set("borderStyle", Str("hidden")).unwrap();
    s.set("borderStyle", Null).unwrap();
    assert_eq!(s.view.borders.widths.to_array(), [px(3.0); 4]);
}

#[test]
fn animation_keys_apply_in_any_order() {
    use StyleValue::{Number, Str};
    in_any_order(
        &[
            ("animationDuration", Number(500.0)),
            ("animationIterationCount", Number(2.0)),
            ("animationName", Str("spin")),
            ("animationDelay", Str("0.1s")),
            ("animationTimingFunction", Str("ease-in")),
        ],
        |s| {
            let a = s.view.animation.unwrap();
            assert_eq!(a.keyframes, Keyframes::Spin);
            assert_eq!(a.duration, 500.0);
            assert_eq!(a.iterations, Some(2));
            assert_eq!(a.delay, 100.0);
            assert_eq!(a.easing, [0.42, 0.0, 1.0, 1.0]);
        },
    );
    // Without a name nothing runs.
    in_any_order(&[("animationDuration", Number(500.0))], |s| {
        assert_eq!(s.view.animation, None)
    });
    // Over a running (class) animation, the values change it.
    let mut s = Style::default();
    s.set("animationName", Str("ping")).unwrap();
    s.set("animationDuration", Number(300.0)).unwrap();
    s.set("animationName", Str("spin")).unwrap();
    assert_eq!(s.view.animation.unwrap().duration, 300.0);
}

#[test]
fn transition_keys_apply_in_any_order() {
    use StyleValue::{Number, Str};
    in_any_order(
        &[
            ("transitionDuration", Number(300.0)),
            ("transitionDelay", Number(50.0)),
            ("transitionTimingFunction", Str("linear")),
            ("transitionProperty", Str("opacity")),
        ],
        |s| {
            let t = s.view.transition.unwrap();
            assert_eq!(t.properties, TransitionProps(TransitionProps::OPACITY));
            assert_eq!(t.duration, 300.0);
            assert_eq!(t.delay, 50.0);
            assert_eq!(t.easing, [0.0, 0.0, 1.0, 1.0]);
        },
    );
}

#[test]
fn corner_radii_override_the_shorthand_in_any_order() {
    let mut s = Style::default();
    s.set("borderTopLeftRadius", StyleValue::Number(4.0))
        .unwrap();
    s.set("borderRadius", StyleValue::Number(10.0)).unwrap();
    let r = s.view.radii();
    assert_eq!(r.to_array(), [px(4.0), px(10.0), px(10.0), px(10.0)]);
    assert_eq!(r.shared(), None);
    s.set("borderTopLeftRadius", StyleValue::Null).unwrap();
    assert_eq!(s.view.radii().shared(), Some(px(10.0)));
}

#[test]
fn shared_radius_ignores_square_corners() {
    let r = CornerRadii {
        top_left: px(12.0),
        top_right: px(12.0),
        ..CornerRadii::default()
    };
    assert_eq!(r.shared(), Some(px(12.0)));
    assert_eq!(CornerRadii::default().shared(), Some(px(0.0)));
    assert!(CornerRadii::default().is_zero());
}

#[test]
fn radii_fit_the_box() {
    // 100 + 300 on a 200 wide box: everything scales by 0.5.
    let r = CornerRadii {
        top_left: px(100.0),
        top_right: px(300.0),
        bottom_right: px(0.0),
        bottom_left: px(0.0),
        continuous: false,
    }
    .fit(size(px(200.0), px(1000.0)));
    assert_eq!(r.to_array(), [px(50.0), px(150.0), px(0.0), px(0.0)]);
    let fits = CornerRadii::all(px(10.0));
    assert_eq!(fits.fit(size(px(100.0), px(100.0))), fits);
    assert_eq!(fits.inset(px(4.0)), CornerRadii::all(px(6.0)));
    assert_eq!(fits.inset(px(40.0)), CornerRadii::all(px(0.0)));
}

#[test]
fn a_replayed_flex_display_keeps_its_row() {
    // A Rust component's host replays an app's style with `push`.
    let mut first = Style::default();
    let mut patch = StylePatch::default();
    patch
        .set(&mut first, "display", StyleValue::Str("flex"))
        .unwrap();
    let mut style = Style::default();
    let mut replayed = StylePatch::default();
    for decl in patch.decls() {
        replayed.push(&mut style, decl.clone()).unwrap();
    }
    assert_eq!(style.layout.flex_direction, FlexDirection::Row);
    // A width with its own side's color reads no text color.
    let mut patch = StylePatch::default();
    patch
        .set(&mut first, "borderBottomWidth", StyleValue::Number(1.0))
        .unwrap();
    assert!(patch.reads_current_color());
    patch
        .set(&mut first, "borderBottomColor", StyleValue::Str("#eee"))
        .unwrap();
    assert!(!patch.reads_current_color());
}

#[test]
fn current_color_written_out_is_the_text_color() {
    let text = Color(0x1234_56ff);
    let mut s = Style::default();
    let mut patch = StylePatch::default();
    for (key, value) in [
        ("backgroundColor", "currentColor"),
        ("borderColor", "CurrentColor"),
        // A later value wins over `currentColor`.
        ("borderTopColor", "red"),
        ("outlineColor", "currentcolor"),
        ("textDecorationColor", "currentColor"),
        ("boxShadow", "0 2px 4px currentColor"),
        ("textShadow", "0 1px 2px currentColor"),
    ] {
        patch.set(&mut s, key, StyleValue::Str(value)).unwrap();
    }
    assert!(patch.reads_current_color());
    s.apply_current_color(text);
    assert_eq!(s.view.background_color, text);
    assert_eq!(
        s.view.borders.colors,
        [Color(0xff00_00ff), text, text, text]
    );
    assert_eq!(s.view.outline.color, text);
    assert_eq!(s.text.decoration_color, Some(text));
    assert_eq!(s.view.effects.as_ref().unwrap().box_shadows[0].color, text);
    assert_eq!(s.text.shadow_color, Some(text));
    // And `currentColor` wins over an earlier value.
    let mut s = Style::default();
    s.set("backgroundColor", StyleValue::Str("red")).unwrap();
    s.set("backgroundColor", StyleValue::Str("currentColor"))
        .unwrap();
    s.apply_current_color(text);
    assert_eq!(s.view.background_color, text);
    // `color: currentColor` is the inherited color.
    s.set("color", StyleValue::Str("red")).unwrap();
    s.set("color", StyleValue::Str("currentColor")).unwrap();
    assert_eq!(s.text.color, None);
    // Where nothing fills it in, it is no color: rejected, never black.
    assert_eq!(
        s.set("tintColor", StyleValue::Str("currentColor")),
        Err(StyleError::InvalidValue)
    );
    assert_eq!(crate::css::box_shadows("0 2px 4px currentColor"), None);
}

#[test]
fn current_color_inside_values_is_the_text_color() {
    let text = Color(0x1234_56ff);
    let close = |a: Color, b: u32| {
        (0..4).all(|i| ((a.0 >> (i * 8)) & 0xff).abs_diff((b >> (i * 8)) & 0xff) <= 1)
    };
    let mut s = Style::default();
    let mut set = |key: &str, value: &str| s.set(key, StyleValue::Str(value));
    set("backgroundImage", "linear-gradient(currentColor, #ffffff)").unwrap();
    // CSS: a `drop-shadow()` without a color is the text color's too.
    set(
        "filter",
        "drop-shadow(0 1px 2px) drop-shadow(0 1px 2px CurrentColor) drop-shadow(0 1px 2px red)",
    )
    .unwrap();
    set(
        "boxShadow",
        "0 1px 2px color-mix(in srgb, currentColor 50%, transparent)",
    )
    .unwrap();
    set(
        "backgroundColor",
        "color-mix(in srgb, currentColor, #ffffff)",
    )
    .unwrap();
    // Checked with a stand-in color: a wrong value is rejected now.
    assert_eq!(
        set("maskImage", "linear-gradient(nope(currentColor), red)"),
        Err(StyleError::InvalidValue)
    );
    // `color` takes it alone only (the inherited color).
    assert_eq!(
        set("color", "color-mix(in srgb, currentColor, red)"),
        Err(StyleError::InvalidValue)
    );
    s.apply_current_color(text);
    let effects = s.view.effects.as_ref().unwrap();
    assert_eq!(effects.background_images[0].stops[0].color, text);
    let shadow_colors: Vec<Color> = effects
        .filters
        .iter()
        .map(|f| match f {
            Filter::DropShadow { color, .. } => *color,
            _ => Color::TRANSPARENT,
        })
        .collect();
    assert_eq!(shadow_colors, [text, text, Color(0xff00_00ff)]);
    assert!(close(effects.box_shadows[0].color, 0x1234_5680));
    assert!(
        close(s.view.background_color, 0x899a_abff),
        "{:x}",
        s.view.background_color.0
    );
    // A style with a `drop-shadow()` that names no color reads it.
    let mut patch = StylePatch::default();
    let mut styled = Style::default();
    patch
        .set(
            &mut styled,
            "filter",
            StyleValue::Str("drop-shadow(0 1px 2px)"),
        )
        .unwrap();
    assert!(patch.reads_current_color());
    // A later value replaces one that waits for the color.
    s.set("backgroundImage", StyleValue::Str("none")).unwrap();
    s.apply_current_color(text);
    assert!(
        s.view
            .effects
            .as_ref()
            .unwrap()
            .background_images
            .is_empty()
    );
}

#[test]
fn box_shadows_take_any_css_color() {
    // A color expression of the app's (`color-mix()`) is one word of the
    // layer.
    let mut s = Style::default();
    s.set(
        "boxShadow",
        StyleValue::Str("0 6px 18px color-mix(in srgb, #00ff00 50%, transparent)"),
    )
    .unwrap();
    let layer = s.view.effects.as_ref().unwrap().box_shadows[0];
    assert_eq!((layer.offset_y, layer.blur), (px(6.0), px(18.0)));
    assert_eq!(layer.color.0 >> 8, 0x00ff00);
    assert!(
        (0x7e..=0x81).contains(&(layer.color.0 & 0xff)),
        "{:x}",
        layer.color.0
    );
}

#[test]
fn text_shadows_are_css() {
    // The widest layer stands for all, as the classes' does.
    let mut s = Style::default();
    s.set(
        "textShadow",
        StyleValue::Str("0 1px 1px rgb(0 0 0 / 0.2), 2px 3px 6px red"),
    )
    .unwrap();
    assert_eq!(s.text.shadow_radius, Some(px(6.0)));
    assert_eq!(s.text.shadow_offset, Some(point(px(2.0), px(3.0))));
    assert_eq!(s.text.shadow_color, Color::parse("red"));
    // A layer without a color is the text color, filled in once known.
    s.set("textShadow", StyleValue::Str("0 1px 2px")).unwrap();
    s.apply_current_color(Color(0x1234_56ff));
    assert_eq!(s.text.shadow_color, Some(Color(0x1234_56ff)));
    assert_eq!(
        s.set("textShadow", StyleValue::Str("inset 0 1px red")),
        Err(StyleError::InvalidValue)
    );
    s.set("textShadow", StyleValue::Str("none")).unwrap();
    assert_eq!(s.text.shadow_radius, None);
}

#[test]
fn text_styles_parse() {
    let mut s = Style::default();
    s.set("fontFamily", StyleValue::Str("Inter")).unwrap();
    s.set("fontStyle", StyleValue::Str("italic")).unwrap();
    s.set(
        "textDecorationLine",
        StyleValue::Str("underline line-through"),
    )
    .unwrap();
    s.set("textTransform", StyleValue::Str("capitalize"))
        .unwrap();
    assert_eq!(s.text.font_family, Some(FontFamily::new("Inter")));
    assert_eq!(&*s.text.font_family.unwrap().name(), "Inter");
    assert_eq!(s.text.italic, Some(true));
    assert_eq!(
        s.text.decoration,
        Some(TextDecoration {
            underline: true,
            line_through: true
        })
    );
    assert_eq!(
        TextTransform::Capitalize.apply("hello wörld-x  ünd"),
        "Hello Wörld-x  Ünd"
    );
    assert_eq!(
        s.set("fontStyle", StyleValue::Str("oblique")),
        Err(StyleError::InvalidValue)
    );
}

#[test]
fn mask_image_parses() {
    let mut s = Style::default();
    assert!(s.view.effects.is_none());
    s.set(
        "maskImage",
        StyleValue::Str("linear-gradient(black 70%, transparent)"),
    )
    .unwrap();
    let masks = &s.view.effects.as_ref().unwrap().mask_images;
    assert_eq!(masks.len(), 1);
    // `none`, and the null of a style that was taken off, both clear it.
    s.set("maskImage", StyleValue::Str("none")).unwrap();
    assert!(
        s.view
            .effects
            .as_ref()
            .is_none_or(|e| e.mask_images.is_empty())
    );
    s.set(
        "maskImage",
        StyleValue::Str("linear-gradient(black, transparent)"),
    )
    .unwrap();
    s.set("maskImage", StyleValue::Null).unwrap();
    assert!(
        s.view
            .effects
            .as_ref()
            .is_none_or(|e| e.mask_images.is_empty())
    );
    assert_eq!(
        s.set("maskImage", StyleValue::Str("url(a.png)")),
        Err(StyleError::InvalidValue)
    );
}

#[test]
fn glass_effect_parses() {
    let mut s = Style::default();
    assert_eq!(s.view.glass_effect, GlassEffect::None);
    s.set("glassEffect", StyleValue::Str("regular")).unwrap();
    assert_eq!(s.view.glass_effect, GlassEffect::Regular);
    s.set("glassEffect", StyleValue::Str("clear")).unwrap();
    assert_eq!(s.view.glass_effect, GlassEffect::Clear);
    // `none`, and the null of a style that was taken off, both clear it.
    s.set("glassEffect", StyleValue::Str("none")).unwrap();
    assert_eq!(s.view.glass_effect, GlassEffect::None);
    s.set("glassEffect", StyleValue::Str("clear")).unwrap();
    s.set("glassEffect", StyleValue::Null).unwrap();
    assert_eq!(s.view.glass_effect, GlassEffect::None);
    assert_eq!(
        s.set("glassEffect", StyleValue::Str("frosted")),
        Err(StyleError::InvalidValue)
    );
    assert_eq!(
        s.set("glassEffect", StyleValue::Number(1.0)),
        Err(StyleError::InvalidValue)
    );
}

#[test]
fn rejects_bad_values() {
    let mut s = Style::default();
    assert_eq!(
        s.set("width", StyleValue::Str("wide")),
        Err(StyleError::InvalidValue)
    );
    assert_eq!(
        s.set("nope", StyleValue::Number(1.0)),
        Err(StyleError::UnknownProperty)
    );
    s.set("width", StyleValue::Str("50%")).unwrap();
    assert_eq!(s.layout.size.width, Dimension::percent(0.5));
}
