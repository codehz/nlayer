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
