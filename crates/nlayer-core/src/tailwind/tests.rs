use taffy::{Dimension, Display, FlexDirection, LengthPercentage, LengthPercentageAuto};

use crate::{
    ResizeMode,
    effects::{GradientKind, GradientSize},
    geometry::{Size, deg, percentage, px},
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
        "blur-sm",
        "blur-md",
        "text-shadow-md",
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
    let (style, _, list) = applied("p-4 blur-sm", 0);
    assert_eq!(style.layout.padding.top, len(16.0));
    assert_eq!(list.unsupported(), ["blur-sm".into()].as_slice());
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
        super::unsupported_classes_warning(["blur-sm", "dark:p-4"]).as_deref(),
        Some("unsupported classes: blur-sm dark:p-4")
    );
    assert_eq!(super::unsupported_classes_warning(std::iter::empty()), None);
}

#[test]
fn shadows_rings_and_gradients_pass_the_gate() {
    let (s, _, list) = applied("shadow-md", 0);
    assert!(list.unsupported().is_empty() && list.unknown().is_empty());
    let layers = &s.view.effects.as_ref().expect("shadow-md").box_shadows;
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].offset_y, px(4.0));
    assert_eq!(layers[0].blur, px(6.0));
    assert_eq!(layers[0].spread, px(-1.0));
    assert!(!layers[0].inset);
    assert_eq!(s.layout, Style::default().layout);

    // A color needs a size's layers; an offset needs a ring. Together they
    // write inset shadow, inset ring, ring offset, ring, then the shadow.
    let (s, _, list) = applied(
        "inset-shadow-sm ring-2 ring-offset-2 inset-ring-2 shadow-md shadow-red-500",
        0,
    );
    assert!(
        list.unsupported().is_empty() && list.unknown().is_empty(),
        "{:?}",
        list.unsupported()
    );
    let layers = &s.view.effects.as_ref().unwrap().box_shadows;
    assert_eq!(layers.len(), 6);
    assert!(layers[0].inset && layers[0].blur == px(4.0));
    assert!(layers[1].inset && layers[1].spread == px(2.0));
    assert_eq!(
        (layers[2].spread, layers[2].color, layers[2].inset),
        (px(2.0), Color(0xffff_ffff), false)
    );
    assert_eq!((layers[3].spread, layers[3].color), (px(4.0), Color::BLACK));
    assert!(layers[4..].iter().all(|l| l.color == Color(0xfb2c_36ff)));
    assert_eq!(layers[4].offset_y, px(4.0));

    let (s, _, list) = applied("shadow-md shadow-red-500/20", 0);
    assert!(list.unsupported().is_empty() && list.unknown().is_empty());
    let layers = &s.view.effects.as_ref().unwrap().box_shadows;
    assert!(
        layers.iter().all(|l| l.color == Color(0xfb2c_3633)),
        "{layers:?}"
    );

    for class in [
        "inset-shadow-sm",
        "ring-2",
        "ring-offset-2",
        "inset-ring-2",
        "shadow-red-500",
        "shadow-red-500/20",
    ] {
        let (s, _, list) = applied(class, 0);
        assert!(
            list.unsupported().is_empty() && list.unknown().is_empty(),
            "{class}"
        );
        if matches!(class, "inset-shadow-sm" | "ring-2" | "inset-ring-2") {
            assert!(
                s.view
                    .effects
                    .as_ref()
                    .is_some_and(|e| !e.box_shadows.is_empty()),
                "{class}"
            );
        }
    }

    let (s, _, list) = applied("bg-linear-to-r from-red-500 via-green-500 to-blue-500", 0);
    assert!(list.unsupported().is_empty() && list.unknown().is_empty());
    let images = &s.view.effects.as_ref().unwrap().background_images;
    assert_eq!(images.len(), 1);
    let g = &images[0];
    assert_eq!(g.kind, GradientKind::Linear { angle: deg(90.0) });
    let (red, green, blue) = (Color(0xfb2c_36ff), Color(0x00c950ff), Color(0x2b7fffff));
    let fraction = |color: Color| {
        let stop = g
            .stops
            .iter()
            .find(|s| s.color == color)
            .unwrap_or_else(|| panic!("{color:?} missing"));
        match stop.position {
            Some(crate::geometry::DefiniteLength::Fraction(p)) => p.0,
            other => panic!("{other:?}"),
        }
    };
    assert!((fraction(red) - 0.0).abs() < 1e-6);
    assert!((fraction(green) - 0.5).abs() < 1e-6);
    assert!((fraction(blue) - 1.0).abs() < 1e-6);
    // The utility's three stops. Oklab sampling may insert colors between them.
    assert_eq!(
        g.stops
            .iter()
            .filter(|s| s.color == red || s.color == green || s.color == blue)
            .count(),
        3
    );

    for (class, kind) in [
        (
            "bg-radial",
            GradientKind::Radial {
                circle: false,
                size: GradientSize::FarthestCorner,
                x: percentage(0.5),
                y: percentage(0.5),
            },
        ),
        (
            "bg-conic",
            GradientKind::Conic {
                from: deg(0.0),
                x: percentage(0.5),
                y: percentage(0.5),
            },
        ),
    ] {
        let (s, _, list) = applied(class, 0);
        assert!(
            list.unsupported().is_empty() && list.unknown().is_empty(),
            "{class}"
        );
        let images = &s.view.effects.as_ref().expect(class).background_images;
        assert_eq!(images.len(), 1, "{class}");
        assert_eq!(images[0].kind, kind, "{class}");
    }
    let (s, _, list) = applied("bg-linear-to-r from-red-500 bg-none", 0);
    assert!(list.unsupported().is_empty() && list.unknown().is_empty());
    assert!(
        s.view
            .effects
            .as_ref()
            .is_none_or(|e| e.background_images.is_empty())
    );

    for class in ["p", "shadow-md", "bg-linear-to-r"] {
        let list = parse(&Theme::default(), class);
        assert!(
            list.unsupported().is_empty(),
            "{class}: {:?}",
            list.unsupported()
        );
    }
    for class in ["text-shadow-md", "blur-sm", "transition"] {
        let list = parse(&Theme::default(), class);
        assert_eq!(list.unsupported(), [class.into()].as_slice(), "{class}");
        assert!(list.unknown().is_empty(), "{class}");
    }

    // Two tokens: the shadow applies, the filter still warns. One utility
    // that mixes an allowed declaration with a rejected one is dropped whole
    // (no generated class does that; the rule is the gate's).
    let (s, _, list) = applied("shadow-md blur-sm", 0);
    assert_eq!(list.unsupported(), ["blur-sm".into()].as_slice());
    assert!(!s.view.effects.as_ref().unwrap().box_shadows.is_empty());
    let shadow =
        super::utilities::Decl::Part(super::utilities::Part::Shadow(std::sync::Arc::from([])));
    let text =
        super::utilities::Decl::Part(super::utilities::Part::TextShadow(std::sync::Arc::from([])));
    let rule = |decls| super::Rule {
        cond: super::Condition::default(),
        decls,
        rank: super::Rank::default(),
        important: false,
        theme: false,
    };
    assert!(super::effect_allowed(&rule(vec![shadow.clone()])));
    assert!(!super::effect_allowed(&rule(vec![shadow, text])));
}

#[test]
fn hover_shadows_rewrite_the_main_style() {
    let (s, _, list) = applied("shadow-md hover:shadow-none", 0);
    assert!(list.unsupported().is_empty() && list.unknown().is_empty());
    assert_eq!(s.view.effects.as_ref().unwrap().box_shadows.len(), 2);
    let (s, _, list) = applied("shadow-md hover:shadow-none", state::HOVERED);
    assert!(list.unsupported().is_empty());
    assert!(
        s.view
            .effects
            .as_ref()
            .is_none_or(|e| e.box_shadows.is_empty())
    );
    let (s, _, list) = applied("hover:shadow-md", 0);
    assert!(list.unsupported().is_empty());
    assert!(s.view.effects.is_none());
    let (s, _, list) = applied("hover:shadow-md", state::HOVERED);
    assert!(list.unsupported().is_empty() && list.unknown().is_empty());
    assert_eq!(s.view.effects.as_ref().unwrap().box_shadows.len(), 2);
    assert_eq!(
        s.view.effects.as_ref().unwrap().box_shadows[0].offset_y,
        px(4.0)
    );
}

#[test]
fn pointer_events_classes_set_the_style() {
    let (s, mut props, list) = applied("pointer-events-none", 0);
    assert!(list.unsupported().is_empty(), "{:?}", list.unsupported());
    s.node_props(&mut props);
    assert_eq!(props.pointer_events, Some(false));
    let (s, mut props, list) = applied("pointer-events-auto hover:pointer-events-none", 0);
    assert!(list.unsupported().is_empty(), "{:?}", list.unsupported());
    s.node_props(&mut props);
    assert_eq!(props.pointer_events, Some(true));
    let (s, mut props, list) = applied(
        "pointer-events-auto hover:pointer-events-none",
        state::HOVERED,
    );
    assert!(list.unsupported().is_empty(), "{:?}", list.unsupported());
    s.node_props(&mut props);
    assert_eq!(props.pointer_events, Some(false));
}
