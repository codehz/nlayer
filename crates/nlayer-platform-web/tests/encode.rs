//! A frame from the engine, and the classes a style becomes.

use std::{process::Command, sync::Arc, time::Duration};

use nlayer_core::{
    Color, MountBatch, MountItem, ViewKind,
    effects::{BoxShadow, ColorStop, Effects, Filter, Gradient, GradientKind, GradientSize},
    geometry::{DefiniteLength, Edges, Size, deg, percentage, px},
    style::ViewStyle,
    transform::{Transform, TransformKind},
};
use nlayer_engine::{Engine, EngineConfig, MountSink};
use nlayer_platform_web::{Encoder, MOUNTER_JS};
use serde_json::Value;

struct Sink {
    tx: crossbeam_channel::Sender<MountBatch>,
}

impl MountSink for Sink {
    fn submit(&self, batch: MountBatch) {
        let _ = self.tx.send(batch);
    }
}

fn items(batch: &str) -> Vec<Value> {
    serde_json::from_str(batch).expect("a batch is JSON")
}

#[test]
fn a_style_is_one_class_and_its_border_is_separate() {
    let mut style = ViewStyle {
        background_color: Color::rgba(0, 0, 255, 255),
        opacity: 0.5,
        border_radius: px(4.0),
        ..ViewStyle::default()
    };
    style.borders.widths = Edges::all(px(1.0));
    let mut transform = Transform::default();
    assert!(transform.push(TransformKind::Rotate, 45.0).is_some());
    style.transform = transform;
    let batch = MountBatch {
        items: vec![
            MountItem::Create {
                id: 2,
                kind: ViewKind::View,
            },
            MountItem::ViewStyle {
                id: 2,
                style: Box::new(style.clone()),
            },
            MountItem::ViewStyle {
                id: 2,
                style: Box::new(style),
            },
        ],
    };
    let json = Encoder::new().encode(&batch);
    let css: Vec<_> = items(&json)
        .into_iter()
        .filter(|item| item[0] == "css")
        .collect();
    assert_eq!(css.len(), 2, "the second item reuses the classes: {json}");
    let view = css[0][2].as_str().unwrap();
    assert!(view.contains("background-color:#0000ff"), "{view}");
    assert!(view.contains("opacity:0.5"), "{view}");
    assert!(view.contains("transform:rotate(45deg);"), "{view}");
    assert!(view.contains("border-radius:4px 4px 4px 4px;"), "{view}");
    assert!(!view.contains("box-shadow"), "{view}");
    assert!(
        css[1][2].as_str().unwrap().contains("border-top-width:1px"),
        "{json}"
    );
    let styled: Vec<_> = items(&json)
        .into_iter()
        .filter(|item| item[0] == "s")
        .collect();
    assert_eq!(styled.len(), 2, "{json}");
    assert_eq!(
        styled[0].as_array().unwrap().len(),
        5,
        "view class, border class, content layer: {json}"
    );
    assert!(styled[0][4].is_null(), "no outer shadow to split: {json}");
    assert!(styled[1][4].is_null(), "{json}");
    assert_eq!(styled[0][2], styled[1][2]);
    assert_eq!(styled[0][3], styled[1][3]);
}

#[test]
fn a_hidden_view_is_laid_out_but_not_drawn() {
    let style = ViewStyle {
        hidden: true,
        ..ViewStyle::default()
    };
    let json = Encoder::new().encode(&MountBatch {
        items: vec![MountItem::ViewStyle {
            id: 2,
            style: Box::new(style),
        }],
    });
    let css = items(&json)
        .into_iter()
        .find(|item| item[0] == "css")
        .expect("a style class");
    assert!(
        css[2].as_str().unwrap().contains("visibility:hidden;"),
        "{json}"
    );
}

#[test]
fn a_view_style_carries_transform_steps() {
    let mut style = ViewStyle::default();
    let mut transform = Transform::default();
    assert!(transform.push(TransformKind::TranslateX, 12.0).is_some());
    assert!(transform.push(TransformKind::Rotate, 15.0).is_some());
    assert!(transform.push(TransformKind::Scale, 1.25).is_some());
    style.transform = transform;
    let set = Encoder::new().encode(&MountBatch {
        items: vec![MountItem::ViewStyle {
            id: 2,
            style: Box::new(style),
        }],
    });
    let steps = items(&set)
        .into_iter()
        .find(|item| item[0] == "xf")
        .expect("transform steps");
    assert_eq!(steps[1], 2);
    assert_eq!(
        steps[2],
        serde_json::json!([{"translateX": 12}, {"rotate": 15}, {"scale": 1.25}])
    );

    let cleared = Encoder::new().encode(&MountBatch {
        items: vec![MountItem::ViewStyle {
            id: 2,
            style: Box::new(ViewStyle::default()),
        }],
    });
    let empty = items(&cleared)
        .into_iter()
        .find(|item| item[0] == "xf")
        .expect("a cleared transform");
    assert_eq!(empty[2], serde_json::json!([]));
}

#[test]
fn a_state_style_is_a_rule_the_page_applies() {
    let mut view = ViewStyle {
        background_color: Color::rgba(255, 0, 0, 255),
        ..ViewStyle::default()
    };
    view.borders.widths = Edges::all(px(2.0));
    let text = nlayer_core::style::TextStyle {
        color: Some(Color::rgba(0, 255, 0, 255)),
        ..nlayer_core::style::TextStyle::default()
    };
    let batch = MountBatch {
        items: vec![MountItem::StateStyle {
            id: 2,
            interaction: nlayer_core::Interaction::Hover,
            view: Box::new(view),
            text,
        }],
    };
    let json = Encoder::new().encode(&batch);
    let parsed = items(&json);
    let rule = parsed.iter().find(|item| item[0] == "rule").unwrap();
    assert_eq!(rule[1], "i2h");
    assert_eq!(rule[2], 3);
    assert_eq!(rule[3], ":hover");
    assert!(
        rule[5]
            .as_str()
            .unwrap()
            .contains("background-color:#ff0000")
    );
    let applied = parsed.iter().find(|item| item[0] == "st").unwrap();
    assert_eq!(applied[1], 2);
    assert_eq!(applied[2], "hover");
    assert_eq!(applied[3], "i2h");
}

fn shadow(x: f32, y: f32, blur: f32, spread: f32, color: Color, inset: bool) -> BoxShadow {
    BoxShadow {
        offset_x: px(x),
        offset_y: px(y),
        blur: px(blur),
        spread: px(spread),
        color,
        inset,
    }
}

fn stop(color: Color, position: Option<DefiniteLength>) -> ColorStop {
    ColorStop { color, position }
}

fn red_blue() -> Vec<ColorStop> {
    vec![
        stop(Color::rgba(255, 0, 0, 255), Some(percentage(0.0).into())),
        stop(Color::rgba(0, 0, 255, 255), None),
    ]
}

fn gradient(kind: GradientKind, stops: Vec<ColorStop>, repeating: bool) -> Gradient {
    Gradient {
        kind,
        stops,
        repeating,
    }
}

fn with_effects(shadows: Vec<BoxShadow>, images: Vec<Gradient>) -> ViewStyle {
    ViewStyle {
        effects: Some(Arc::new(Effects {
            box_shadows: shadows,
            background_images: images,
            ..Effects::default()
        })),
        ..ViewStyle::default()
    }
}

fn encode_one(style: ViewStyle) -> Vec<Value> {
    items(&Encoder::new().encode(&MountBatch {
        items: vec![MountItem::ViewStyle {
            id: 2,
            style: Box::new(style),
        }],
    }))
}

fn class_text(batch: &[Value], name: &str) -> String {
    batch
        .iter()
        .find(|item| item[0] == "css" && item[1] == name)
        .unwrap_or_else(|| panic!("missing class {name}: {batch:?}"))[2]
        .as_str()
        .unwrap()
        .to_owned()
}

fn styled(batch: &[Value]) -> (String, Option<String>) {
    let item = batch.iter().find(|item| item[0] == "s").expect("a style");
    assert_eq!(
        item.as_array().unwrap().len(),
        5,
        "view, border, content layer: {item}"
    );
    let view = class_text(batch, item[2].as_str().unwrap());
    let layer = item[4].as_str().map(|name| class_text(batch, name));
    (view, layer)
}

#[test]
fn shadows_and_gradients_are_css_and_empty_lists_emit_nothing() {
    let outer = shadow(1.0, 2.0, 3.0, 4.0, Color::BLACK, false);
    let inset = shadow(0.0, 1.0, 2.0, -1.0, Color::rgba(255, 0, 0, 255), true);
    let linear = gradient(
        GradientKind::Linear { angle: deg(180.0) },
        red_blue(),
        false,
    );
    let corner = gradient(
        GradientKind::LinearCorner {
            right: true,
            bottom: true,
        },
        red_blue(),
        false,
    );
    let radial = gradient(
        GradientKind::Radial {
            circle: true,
            size: GradientSize::ClosestSide,
            x: percentage(0.25),
            y: percentage(0.75),
        },
        red_blue(),
        false,
    );
    let conic = gradient(
        GradientKind::Conic {
            from: deg(90.0),
            x: percentage(0.5),
            y: percentage(0.5),
        },
        red_blue(),
        true,
    );
    let lengths = gradient(
        GradientKind::Linear { angle: deg(90.0) },
        vec![
            stop(Color::rgba(255, 0, 0, 255), Some(px(10.0).into())),
            stop(Color::rgba(0, 0, 255, 255), Some(percentage(0.5).into())),
            stop(Color::rgba(0, 255, 0, 255), None),
        ],
        true,
    );
    let (view, layer) = styled(&encode_one(with_effects(
        vec![outer, inset],
        vec![linear, corner, radial, conic, lengths],
    )));
    assert!(layer.is_none(), "overflow is not hidden: {view}");
    assert!(
        view.contains("box-shadow:1px 2px 3px 4px #000000,inset 0px 1px 2px -1px #ff0000;"),
        "{view}"
    );
    assert!(
        view.contains(
            "background-image:linear-gradient(180deg, #ff0000 0%, #0000ff 100%),\
             linear-gradient(to bottom right, #ff0000 0%, #0000ff 100%),\
             radial-gradient(circle closest-side at 25% 75%, #ff0000 0%, #0000ff 100%),\
             repeating-conic-gradient(from 90deg at 50% 50%, #ff0000 0%, #0000ff 100%),\
             repeating-linear-gradient(90deg, #ff0000 10px, #0000ff 50%, #00ff00);"
        ),
        "{view}"
    );
    assert!(!view.contains("filter"), "{view}");
    assert!(!view.contains("mask-image"), "{view}");
    assert!(!view.contains("outline"), "{view}");

    let mut empty = ViewStyle {
        background_color: Color::rgba(1, 2, 3, 255),
        effects: Some(Arc::new(Effects {
            filters: vec![Filter::Blur(px(4.0))],
            ..Effects::default()
        })),
        ..ViewStyle::default()
    };
    let (cleared, _) = styled(&encode_one(empty.clone()));
    assert!(cleared.contains("background-color:#010203"), "{cleared}");
    assert!(!cleared.contains("box-shadow"), "{cleared}");
    assert!(!cleared.contains("background-image"), "{cleared}");
    assert!(!cleared.contains("filter"), "{cleared}");
    assert!(!cleared.contains("none"), "{cleared}");

    empty.effects = None;
    let (absent, _) = styled(&encode_one(empty));
    assert!(!absent.contains("box-shadow"), "{absent}");
    assert!(!absent.contains("background-image"), "{absent}");
}

#[test]
fn an_outer_shadow_moves_overflow_clip_onto_the_content_layer() {
    let outer = shadow(0.0, 6.0, 18.0, 0.0, Color::rgba(0, 0, 0, 128), false);
    let mut split = with_effects(vec![outer], vec![]);
    split.overflow_hidden = true;
    let (view, layer) = styled(&encode_one(split));
    assert!(
        view.contains("box-shadow:0px 6px 18px 0px rgba(0,0,0,0.502);"),
        "{view}"
    );
    assert!(!view.contains("overflow"), "{view}");
    assert_eq!(layer.as_deref(), Some("overflow:clip;"));

    let mut inset_only = with_effects(vec![shadow(0.0, 1.0, 2.0, 0.0, Color::BLACK, true)], vec![]);
    inset_only.overflow_hidden = true;
    let (view, layer) = styled(&encode_one(inset_only));
    assert!(view.contains("overflow:clip;"), "{view}");
    assert!(view.contains("inset "), "{view}");
    assert!(layer.is_none(), "{view}");

    let mut bare = ViewStyle {
        overflow_hidden: true,
        ..ViewStyle::default()
    };
    let (view, layer) = styled(&encode_one(bare.clone()));
    assert!(view.contains("overflow:clip;"), "{view}");
    assert!(!view.contains("box-shadow"), "{view}");
    assert!(layer.is_none());

    bare.effects = Some(Arc::new(Effects {
        box_shadows: vec![shadow(4.0, 0.0, 0.0, 0.0, Color::TRANSPARENT, false)],
        ..Effects::default()
    }));
    let (view, layer) = styled(&encode_one(bare));
    assert!(
        view.contains("overflow:clip;"),
        "a transparent outer shadow does not split: {view}"
    );
    assert!(layer.is_none(), "{view}");

    let mut encoder = Encoder::new();
    let _ = encoder.encode(&MountBatch {
        items: vec![MountItem::Create {
            id: 4,
            kind: ViewKind::Scroll,
        }],
    });
    let mut scrolling = with_effects(
        vec![shadow(0.0, 6.0, 18.0, 0.0, Color::BLACK, false)],
        vec![],
    );
    scrolling.overflow_hidden = true;
    let later = items(&encoder.encode(&MountBatch {
        items: vec![MountItem::ViewStyle {
            id: 4,
            style: Box::new(scrolling),
        }],
    }));
    let (view, layer) = styled(&later);
    assert!(
        view.contains("overflow:clip;"),
        "a scroll keeps the clip on itself: {view}"
    );
    assert!(view.contains("box-shadow:"), "{view}");
    assert!(layer.is_none(), "{view}");
}

#[test]
fn a_state_writes_a_shadow_or_gradient_only_when_its_list_is_set() {
    let set = with_effects(
        vec![shadow(0.0, 2.0, 4.0, 0.0, Color::BLACK, false)],
        vec![gradient(
            GradientKind::Linear { angle: deg(0.0) },
            red_blue(),
            false,
        )],
    );
    let json = Encoder::new().encode(&MountBatch {
        items: vec![MountItem::StateStyle {
            id: 2,
            interaction: nlayer_core::Interaction::Active,
            view: Box::new(set),
            text: nlayer_core::style::TextStyle::default(),
        }],
    });
    let rule = items(&json)
        .into_iter()
        .find(|item| item[0] == "rule" && item[4] == "")
        .expect("the view rule");
    let text = rule[5].as_str().unwrap();
    assert!(
        text.contains("box-shadow:0px 2px 4px 0px #000000;"),
        "{text}"
    );
    assert!(
        text.contains("background-image:linear-gradient(0deg, #ff0000 0%, #0000ff 100%);"),
        "{text}"
    );
    assert!(!text.contains("none"), "{text}");

    let cleared = Encoder::new().encode(&MountBatch {
        items: vec![MountItem::StateStyle {
            id: 2,
            interaction: nlayer_core::Interaction::Hover,
            view: Box::new(ViewStyle {
                effects: Some(Arc::new(Effects::default())),
                ..ViewStyle::default()
            }),
            text: nlayer_core::style::TextStyle::default(),
        }],
    });
    let rule = items(&cleared)
        .into_iter()
        .find(|item| item[0] == "rule" && item[4] == "")
        .expect("the view rule");
    let text = rule[5].as_str().unwrap();
    assert!(text.is_empty(), "an empty list does not emit none: {text}");
}

const APP: &str = r#"
import { mount } from "@nlayer/runtime";
import { Image, Input, Scroll, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ paddingTop: "env(safe-area-inset-top)" }}>
            <View style={{ display: "grid", gridTemplateColumns: "1fr 1fr", height: 80 }}>
                <View style={{ backgroundColor: "red", height: 80 }} />
                <View
                    style={{ backgroundColor: "blue", height: 80, transform: [{ rotate: "45deg" }] }}
                    onPress={() => {}}
                />
            </View>
            <Text>
                Hello <Text style={{ color: "red", fontWeight: "700", fontSize: 20, fontStyle: "italic" }}>span</Text>
            </Text>
            <Image src="https://example.com/a.png" style={{ width: 32, height: 32 }} />
            <Scroll style={{ width: 120, height: 40 }}>
                <View style={{ height: 200 }} />
            </Scroll>
            <Scroll horizontal style={{ width: 80, height: 40 }}>
                <View style={{ width: 300, height: 40 }} />
            </Scroll>
            <Input value="hi" placeholder="name" onChange={() => {}} />
        </View>
    );
}
mount(App);
"#;

fn frame(rx: &crossbeam_channel::Receiver<MountBatch>) -> MountBatch {
    rx.recv_timeout(Duration::from_secs(5)).expect("a frame")
}

/// The id whose style class paints `color` (`#ff0000`).
fn painted(batch: &[Value], color: &str) -> u64 {
    let class = batch.iter().find_map(|item| {
        (item[0] == "css" && item[2].as_str().unwrap().contains(color))
            .then(|| item[1].as_str().unwrap().to_owned())
    });
    let class = class.expect(color);
    batch
        .iter()
        .find_map(|item| {
            (item[0] == "s" && item[2].as_str() == Some(class.as_str()))
                .then(|| item[1].as_u64().unwrap())
        })
        .expect("a styled view")
}

fn frame_y(batch: &[Value], id: u64) -> f64 {
    batch
        .iter()
        .rev()
        .find_map(|item| {
            (item[0] == "f" && item[1].as_u64() == Some(id)).then(|| item[3].as_f64().unwrap())
        })
        .expect("a frame")
}

#[test]
fn an_app_frame_encodes_the_v1_surface() {
    let code = nlayer_bundler::compile_module("App.tsx", APP).expect("the app compiles");
    let (tx, rx) = crossbeam_channel::unbounded();
    let engine = Engine::start(
        EngineConfig::new(code, Size::new(px(390.0), px(800.0))),
        nlayer_platform_web::WebTextEngine::default,
        Arc::new(Sink { tx }),
    )
    .expect("the engine starts");
    let mut encoder = Encoder::new();
    let first = items(&encoder.encode(&frame(&rx)));
    let red = painted(&first, "background-color:#ff0000");
    let blue = painted(&first, "background-color:#0000ff");
    let red_frame = first
        .iter()
        .rev()
        .find(|item| item[0] == "f" && item[1].as_u64() == Some(red))
        .unwrap();
    let blue_frame = first
        .iter()
        .rev()
        .find(|item| item[0] == "f" && item[1].as_u64() == Some(blue))
        .unwrap();
    assert!(
        (red_frame[2].as_f64().unwrap() - blue_frame[2].as_f64().unwrap()).abs() > 50.0,
        "the grid's two columns: {red_frame} {blue_frame}"
    );
    assert!(
        first
            .iter()
            .any(|item| item[0] == "css" && item[2].as_str().unwrap().contains("rotate(45deg)")),
        "the rotated cell"
    );
    let text = first
        .iter()
        .find(|item| item[0] == "t")
        .expect("a text block");
    let runs = text[6].as_array().unwrap();
    let words: Vec<&str> = runs.iter().map(|run| run[2].as_str().unwrap()).collect();
    assert!(words.iter().any(|w| w.contains("span")), "{words:?}");
    assert!(
        text.get(9).and_then(|item| item.as_array()).is_some(),
        "span press ranges"
    );
    assert!(
        first.iter().any(|item| {
            item[0] == "css" && {
                let css = item[2].as_str().unwrap();
                css.contains("italic")
                    && css.contains("700")
                    && css.contains("20px")
                    && css.contains("#ff0000")
            }
        }),
        "the nested span"
    );
    assert!(
        first
            .iter()
            .any(|item| item[0] == "img" && item[2] == "https://example.com/a.png")
    );
    assert!(first.iter().any(|item| item[0] == "sc" && item[2] == false));
    assert!(first.iter().any(|item| item[0] == "sc" && item[2] == true));
    assert!(
        first
            .iter()
            .any(|item| item[0] == "scs" && item[3].as_f64().unwrap() >= 200.0)
    );
    assert!(
        first
            .iter()
            .any(|item| item[0] == "scs" && item[2].as_f64().unwrap() >= 300.0)
    );
    assert!(
        first.iter().any(
            |item| item[0] == "e" && item[2].as_array().unwrap().iter().any(|e| e == "onPress")
        )
    );
    assert!(
        first
            .iter()
            .any(|item| item[0] == "in" && item[3]["placeholder"] == "name")
    );
    assert!(first.iter().any(|item| item[0] == "iv" && item[2] == "hi"));
    let cell_parent = |batch: &[Value]| {
        batch.iter().rev().find_map(|item| {
            (item[0] == "i" && item[2].as_u64() == Some(red)).then(|| item[1].as_u64().unwrap())
        })
    };
    let padded = cell_parent(&first).expect("the grid");
    assert_eq!(frame_y(&first, padded), 0.0, "no inset yet");

    engine.control().set_safe_area(Edges {
        top: px(47.0),
        ..Edges::default()
    });
    let next = items(&encoder.encode(&frame(&rx)));
    let padded = cell_parent(&next).unwrap_or(padded);
    assert_eq!(frame_y(&next, padded), 47.0, "env(safe-area-inset-top)");
    engine.stop();
}

#[test]
fn a_text_carries_its_selection_color_and_hrefs() {
    use nlayer_core::{
        geometry::{Bounds, Point},
        text::{AttributedText, ResolvedTextStyle, TextHref, TextLayout},
    };
    let mut text = AttributedText {
        selectable: Some(true),
        selection_color: Some(Color::rgba(255, 0, 0, 255)),
        ..AttributedText::default()
    };
    text.push("Terms", ResolvedTextStyle::default());
    text.hrefs.push(TextHref {
        start: 0,
        end: 5,
        node: 7,
        url: "https://example.com/terms".into(),
    });
    let content = Bounds::new(Point::new(px(4.0), px(6.0)), Size::new(px(30.0), px(20.0)));
    let json = Encoder::new().encode(&MountBatch {
        items: vec![MountItem::Text {
            id: 6,
            text: Arc::new(text),
            layout: TextLayout(Arc::new(())),
            content,
        }],
    });
    let text = items(&json)
        .into_iter()
        .find(|item| item[0] == "t")
        .expect("a text block");
    assert_eq!(text[2], 4.0, "the content origin stays inside the view");
    assert_eq!(text[3], 6.0);
    assert_eq!(text[10], Value::Bool(true), "userSelect: text");
    assert_eq!(text[11], "#ff0000", "selectionColor");
    assert_eq!(text[12], serde_json::json!([[0, 5, 7]]), "href ranges");
}

#[test]
fn a_selectable_view_and_an_open_url_are_ops() {
    let json = Encoder::new().encode(&MountBatch {
        items: vec![
            MountItem::Selectable {
                id: 4,
                selectable: true,
            },
            MountItem::Command {
                id: 1,
                command: nlayer_core::Command::OpenUrl {
                    url: "https://example.com".to_owned(),
                },
            },
        ],
    });
    let batch = items(&json);
    assert_eq!(batch[0], serde_json::json!(["sel", 4, true]));
    assert_eq!(
        batch[1],
        serde_json::json!(["cmd", 1, "openUrl", "https://example.com"])
    );
}

#[test]
fn the_mounter_applies_a_batch_in_node() {
    assert!(MOUNTER_JS.contains("export class Player"));
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/web/nlayer.test.mjs");
    let host = concat!(env!("CARGO_MANIFEST_DIR"), "/web/host.test.mjs");
    let status = Command::new("node")
        .args(["--test", script, host])
        .status()
        .expect("node runs the mounter test");
    assert!(status.success(), "node --test {script}");
}
