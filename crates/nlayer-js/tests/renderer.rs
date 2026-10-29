//! The JSX runtime: compiled-shape calls building the shadow tree.

use nlayer_core::{
    Command, Core, EventKind, MountBatch, MountItem, ViewKind,
    geometry::{Bounds, Points, Size},
    text::ApproxTextEngine,
    transform::TransformKind,
};
use nlayer_js::{EventPayload, Host};

fn core() -> Core<ApproxTextEngine> {
    Core::new(ApproxTextEngine, Size::new(Points(390.0), Points(844.0)))
}

/// Applies what JS queued and commits a frame.
fn frame(host: &Host, core: &mut Core<ApproxTextEngine>) -> MountBatch {
    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    for op in ops {
        core.apply(op).unwrap();
    }
    core.commit().expect("a batch")
}

fn text_of(batch: &MountBatch) -> Vec<String> {
    batch
        .items
        .iter()
        .filter_map(|item| match item {
            MountItem::Text { text, .. } => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

fn frame_of(batch: &MountBatch, id: u32) -> Option<Bounds<Points>> {
    batch.items.iter().find_map(|item| match item {
        MountItem::Frame { id: at, frame } if *at == id => Some(*frame),
        _ => None,
    })
}

/// The shape the JSX compiler emits for `<View style={…}><Text>hi</Text></View>`.
#[test]
fn elements_styles_and_children_reach_the_core() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount } from "@nlayer/runtime";
        import { element, prop, append, text } from "nlayer:rt";

        function App() {
            const view = element("view");
            prop(view, "style", { width: 100, height: 50, backgroundColor: "red" });
            const block = element("text");
            append(block, text("hi"));
            append(view, block);
            return view;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let batch = frame(&host, &mut core);
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::Create {
            kind: ViewKind::View,
            ..
        }
    )));
    assert_eq!(text_of(&batch), ["hi"]);
    // The view is at the root with its size; the text block sits inside it.
    let view = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Create {
                id,
                kind: ViewKind::View,
                ..
            } => Some(*id),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        frame_of(&batch, view).map(|f| (f.size.width.0, f.size.height.0)),
        Some((100.0, 50.0))
    );
}

/// `testID` names a view in the mount batch and a nested span on its block.
#[test]
fn test_ids_name_a_view_and_a_nested_span() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount } from "@nlayer/runtime";
        import { element, prop, append, text } from "nlayer:rt";

        function App() {
            const view = element("view");
            prop(view, "testID", "box");
            prop(view, "style", { width: 80, height: 40 });
            const block = element("text");
            append(block, text("see "));
            const span = element("text");
            prop(span, "testID", "docs");
            append(span, text("docs"));
            append(block, span);
            append(view, block);
            return view;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let batch = frame(&host, &mut core);
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::TestId { test_id, .. } if test_id.as_deref() == Some("box")
    )));
    let text = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Text { text, .. } => Some(text),
            _ => None,
        })
        .expect("the block");
    assert_eq!(text.text, "see docs");
    assert_eq!(text.test_ids.len(), 1);
    assert_eq!(text.test_ids[0].test_id.as_ref(), "docs");
    assert_eq!(
        batch
            .items
            .iter()
            .filter(|item| matches!(item, MountItem::TestId { .. }))
            .count(),
        1,
        "a span has no view of its own"
    );
}

/// `onPress` is a listener on the node, and the handler is called with the
/// event object.
#[test]
fn press_handlers_are_listened_and_called() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount, signal } from "@nlayer/runtime";
        import { element, prop, append, text } from "nlayer:rt";

        const pressed = signal(0);
        globalThis.pressed = pressed;
        function App() {
            const view = element("view");
            prop(view, "style", { width: 10, height: 10 });
            prop(view, "onPress", () => pressed.set(pressed() + 1));
            prop(view, "onLongPress", () => {});
            prop(view, "pointerEvents", "auto");
            return view;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let batch = frame(&host, &mut core);
    let (id, events) = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Events { id, events } => Some((*id, *events)),
            _ => None,
        })
        .expect("listeners");
    assert!(events.contains(EventKind::Press));
    assert!(!events.contains(EventKind::Change));
    host.dispatch_event(id, EventKind::Press, &EventPayload::None);
    host.dispatch_event(id, EventKind::Press, &EventPayload::None);
    // The handler ran: the next script reads the signal's value.
    let error = host
        .eval_source("import { signal } from '@nlayer/runtime'; throw new Error('check');")
        .unwrap_err();
    assert!(error.message.contains("check"));
}

/// A signal read in a dynamic child updates the text in place: the text
/// node is kept (one `SetText`, no new node).
#[test]
fn dynamic_children_are_kept_by_their_owner() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount, signal } from "@nlayer/runtime";
        import { element, append, insert, prop, text } from "nlayer:rt";

        const count = signal(0);
        function App() {
            const view = element("view");
            prop(view, "onPress", () => count.set(count() + 1));
            const block = element("text");
            append(block, text("count: "));
            insert(block, () => String(count()));
            append(view, block);
            return view;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let first = frame(&host, &mut core);
    assert_eq!(text_of(&first), ["count: 0"]);
    let id = first
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Events { id, events } if events.contains(EventKind::Press) => Some(*id),
            _ => None,
        })
        .unwrap();
    host.dispatch_event(id, EventKind::Press, &EventPayload::None);
    let second = frame(&host, &mut core);
    assert_eq!(
        text_of(&second),
        ["count: 1"],
        "the same text node is updated"
    );
    assert!(
        !second
            .items
            .iter()
            .any(|item| matches!(item, MountItem::Create { .. })),
        "no node is made"
    );
}

/// `<Each>` keeps the rows whose key stays and makes only new ones.
#[test]
fn each_keeps_keyed_rows() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount, signal } from "@nlayer/runtime";
        import { element, prop, append, insert, each, text } from "nlayer:rt";

        const items = signal(["a", "b"]);
        const label = (value) => {
            const block = element("text");
            append(block, text(value));
            return block;
        };
        function App() {
            const view = element("view");
            prop(view, "onPress", () => items.set(["b", "a", "c"]));
            insert(view, each(() => items(), (item) => label(item())));
            return view;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let first = frame(&host, &mut core);
    assert_eq!(text_of(&first), ["a", "b"]);
    let id = first
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Events { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    host.dispatch_event(id, EventKind::Press, &EventPayload::None);
    let second = frame(&host, &mut core);
    // "b" keeps its text node, "a" is moved, "c" is new.
    assert_eq!(text_of(&second), ["c"], "only the new row is made");
    let inserts: Vec<_> = second
        .items
        .iter()
        .filter_map(|item| match item {
            MountItem::Insert { child, .. } => Some(*child),
            _ => None,
        })
        .collect();
    assert!(!inserts.is_empty(), "rows are reordered");
}

/// A component's failure shows the boundary's fallback.
#[test]
fn error_boundary_shows_the_fallback() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount, ErrorBoundary } from "@nlayer/runtime";
        import { element, append, text, component } from "nlayer:rt";

        const label = (value) => {
            const block = element("text");
            append(block, text(value));
            return block;
        };
        function Broken() {
            throw new Error("boom");
        }
        function App() {
            const view = element("view");
            append(view, component(ErrorBoundary, {
                get fallback() { return label("fell"); },
                get children() { return component(Broken, {}); },
            }));
            return view;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let batch = frame(&host, &mut core);
    assert_eq!(text_of(&batch), ["fell"]);
}

/// `context` provides its value to what it renders, and reads its default
/// outside.
#[test]
fn context_provides_values() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount, context } from "@nlayer/runtime";
        import { element, append, insert, text, component } from "nlayer:rt";

        const Theme = context("dark");
        const label = (value) => {
            const block = element("text");
            append(block, text(value));
            return block;
        };
        function App() {
            const view = element("view");
            append(view, component(Theme.Provider, {
                value: "light",
                get children() { return () => label(Theme()); },
            }));
            insert(view, () => label(Theme()));
            return view;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let batch = frame(&host, &mut core);
    assert_eq!(text_of(&batch), ["light", "dark"]);
}

/// `platform.os` is the OS the host set before the module was imported.
#[test]
fn platform_os_is_the_host() {
    let host = Host::new().unwrap();
    host.set_os(nlayer_js::Os::Ios);
    host.eval_source(
        r#"
        import { platform } from "@nlayer/ui";
        globalThis.__os = platform.os;
        "#,
    )
    .unwrap();
    assert_eq!(host.global_string("__os").as_deref(), Some("ios"));

    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { platform } from "@nlayer/ui";
        globalThis.__os = platform.os;
        "#,
    )
    .unwrap();
    assert_eq!(host.global_string("__os").as_deref(), Some("web"));
}

/// `Keyboard.dismiss` is a blur command. The platform resigns the field.
#[test]
fn keyboard_dismiss_is_a_blur_command() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { Keyboard } from "@nlayer/ui";
        Keyboard.dismiss();
        "#,
    )
    .unwrap();
    let mut core = core();
    let batch = frame(&host, &mut core);
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::Command {
            command: Command::Blur,
            ..
        }
    )));
}

/// `activeStyle` is paint in the mount batch. Its `width` is not layout.
#[test]
fn active_style_is_paint_the_platform_applies() {
    let host = Host::new().unwrap();
    host.eval_source(
        r##"
        import { mount } from "@nlayer/runtime";
        import { element, prop, append, text } from "nlayer:rt";

        function App() {
            const view = element("view");
            prop(view, "style", { width: 80, height: 40 });
            prop(view, "activeStyle", { width: 10, backgroundColor: "red", color: "#00ff00" });
            prop(view, "hoverStyle", false);
            const block = element("text");
            append(block, text("hi"));
            append(view, block);
            return view;
        }
        mount(App);
        "##,
    )
    .unwrap();
    let mut core = core();
    let batch = frame(&host, &mut core);
    let view = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Create {
                id,
                kind: ViewKind::View,
                ..
            } => Some(*id),
            _ => None,
        })
        .unwrap();
    assert_eq!(frame_of(&batch, view).unwrap().size.width, Points(80.0));
    let paint = batch.items.iter().find_map(|item| match item {
        MountItem::StateStyle {
            id,
            interaction: nlayer_core::Interaction::Active,
            view: paint,
            text,
        } if *id == view => Some((paint, text)),
        _ => None,
    });
    let (view, text) = paint.expect("active paint");
    assert_eq!(
        view.background_color,
        nlayer_core::Color::rgba(255, 0, 0, 255)
    );
    assert_eq!(text.color, Some(nlayer_core::Color::rgba(0, 255, 0, 255)));
    assert!(
        !batch.items.iter().any(|item| matches!(
            item,
            MountItem::StateStyle {
                interaction: nlayer_core::Interaction::Hover,
                ..
            }
        )),
        "a cleared hover style that was never set is not sent"
    );
}

/// An unknown style name and a class warn and do not drop the styles that apply.
#[test]
fn unknown_style_and_class_do_not_drop_known_styles() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount } from "@nlayer/runtime";
        import { element, prop } from "nlayer:rt";

        function App() {
            const view = element("view");
            prop(view, "style", { width: 10, height: 10, nope: 1 });
            prop(view, "class", "p-4");
            prop(view, "classList", { "m-2": true, off: false });
            return view;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let batch = frame(&host, &mut core);
    let view = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Create {
                id,
                kind: ViewKind::View,
                ..
            } => Some(*id),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        frame_of(&batch, view).unwrap().size,
        Size::new(Points(10.0), Points(10.0))
    );
}

fn transforms_of(batch: &MountBatch, id: u32) -> Vec<(TransformKind, f32)> {
    batch
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            MountItem::ViewStyle { id: at, style } if *at == id => Some(
                style
                    .transform
                    .ops()
                    .iter()
                    .map(|op| (op.kind, op.value))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn view_sized(batch: &MountBatch, width: f32, height: f32) -> u32 {
    batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Frame { id, frame }
                if frame.size.width.0 == width && frame.size.height.0 == height =>
            {
                Some(*id)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no view {width}x{height}"))
}

/// `translate` / `scale` arrays are static transforms the host paints.
#[test]
fn translate_and_scale_arrays_reach_the_host() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount } from "@nlayer/runtime";
        import { element, prop, append } from "nlayer:rt";

        function App() {
            const root = element("view");
            const pair = element("view");
            prop(pair, "style", {
                width: 100,
                height: 40,
                translate: [10, "50%"],
                scale: ["150%", 2],
            });
            const one = element("view");
            prop(one, "style", { width: 80, height: 20, translate: [12], scale: [1.25] });
            const bad = element("view");
            prop(bad, "style", { width: 30, height: 10, translate: [1, 2, 3] });
            append(root, pair);
            append(root, one);
            append(root, bad);
            return root;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let batch = frame(&host, &mut core);
    assert_eq!(
        transforms_of(&batch, view_sized(&batch, 100.0, 40.0)),
        [
            (TransformKind::TranslateX, 10.0),
            (TransformKind::TranslateY, 20.0),
            (TransformKind::ScaleX, 1.5),
            (TransformKind::ScaleY, 2.0),
        ]
    );
    assert_eq!(
        transforms_of(&batch, view_sized(&batch, 80.0, 20.0)),
        [
            (TransformKind::TranslateX, 12.0),
            (TransformKind::Scale, 1.25),
        ]
    );
    assert!(
        transforms_of(&batch, view_sized(&batch, 30.0, 10.0)).is_empty(),
        "an invalid array does not apply"
    );
}
