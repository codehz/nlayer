use taffy::{Dimension, Display, FlexDirection, LengthPercentage, LengthPercentageAuto};

use crate::{
    ResizeMode,
    geometry::{Size, px},
    style::{Color, Style, StyleKey, StyleValue},
    transform::TransformKind,
};

use super::{ClassEnvironment, ClassList, ClassProps, Context, Theme, parse, state};

fn applied(classes: &str, states: u16) -> (Style, ClassProps, ClassList) {
    let theme = Theme::default();
    let list = parse(&theme, classes);
    let mut style = Style::default();
    let mut props = ClassProps::default();
    let group = |_: &str| 0;
    let cx = Context {
        env: ClassEnvironment::default(),
        window: Size::new(px(390.0), px(844.0)),
        rtl: false,
        states,
        position: 0,
        group: &group,
    };
    list.apply(&cx, &theme, &mut style, &mut props, |_| {});
    (style, props, list)
}

fn padding(classes: &str, states: u16) -> taffy::Rect<LengthPercentage> {
    applied(classes, states).0.layout.padding
}

fn len(n: f32) -> LengthPercentage {
    LengthPercentage::length(n)
}

#[test]
fn utilities_write_style_or_props() {
    let (s, mut props, list) = applied(
        "flex p-4 gap-3 bg-zinc-800 text-sm font-semibold rounded-lg opacity-50 translate-x-1 object-cover line-clamp-2 space-x-2",
        0,
    );
    assert!(list.unknown().is_empty(), "{:?}", list.unknown());
    assert!(list.unsupported().is_empty(), "{:?}", list.unsupported());
    assert_eq!(s.layout.display, Display::Flex);
    assert_eq!(s.layout.flex_direction, FlexDirection::Row);
    assert_eq!(s.layout.padding.top, len(16.0));
    assert_eq!(s.layout.padding.left, len(16.0));
    assert_eq!(s.layout.gap.width, len(12.0));
    assert_eq!(s.layout.gap.height, len(12.0));
    assert_eq!(s.view.background_color, Color(0x2727_2aff));
    assert_eq!(s.text.font_size, Some(px(14.0)));
    assert_eq!(s.text.font_weight, Some(600));
    assert_eq!(s.view.border_radius, px(8.0));
    assert_eq!(s.view.opacity, 0.5);
    assert_eq!(
        s.view.transform.ops(),
        &[crate::transform::TransformOp {
            kind: TransformKind::TranslateX,
            value: 4.0,
        }]
    );
    assert_eq!(s.object_fit(), Some(ResizeMode::Cover));
    s.node_props(&mut props);
    assert_eq!(props.number_of_lines, Some(2));
    let mut child = Style::default();
    props.space.unwrap().apply(&mut child, false, false);
    assert_eq!(child.layout.margin.right, LengthPercentageAuto::length(8.0));
}

#[test]
fn arbitrary_lengths_and_colors() {
    let (s, _, list) = applied("bg-[#7c3aed] w-[37px]", 0);
    assert!(list.unknown().is_empty() && list.unsupported().is_empty());
    assert_eq!(s.view.background_color, Color(0x7c3a_edff));
    assert_eq!(s.layout.size.width, Dimension::length(37.0));
}

#[test]
fn later_equal_rank_wins_and_variants_follow_tailwind_order() {
    assert_eq!(padding("p-2 p-4", 0).top, len(16.0));
    // Written first, still after the plain rule.
    assert_eq!(padding("hover:p-2 p-8", 0).top, len(32.0));
    assert_eq!(padding("hover:p-2 p-8", state::HOVERED).top, len(8.0));
    assert_eq!(padding("active:p-1 p-8", 0).top, len(32.0));
    assert_eq!(padding("active:p-1 p-8", state::PRESSED).top, len(4.0));
}

#[test]
fn important_beats_an_inline_style() {
    let theme = Theme::default();
    let list = parse(&theme, "!p-4");
    let mut style = Style::default();
    let mut props = ClassProps::default();
    let group = |_: &str| 0;
    let cx = Context {
        env: ClassEnvironment::default(),
        window: Size::new(px(390.0), px(844.0)),
        rtl: false,
        states: 0,
        position: 0,
        group: &group,
    };
    list.apply(&cx, &theme, &mut style, &mut props, |style| {
        style
            .set_key(StyleKey::PaddingTop, StyleValue::Number(1.0))
            .unwrap();
    });
    assert_eq!(style.layout.padding.top, len(16.0));
    let (plain, _, _) = applied("p-4", 0);
    let mut beaten = plain;
    beaten
        .set_key(StyleKey::PaddingTop, StyleValue::Number(1.0))
        .unwrap();
    let mut inline_wins = Style::default();
    parse(&theme, "p-4").apply(&cx, &theme, &mut inline_wins, &mut props, |style| {
        style
            .set_key(StyleKey::PaddingTop, StyleValue::Number(1.0))
            .unwrap();
    });
    assert_eq!(inline_wins.layout.padding.top, beaten.layout.padding.top);
    assert_eq!(inline_wins.layout.padding.top, len(1.0));
}

#[test]
fn selection_passes_and_placeholder_does_not() {
    let (_, props, list) = applied("selection:bg-red-500", 0);
    assert!(list.unsupported().is_empty() && list.unknown().is_empty());
    assert_eq!(props.selection_color, Some(Color(0xfb2c_36ff)));
    let (_, props, list) = applied("placeholder:text-zinc-400", 0);
    assert_eq!(
        list.unsupported(),
        ["placeholder:text-zinc-400".into()].as_slice()
    );
    assert!(list.unknown().is_empty());
    assert_eq!(props.placeholder_color, None);
}

#[test]
fn gate_drops_the_whole_utility() {
    for class in [
        "shadow-md",
        "bg-linear-to-r",
        "blur-md",
        "ring",
        "outline",
        "animate-spin",
        "transition",
        "dark:bg-white",
        "sm:flex",
        "w-screen",
        "portrait:flex",
        "ios:flex",
        "motion-reduce:flex",
        "rtl:ml-2",
        "group-hover:flex",
        "group",
        "first:flex",
        "odd:flex",
        "aria-selected:flex",
        "caret-red-500",
    ] {
        let (style, props, list) = applied(class, 0);
        assert_eq!(list.unsupported(), [class.into()].as_slice(), "{class}");
        assert!(list.unknown().is_empty(), "{class}");
        let base = Style::default();
        assert_eq!(style.layout, base.layout, "{class}");
        assert_eq!(style.view, base.view, "{class}");
        assert_eq!(style.text, base.text, "{class}");
        assert!(list.groups().is_empty(), "{class}");
        assert_eq!(props, ClassProps::default(), "{class}");
    }
    let (style, _, list) = applied("p-4 shadow-md", 0);
    assert_eq!(style.layout.padding.top, len(16.0));
    assert_eq!(list.unsupported(), ["shadow-md".into()].as_slice());
    assert!(list.unknown().is_empty());
}

#[test]
fn unknown_names_are_separate_from_ignored() {
    let list = parse(&Theme::default(), "wat bg-brand bg-(--primary) antialiased");
    assert_eq!(
        list.unknown(),
        ["wat".into(), "bg-brand".into(), "bg-(--primary)".into()].as_slice()
    );
    assert!(list.unsupported().is_empty());
    let silent = parse(&Theme::default(), "antialiased");
    assert!(silent.unknown().is_empty() && silent.unsupported().is_empty());
}

#[test]
fn font_sans_sets_the_family_and_is_drawn() {
    let mut style = Style::default();
    style
        .set_key(StyleKey::FontFamily, StyleValue::Str("serif"))
        .unwrap();
    assert!(style.text.font_family.is_some());
    let theme = Theme::default();
    let list = parse(&theme, "font-sans");
    assert!(list.unknown().is_empty() && list.unsupported().is_empty());
    let mut props = ClassProps::default();
    let group = |_: &str| 0;
    list.apply(
        &Context {
            env: ClassEnvironment::default(),
            window: Size::new(px(1.0), px(1.0)),
            rtl: false,
            states: 0,
            position: 0,
            group: &group,
        },
        &theme,
        &mut style,
        &mut props,
        |_| {},
    );
    // The default sans stack is the system font: a family name, not a file.
    assert!(style.text.font_family.is_none());
}

#[test]
fn unknown_classes_are_named_once() {
    assert_eq!(
        super::class_tokens("  p-4  m-2 ").collect::<Vec<_>>(),
        ["p-4", "m-2"]
    );
    assert_eq!(
        super::unknown_classes_warning(["p-4", "m-2"]).as_deref(),
        Some("unknown classes: p-4 m-2")
    );
    assert_eq!(super::unknown_classes_warning(std::iter::empty()), None);
    assert_eq!(
        super::unsupported_classes_warning(["shadow-md", "dark:p-4"]).as_deref(),
        Some("unsupported classes: shadow-md dark:p-4")
    );
    assert_eq!(super::unsupported_classes_warning(std::iter::empty()), None);
}
