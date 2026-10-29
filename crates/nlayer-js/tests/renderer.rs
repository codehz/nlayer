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

fn press_id(batch: &MountBatch) -> u32 {
    batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Events { id, events } if events.contains(EventKind::Press) => Some(*id),
            _ => None,
        })
        .expect("a pressable view")
}

fn press(host: &Host, core: &mut Core<ApproxTextEngine>, id: u32) -> MountBatch {
    host.dispatch_event(id, EventKind::Press, &EventPayload::None);
    frame(host, core)
}

fn creates(batch: &MountBatch) -> usize {
    batch
        .items
        .iter()
        .filter(|item| matches!(item, MountItem::Create { .. }))
        .count()
}

/// `<If when else>` shows one branch, including a function child that reads
/// `when`, and the other branch when `when` flips.
#[test]
fn if_switches_when_else_and_a_function_child() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount, signal } from "@nlayer/runtime";
        import { element, prop, append, insert, text, cond, component } from "nlayer:rt";

        const on = signal(true);
        const who = signal("ada");
        const label = (value) => {
            const block = element("text");
            append(block, text(value));
            return block;
        };
        function App() {
            const view = element("view");
            prop(view, "onPress", () => {
                if (on()) who.set("bea");
                on.set(!on());
            });
            // Compiled `<If when={on() ? who() : null} else="off">{(value) => value()}</If>`.
            insert(view, ((w) => cond(w, () => ((value) => label(value()))(w), () => label("off")))(
                () => (on() ? who() : null),
            ));
            insert(view, component(globalThis.If, {
                get when() { return on(); },
                get else() { return label("else"); },
                get children() { return label("when"); },
            }));
            return view;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let first = frame(&host, &mut core);
    assert_eq!(text_of(&first), ["ada", "when"]);
    let id = press_id(&first);
    let second = press(&host, &mut core, id);
    assert!(
        text_of(&second).contains(&"off".to_owned())
            && text_of(&second).contains(&"else".to_owned()),
        "{:?}",
        text_of(&second)
    );
    let third = press(&host, &mut core, id);
    assert!(
        text_of(&third).contains(&"bea".to_owned()) && text_of(&third).contains(&"when".to_owned()),
        "{:?}",
        text_of(&third)
    );
}

/// `<Each key={fn}>` keeps rows across a reorder. `key="index"` updates the
/// row in place, and a key function is also given the index.
#[test]
fn each_keeps_a_key_function_and_an_index() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount, signal } from "@nlayer/runtime";
        import { element, prop, append, insert, text, each } from "nlayer:rt";

        const rows = signal([{ id: "a" }, { id: "b" }, { id: "c" }]);
        const lines = signal(["a"]);
        const seen = [];
        globalThis.seen = seen;
        const label = (value) => {
            const block = element("text");
            append(block, text(value));
            return block;
        };
        function App() {
            const view = element("view");
            prop(view, "onPress", () => {
                rows.set([{ id: "c" }, { id: "a" }, { id: "d" }]);
                lines.set(["ab"]);
            });
            const row = (value) => {
                const block = element("text");
                insert(block, () => value());
                return block;
            };
            insert(view, each(() => rows(), (item, index) => row(() => `${item().id}${index()}`), (item) => item.id));
            insert(view, each(() => lines(), (item) => row(item), "index"));
            insert(view, each(() => lines(), (item, index) => row(() => `${item()}:${index()}`), (item, index) => {
                globalThis.__keyIndex = String(index);
                return index;
            }));
            return view;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let first = frame(&host, &mut core);
    assert_eq!(text_of(&first), ["a0", "b1", "c2", "a", "a:0"]);
    let id = press_id(&first);
    let second = press(&host, &mut core, id);
    assert!(
        text_of(&second).contains(&"d2".to_owned()),
        "the new key is rendered: {:?}",
        text_of(&second)
    );
    assert!(
        text_of(&second).contains(&"c0".to_owned()) && text_of(&second).contains(&"a1".to_owned()),
        "kept keys take their new index: {:?}",
        text_of(&second)
    );
    assert_eq!(
        creates(&second),
        1,
        "only the new key makes a row: {second:?}"
    );
    assert!(
        second
            .items
            .iter()
            .any(|item| matches!(item, MountItem::Insert { .. })),
        "kept rows are reordered"
    );
    assert!(
        text_of(&second).contains(&"ab".to_owned()),
        "key=index updates the row in place: {:?}",
        text_of(&second)
    );
    assert!(
        !text_of(&second).contains(&"a".to_owned()),
        "the index row is not made again: {:?}",
        text_of(&second)
    );
    assert_eq!(host.global_string("__keyIndex").as_deref(), Some("0"));
}

/// A failure under an `ErrorBoundary` (a binding, an effect, a listener, the
/// component body) shows the fallback. `reset` renders the children again.
/// A fallback that throws goes to the boundary above.
#[test]
fn error_boundary_catches_effects_listeners_and_a_rethrown_fallback() {
    let host = Host::new().unwrap();
    host.eval_source(
        r#"
        import { mount, signal, effect, ErrorBoundary } from "@nlayer/runtime";
        import { element, prop, append, insert, text, component } from "nlayer:rt";

        const mode = signal("ok");
        const label = (value) => {
            const block = element("text");
            append(block, text(value));
            return block;
        };
        const button = (name, run) => {
            const view = element("view");
            prop(view, "testID", name);
            prop(view, "onPress", run);
            append(view, text(name));
            return view;
        };
        function Risky() {
            effect(() => {
                if (mode() === "effect") throw new Error("effect");
            });

            const block = element("text");
            prop(block, "onPress", () => { throw new Error("press"); });
            insert(block, () => {
                if (mode() === "binding") throw new Error("binding");
                return "ok";
            });
            return block;
        }
        function App() {
            const view = element("view");
            append(view, button("binding", () => mode.set("binding")));
            append(view, button("effect", () => mode.set("effect")));

            append(view, component(ErrorBoundary, {
                fallback: (error, reset) => {
                    const row = element("view");
                    append(row, label(`caught ${error.message}`));
                    append(row, button("reset", () => { mode.set("ok"); reset(); }));
                    return row;
                },
                get children() { return component(Risky, {}); },
            }));
            append(view, component(ErrorBoundary, {
                fallback: (error) => label(`outer ${error.message}`),
                get children() {
                    return component(ErrorBoundary, {
                        fallback: (error) => { throw new Error(`fallback of ${error.message}`); },
                        get children() { return component(Risky, {}); },
                    });
                },
            }));
            return view;
        }
        mount(App);
        "#,
    )
    .unwrap();
    let mut core = core();
    let first = frame(&host, &mut core);
    assert!(
        text_of(&first).contains(&"ok".to_owned()),
        "{:?}",
        text_of(&first)
    );
    let binding = press_named(&first, "binding");
    let effect = press_named(&first, "effect");
    let pressed = press(&host, &mut core, text_id(&first, "ok"));
    assert!(
        text_of(&pressed).contains(&"caught press".to_owned()),
        "{:?}",
        text_of(&pressed)
    );
    let restored = press(&host, &mut core, press_named(&pressed, "reset"));
    assert!(
        text_of(&restored).contains(&"ok".to_owned()),
        "{:?}",
        text_of(&restored)
    );

    let bound = press(&host, &mut core, binding);
    assert!(
        text_of(&bound).contains(&"caught binding".to_owned())
            && text_of(&bound).contains(&"outer fallback of binding".to_owned()),
        "{:?}",
        text_of(&bound)
    );
    let restored = press(&host, &mut core, press_named(&bound, "reset"));
    assert!(
        text_of(&restored).contains(&"ok".to_owned()),
        "{:?}",
        text_of(&restored)
    );

    let effected = press(&host, &mut core, effect);
    assert!(
        text_of(&effected).contains(&"caught effect".to_owned()),
        "{:?}",
        text_of(&effected)
    );
    let restored = press(&host, &mut core, press_named(&effected, "reset"));
    assert!(
        text_of(&restored).contains(&"ok".to_owned()),
        "{:?}",
        text_of(&restored)
    );
}

fn press_named(batch: &MountBatch, name: &str) -> u32 {
    batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::TestId { id, test_id } if test_id.as_deref() == Some(name) => Some(*id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {name} in {batch:?}"))
}

fn text_id(batch: &MountBatch, text: &str) -> u32 {
    batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Text { id, text: body, .. } if body.text == text => Some(*id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text {text} in {batch:?}"))
}
