//! Headless runs: the mount tree, text, layout and input.

use std::time::Duration;

use nlayer_core::{
    ViewKind,
    geometry::{Points, Size, px},
};
use nlayer_testing::Headless;

fn app(source: &str) -> Headless {
    let bundle = nlayer_bundler::compile_module("App.tsx", source).unwrap();
    Headless::start(bundle, Size::new(px(390.0), px(844.0))).unwrap()
}

#[test]
fn a_press_changes_the_text() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    const count = signal(0);
    return (
        <View style={{ width: 100, height: 40 }} onPress={() => count.set(count() + 1)}>
            <Text>{`Count: ${count()}`}</Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    assert_eq!(app.texts(), ["Count: 0"]);
    assert!(app.press_text("Count"));
    assert!(
        app.wait_for("Count: 1", Duration::from_secs(1)),
        "{:?}",
        app.texts()
    );
    app.stop();
}

#[test]
fn layout_and_tree_are_assertable() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 200, height: 100, padding: 10 }}>
            <Text>hello</Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    let screen = app.screen();
    let root = screen.tree(nlayer_core::ROOT_ID).expect("the root");
    assert_eq!(root.children.len(), 1);
    let view = &root.children[0];
    assert_eq!(view.kind, ViewKind::View);
    assert_eq!(view.frame.size, Size::new(Points(200.0), Points(100.0)));
    let text = &view.children[0];
    assert_eq!(text.text.as_deref(), Some("hello"));
    // Inside the padding.
    assert_eq!(
        text.frame.origin,
        nlayer_core::geometry::Point::new(Points(10.0), Points(10.0))
    );
    app.stop();
}

#[test]
fn typing_reaches_onchange_and_the_text() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { Input, Text, View } from "@nlayer/ui";

function App() {
    const text = signal("");
    return (
        <View style={{ width: 300, height: 100 }}>
            <Input value={text()} onChange={(value: string) => text.set(value)} />
            <Text>{`hi ${text()}`}</Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    let input = app
        .screen()
        .views()
        .find_map(|(id, view)| (view.kind == ViewKind::Input).then_some(id))
        .expect("an input");
    app.type_text(input, "abc");
    assert!(
        app.wait_for("hi abc", Duration::from_secs(1)),
        "{:?}",
        app.texts()
    );
    assert_eq!(app.screen().input_values(), ["abc"]);
    app.stop();
}

#[test]
fn the_color_scheme_follows_the_platform() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Text, View, platform } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 200, height: 40 }}>
            <Text>{`scheme: ${platform.colorScheme()}`}</Text>
        </View>
    );
}
mount(App);
"#;
    let bundle = nlayer_bundler::compile_module("App.tsx", source).unwrap();
    let mut config = nlayer_engine::EngineConfig::new(bundle, Size::new(px(390.0), px(844.0)));
    config = config.with_color_scheme(nlayer_core::ColorScheme::Light);
    let mut app =
        Headless::start_with_config(config, || nlayer_core::text::ApproxTextEngine).unwrap();
    assert_eq!(app.texts(), ["scheme: light"]);
    app.control()
        .set_color_scheme(nlayer_core::ColorScheme::Dark);
    assert!(
        app.wait_for("scheme: dark", Duration::from_secs(1)),
        "{:?}",
        app.texts()
    );
    app.stop();
}

#[test]
fn accessibility_is_recorded_and_disabled_blocks_a_press() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    const count = signal(0);
    return (
        <View onPress={() => count.set(count() + 1)} style={{ width: 300, height: 200 }}>
            <View
                accessibilityLabel="inner"
                accessibilityHint="does nothing"
                role="button"
                accessibilityState={{ disabled: true, selected: true, checked: "mixed", expanded: false }}
                accessibilityValue={{ min: 0, max: 10, now: 5, text: "five" }}
                disabled
                style={{ width: 100, height: 50 }}
            />
            <Text>{`count: ${count()}`}</Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    let id = app
        .screen()
        .views()
        .find_map(|(id, view)| (view.accessibility.label.as_deref() == Some("inner")).then_some(id))
        .expect("the labelled view");
    let view = app.screen().view(id).unwrap();
    assert_eq!(
        view.accessibility.role,
        nlayer_core::AccessibilityRole::Button
    );
    assert_eq!(view.accessibility.hint.as_deref(), Some("does nothing"));
    assert!(view.accessibility.state.disabled);
    assert!(view.accessibility.state.selected);
    assert_eq!(
        view.accessibility.state.checked,
        Some(nlayer_core::Checked::Mixed)
    );
    assert_eq!(view.accessibility.state.expanded, Some(false));
    let value = view.accessibility.value.as_ref().expect("a value");
    assert_eq!(
        (value.min, value.max, value.now),
        (Some(0.0), Some(10.0), Some(5.0))
    );
    assert_eq!(value.text.as_deref(), Some("five"));
    // The parent takes presses; the disabled child blocks them.
    assert!(app.press_selector("label:inner").is_err());
    assert_eq!(app.texts(), ["count: 0"]);
    app.stop();
}

#[test]
fn dialog_onclose_fires_on_back() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { Dialog, Text, View } from "@nlayer/ui";

function App() {
    const open = signal(true);
    const closed = signal(false);
    return (
        <View style={{ flex: 1 }}>
            <Text testID="status">{closed() ? "closed" : "open"}</Text>
            <Dialog
                open={open()}
                onOpenChange={(e) => open.set(e.open)}
                onClose={() => closed.set(true)}
            >
                <View style={{ width: 200, height: 100 }}>
                    <Text>Modal content</Text>
                </View>
            </Dialog>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    assert_eq!(app.texts(), ["open", "Modal content"]);

    app.back();
    app.next_frame(Duration::from_secs(1));

    assert!(
        app.wait_for("closed", Duration::from_secs(1)),
        "onClose should fire and set closed=true, texts: {:?}",
        app.texts()
    );
    assert_eq!(app.texts(), ["closed"], "modal should unmount after back");
    app.stop();
}

#[test]
fn dialog_reopens_after_user_close() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { Dialog, Text, View } from "@nlayer/ui";

function App() {
    const open = signal(true);
    const closed = signal(false);
    return (
        <View style={{ flex: 1 }}>
            <Text testID="status">{closed() ? "closed" : "open"}</Text>
            <View
                testID="reopen-button"
                style={{ width: 100, height: 40 }}
                onPress={() => { open.set(true); closed.set(false); }}
            >
                <Text>Reopen</Text>
            </View>
            <Dialog
                open={open()}
                onOpenChange={(e) => open.set(e.open)}
                onClose={() => closed.set(true)}
            >
                <View style={{ width: 200, height: 100 }}>
                    <Text>Modal content</Text>
                </View>
            </Dialog>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    assert_eq!(app.texts(), ["open", "Reopen", "Modal content"]);

    // Close via back button
    app.back();
    app.next_frame(Duration::from_secs(1));
    assert!(
        app.wait_for("closed", Duration::from_secs(1)),
        "onClose should fire, texts: {:?}",
        app.texts()
    );
    assert_eq!(app.texts(), ["closed", "Reopen"], "modal should close");

    // Reopen by pressing the button (regression test: closedByUser must be reset)
    app.press_selector("reopen-button").unwrap();
    app.next_frame(Duration::from_secs(1));

    // Modal should reopen
    assert!(
        app.wait_for("Modal content", Duration::from_secs(1)),
        "modal should reopen after user close, texts: {:?}",
        app.texts()
    );
    assert_eq!(
        app.texts(),
        ["open", "Reopen", "Modal content"],
        "modal reopened"
    );
    app.stop();
}

#[test]
fn dialog_onclose_not_fired_with_close_on_escape_false() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { Dialog, Text, View } from "@nlayer/ui";

function App() {
    const open = signal(true);
    const closed = signal(false);
    return (
        <View style={{ flex: 1 }}>
            <Text testID="status">{closed() ? "closed" : "open"}</Text>
            <Dialog
                open={open()}
                closeOnEscape={false}
                onOpenChange={(e) => open.set(e.open)}
                onClose={() => closed.set(true)}
            >
                <View style={{ width: 200, height: 100 }}>
                    <Text>Modal content</Text>
                </View>
            </Dialog>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    assert_eq!(app.texts(), ["open", "Modal content"]);

    app.back();
    app.next_frame(Duration::from_secs(1));

    // Modal should stay mounted, onClose should not fire
    assert_eq!(
        app.texts(),
        ["open", "Modal content"],
        "modal should stay open when closeOnEscape=false"
    );
    app.stop();
}

#[test]
fn dialog_onclose_not_fired_on_programmatic_close() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { Dialog, Text, View } from "@nlayer/ui";

function App() {
    const open = signal(true);
    const closed = signal(false);
    return (
        <View style={{ flex: 1 }}>
            <Text testID="status">{closed() ? "closed" : "open"}</Text>
            <View
                testID="close-button"
                style={{ width: 100, height: 40 }}
                onPress={() => open.set(false)}
            >
                <Text>Close</Text>
            </View>
            <Dialog
                open={open()}
                onOpenChange={(e) => open.set(e.open)}
                onClose={() => closed.set(true)}
            >
                <View style={{ width: 200, height: 100 }}>
                    <Text>Modal content</Text>
                </View>
            </Dialog>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    assert_eq!(app.texts(), ["open", "Close", "Modal content"]);

    // Press the close button (programmatic close via open=false)
    app.press_selector("close-button").unwrap();
    app.next_frame(Duration::from_secs(1));

    // Modal should close but onClose should NOT fire
    assert_eq!(
        app.texts(),
        ["open", "Close"],
        "modal should close but status should stay 'open' (onClose not called)"
    );
    app.stop();
}
