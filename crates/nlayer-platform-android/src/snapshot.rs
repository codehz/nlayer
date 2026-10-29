//! Turns the JSON tree the UI thread reports into a [`Scene`].
//!
//! The words are the views on screen, not the engine's mount batch. A boot
//! error is a failed scene, so the test step fails instead of waiting.

use serde::Deserialize;

use crate::scene::{ImageSnap, InputSnap, Kind, Node, Rect, Run, Scene, ScrollSnap, Span};

#[derive(Deserialize)]
struct Snap {
    width: f32,
    height: f32,
    #[serde(default)]
    error: String,
    #[serde(default)]
    nodes: Vec<Wire>,
}

#[derive(Deserialize)]
struct Wire {
    id: u32,
    parent: Option<u32>,
    kind: String,
    #[serde(default, rename = "testID")]
    test_id: Option<String>,
    #[serde(default)]
    text: Option<String>,
    frame: Frame,
    #[serde(default)]
    hidden: bool,
    #[serde(default = "opaque")]
    opacity: f32,
    #[serde(default)]
    pressable: bool,
    #[serde(default)]
    children: Vec<u32>,
    #[serde(default)]
    runs: Vec<WireRun>,
    #[serde(default)]
    spans: Vec<WireSpan>,
    #[serde(default)]
    input: Option<WireInput>,
    #[serde(default)]
    scroll: Option<WireScroll>,
    #[serde(default)]
    image: Option<WireImage>,
    #[serde(default)]
    transform: Vec<WireOp>,
}

#[derive(Deserialize)]
struct Frame {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Deserialize)]
struct WireRun {
    text: String,
    #[serde(rename = "fontSize")]
    font_size: f32,
    #[serde(rename = "fontWeight")]
    font_weight: u16,
    italic: bool,
    color: u32,
}

#[derive(Deserialize)]
struct WireSpan {
    start: u32,
    end: u32,
    #[serde(default, rename = "testID")]
    test_id: Option<String>,
    node: Option<u32>,
    text: String,
    frame: Frame,
}

#[derive(Deserialize)]
struct WireInput {
    value: String,
    placeholder: String,
    secure: bool,
    keyboard: String,
    disabled: bool,
    #[serde(default)]
    focused: bool,
}

#[derive(Deserialize)]
struct WireScroll {
    horizontal: bool,
    x: f32,
    y: f32,
    #[serde(rename = "contentW")]
    content_w: f32,
    #[serde(rename = "contentH")]
    content_h: f32,
}

#[derive(Deserialize)]
struct WireImage {
    src: String,
    fit: String,
}

#[derive(Deserialize)]
struct WireOp {
    kind: String,
    value: f32,
}

fn opaque() -> f32 {
    1.0
}

fn frame(frame: Frame) -> Rect {
    Rect {
        x: frame.x,
        y: frame.y,
        w: frame.w,
        h: frame.h,
    }
}

/// The scene `text` describes. A non-empty `error` fails the step.
pub(crate) fn scene_of(text: &str) -> Result<Scene, String> {
    let snap: Snap = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if !snap.error.is_empty() {
        return Err(snap.error);
    }
    let nodes = snap
        .nodes
        .into_iter()
        .map(|wire| {
            let kind = match wire.kind.as_str() {
                "text" => Kind::Text,
                "image" => Kind::Image,
                "scroll" => Kind::Scroll,
                "input" => Kind::Input,
                _ => Kind::View,
            };
            let mut transform = Vec::new();
            for op in wire.transform {
                let Some(kind) = nlayer_core::transform::TransformKind::parse(&op.kind) else {
                    continue;
                };
                transform.push(nlayer_core::transform::TransformOp {
                    kind,
                    value: op.value,
                });
            }
            Node {
                id: wire.id,
                parent: wire.parent,
                children: wire.children,
                kind,
                test_id: wire.test_id,
                text: wire.text,
                frame: frame(wire.frame),
                hidden: wire.hidden,
                opacity: wire.opacity,
                pressable: wire.pressable,
                input: wire.input.map(|input| InputSnap {
                    value: input.value,
                    placeholder: input.placeholder,
                    secure: input.secure,
                    keyboard: input.keyboard,
                    disabled: input.disabled,
                    focused: input.focused,
                }),
                runs: wire
                    .runs
                    .into_iter()
                    .map(|run| Run {
                        text: run.text,
                        font_size: run.font_size,
                        font_weight: run.font_weight,
                        italic: run.italic,
                        color: run.color,
                    })
                    .collect(),
                spans: wire
                    .spans
                    .into_iter()
                    .map(|span| Span {
                        start: span.start,
                        end: span.end,
                        test_id: span.test_id,
                        node: span.node,
                        text: span.text,
                        frame: frame(span.frame),
                    })
                    .collect(),
                scroll: wire.scroll.map(|scroll| ScrollSnap {
                    horizontal: scroll.horizontal,
                    x: scroll.x,
                    y: scroll.y,
                    content_w: scroll.content_w,
                    content_h: scroll.content_h,
                }),
                image: wire.image.map(|image| ImageSnap {
                    src: image.src,
                    fit: image.fit,
                }),
                transform,
            }
        })
        .collect();
    Ok(Scene::from_nodes(nodes, snap.width, snap.height))
}

#[cfg(test)]
mod tests {
    use super::scene_of;

    #[test]
    fn a_snapshot_names_a_press_and_keeps_the_authored_transform() {
        let scene = scene_of(
            r#"{
                "width": 100,
                "height": 80,
                "nodes": [
                    {
                        "id": 1,
                        "kind": "view",
                        "frame": {"x": 0, "y": 0, "w": 100, "h": 80},
                        "children": [2]
                    },
                    {
                        "id": 2,
                        "parent": 1,
                        "kind": "view",
                        "testID": "shift",
                        "frame": {"x": 10, "y": 20, "w": 40, "h": 16},
                        "pressable": true,
                        "transform": [{"kind": "translateX", "value": 12}, {"kind": "rotate", "value": 15}]
                    }
                ]
            }"#,
        )
        .expect("snapshot");
        assert_eq!(scene.press("shift").unwrap(), crate::scene::Press::View(2));
        let tree = scene.tree();
        let child = &tree["children"][0];
        assert_eq!(child["transform"][0]["translateX"], 12.0);
        assert_eq!(child["transform"][1]["rotate"], 15.0);
        assert_eq!(
            scene.point(crate::scene::Press::View(2)).unwrap(),
            (30.0, 28.0)
        );
    }

    #[test]
    fn a_boot_error_fails_the_step() {
        let error = match scene_of(r#"{"width":1,"height":1,"error":"no app.js"}"#) {
            Err(error) => error,
            Ok(_) => panic!("a boot error should fail"),
        };
        assert_eq!(error, "no app.js");
    }
}
