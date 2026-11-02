//! v4 M1: `userSelect`, `selectable`, inline attachments and `href`, as the
//! mount batch carries them to the headless model.

use nlayer_core::geometry::{Points, Size, px};
use nlayer_testing::Headless;

fn app(source: &str) -> Headless {
    let bundle = nlayer_bundler::compile_module("App.tsx", source).unwrap();
    Headless::start(bundle, Size::new(px(390.0), px(844.0))).unwrap()
}

#[test]
fn user_select_resolves_and_a_selectable_view_owns_its_texts() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    return (
        <View>
            <View selectable testID="outer" style={{ width: 300 }}>
                <Text testID="plain">Plain</Text>
                <View selectable testID="inner">
                    <Text testID="nested">Nested</Text>
                    <Text testID="skip" style={{ userSelect: "none" }}>Skip</Text>
                </View>
                <Text testID="own" style={{ userSelect: "text" }}>Own</Text>
            </View>
            <Text testID="auto">Auto</Text>
            <Text testID="lone" style={{ userSelect: "text" }}>Lone</Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    let screen = app.screen();
    let id = |name: &str| {
        screen
            .views()
            .find_map(|(id, view)| (view.test_id.as_deref() == Some(name)).then_some(id))
            .unwrap_or_else(|| panic!("no `{name}`"))
    };
    let text_id = |text: &str| {
        screen
            .views()
            .find_map(|(id, view)| (view.text.as_deref() == Some(text)).then_some(id))
            .unwrap()
    };

    assert!(screen.view(id("outer")).unwrap().selectable);
    assert!(screen.view(id("inner")).unwrap().selectable);
    // `auto` with no selectable around it is not selectable.
    assert_eq!(screen.view(text_id("Auto")).unwrap().text_selectable, None);
    assert_eq!(screen.selection_owner(text_id("Auto")), None);
    // A text inside a selectable view belongs to the outermost one.
    assert_eq!(screen.view(text_id("Plain")).unwrap().text_selectable, None);
    assert_eq!(screen.selection_owner(text_id("Plain")), Some(id("outer")));
    // Nested selectable views collapse into the outer one.
    assert_eq!(screen.selection_owner(text_id("Nested")), Some(id("outer")));
    // A `userSelect: "text"` text inside one is not independently selectable.
    assert_eq!(
        screen.view(text_id("Own")).unwrap().text_selectable,
        Some(true)
    );
    assert_eq!(screen.selection_owner(text_id("Own")), Some(id("outer")));
    // `userSelect: "none"` stays out of the area.
    assert_eq!(
        screen.view(text_id("Skip")).unwrap().text_selectable,
        Some(false)
    );
    assert_eq!(screen.selection_owner(text_id("Skip")), None);
    // Alone, `"text"` is a single-block selection.
    assert_eq!(
        screen.view(text_id("Lone")).unwrap().text_selectable,
        Some(true)
    );
    assert_eq!(
        screen.selection_owner(text_id("Lone")),
        Some(text_id("Lone"))
    );
    app.stop();
}

#[test]
fn an_inline_view_is_a_child_of_the_text_at_its_size() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 300 }}>
            <Text testID="line" style={{ fontSize: 16, lineHeight: 26 }}>
                {"Made with "}
                <View testID="badge" style={{ height: 18, paddingHorizontal: 7 }}>
                    <Text>LIVE</Text>
                </View>
                {"!"}
            </Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    let screen = app.screen();
    let text = screen
        .views()
        .find_map(|(id, view)| (view.test_id.as_deref() == Some("line")).then_some(id))
        .unwrap();
    let badge = screen
        .views()
        .find_map(|(id, view)| (view.test_id.as_deref() == Some("badge")).then_some(id))
        .unwrap();
    let text_view = screen.view(text).unwrap();
    assert!(
        text_view.text.as_deref().unwrap().contains('\u{FFFC}'),
        "{:?}",
        text_view.text
    );
    let badge_view = screen.view(badge).unwrap();
    assert_eq!(badge_view.parent, Some(text), "under the text block");
    assert_eq!(badge_view.frame.size.height, Points(18.0));
    assert!(
        badge_view.frame.size.width > Points(0.0),
        "measured at its content size: {:?}",
        badge_view.frame
    );
    assert!(
        badge_view.frame.origin.x > Points(0.0),
        "after the text before it: {:?}",
        badge_view.frame
    );
    app.stop();
}

#[test]
fn an_attachment_past_the_line_limit_is_out_of_sight() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 100 }}>
            <Text style={{ numberOfLines: 1 }}>
                {"aaaaaaaaaaaaaaaaaaaa"}
                <View testID="hidden" style={{ width: 18, height: 18 }} />
            </Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    let screen = app.screen();
    let hidden = screen
        .views()
        .find_map(|(id, view)| (view.test_id.as_deref() == Some("hidden")).then_some(id))
        .unwrap();
    assert!(
        screen.view(hidden).unwrap().frame.origin.x < Points(-99_000.0),
        "cut off: {:?}",
        screen.view(hidden).unwrap().frame
    );
    app.stop();
}

#[test]
fn a_short_press_opens_a_checked_href_and_still_presses() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    const pressed = signal(0);
    return (
        <View>
            <Text
                testID="terms"
                href="https://example.com/terms"
                onPress={() => pressed.set(pressed() + 1)}
            >Terms</Text>
            <Text testID="bad" href="javascript:alert(1)" onPress={() => pressed.set(pressed() + 10)}>
                Bad
            </Text>
            <Text>{`pressed:${pressed()}`}</Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    app.press_selector("id:terms").unwrap();
    assert!(
        app.wait_for("pressed:1", std::time::Duration::from_secs(1)),
        "{:?}",
        app.texts()
    );
    assert_eq!(
        app.opened_urls(),
        ["https://example.com/terms"],
        "the checked address opens"
    );

    app.press_selector("id:bad").unwrap();
    assert!(
        app.wait_for("pressed:11", std::time::Duration::from_secs(1)),
        "{:?}",
        app.texts()
    );
    assert_eq!(
        app.opened_urls(),
        ["https://example.com/terms"],
        "a refused address opens nothing"
    );
    app.stop();
}

#[test]
fn an_href_on_a_nested_span_opens_from_its_own_press() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    const pressed = signal(0);
    return (
        <View>
            <Text>
                {"By continuing you agree to the "}
                <Text testID="terms" href="https://example.com/terms" onPress={() => pressed.set(pressed() + 1)}>
                    Terms
                </Text>
                {"."}
            </Text>
            <Text>{`pressed:${pressed()}`}</Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    app.press_selector("id:terms").unwrap();
    assert!(
        app.wait_for("pressed:1", std::time::Duration::from_secs(1)),
        "{:?}",
        app.texts()
    );
    assert_eq!(app.opened_urls(), ["https://example.com/terms"]);
    app.stop();
}
