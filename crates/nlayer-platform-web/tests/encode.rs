//! A frame from the engine, and the classes a style becomes.

use std::{process::Command, sync::Arc, time::Duration};

use nlayer_core::{
    Color, MountBatch, MountItem, ViewKind,
    geometry::{Edges, Size, px},
    style::ViewStyle,
    text::ApproxTextEngine,
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
        || ApproxTextEngine,
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
    let runs = text[8].as_array().unwrap();
    let words: Vec<&str> = runs.iter().map(|run| run[0].as_str().unwrap()).collect();
    assert!(words.iter().any(|w| w.contains("span")), "{words:?}");
    assert!(
        text.get(10).and_then(|item| item.as_array()).is_some(),
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
