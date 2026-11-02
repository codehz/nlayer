//! Test sticky headers and inverted lists with numeric frame assertions.

use std::time::Duration;

use nlayer_core::{
    ViewKind,
    geometry::{Size, px},
};
use nlayer_testing::Headless;

fn app(source: &str) -> Headless {
    let bundle = nlayer_bundler::compile_module("App.tsx", source).unwrap();
    Headless::start(bundle, Size::new(px(390.0), px(844.0))).unwrap()
}

#[test]
fn sticky_header_at_natural_position() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 390, height: 400 }}>
            <List
                each={[0, 1, 2, 3, 4, 5]}
                stickyHeaderIndices={[0, 3]}
                style={{ width: 390, height: 400 }}
            >{(item, index) => (
                <View style={{ height: 50 }} testID={`item-${index()}`}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    let screen = app.screen();
    let item0_frame = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(id, _)| screen.frame(id))
        .expect("item-0 frame");

    assert_eq!(
        item0_frame.origin.y.0, 0.0,
        "sticky item 0 at offset=0: y=0"
    );

    app.stop();
}

#[test]
fn sticky_header_pins_when_scrolled() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 390, height: 400 }}>
            <List
                each={Array.from({ length: 10 }, (_, i) => i)}
                stickyHeaderIndices={[0, 5]}
                style={{ width: 390, height: 400 }}
            >{(item, index) => (
                <View style={{ height: 50 }} testID={`item-${index()}`}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    let screen = app.screen();
    let list_id = screen
        .views()
        .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
        .expect("list");

    // Scroll to 100: item 0 (at=0, next=250) -> shift=100, y=100
    app.scroll(list_id, 0.0, 100.0);
    // Wait for content to update
    std::thread::sleep(Duration::from_millis(100));

    let screen = app.screen();
    let item0_container_frame = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-0 container");

    assert_eq!(
        item0_container_frame.origin.y.0, 100.0,
        "sticky item 0 at offset=100: shift=(100-0).clamp(0,250)=100"
    );

    app.stop();
}
#[test]
fn sticky_header_clamped_at_boundary() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 390, height: 400 }}>
            <List
                each={Array.from({ length: 20 }, (_, i) => i)}
                stickyHeaderIndices={[0, 5]}
                style={{ width: 390, height: 400 }}
            >{(item, index) => (
                <View style={{ height: 50 }} testID={`item-${index()}`}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    let screen = app.screen();
    let list_id = screen
        .views()
        .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
        .expect("list");

    // Scroll to 300. Item 5 starts at 250, so it has pinned and pushed
    // item 0 up until their edges meet (250 - 50).
    app.scroll(list_id, 0.0, 300.0);
    assert!(app.wait_for("Item 5", Duration::from_millis(300)));

    let screen = app.screen();
    let item0_container_frame = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-0 container");
    let item5_container_frame = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-5"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-5 container");

    assert_eq!(
        item0_container_frame.origin.y.0, 200.0,
        "sticky item 0 yields so its bottom meets item 5"
    );
    assert_eq!(
        item5_container_frame.origin.y.0, 300.0,
        "sticky item 5 is pinned to the viewport"
    );

    app.stop();
}

#[test]
fn inverted_list_reverses_positions() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 390, height: 400 }}>
            <List
                each={[0, 1, 2, 3, 4]}
                inverted={true}
                style={{ width: 390, height: 400 }}
            >{(item, index) => (
                <View style={{ height: 50 }} testID={`item-${index()}`}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    let screen = app.screen();

    // Content is 250, padded to the 400 viewport so item 0 rests on the
    // bottom. item 4 (natural 200) sits above it.
    let item0_container_frame = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-0 container");
    let item4_container_frame = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-4"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-4 container");

    assert_eq!(
        item0_container_frame.origin.y.0, 350.0,
        "inverted item 0 rests on the viewport bottom"
    );
    assert_eq!(
        item4_container_frame.origin.y.0, 150.0,
        "inverted item 4 sits above item 0"
    );

    app.stop();
}

#[test]
fn inverted_ignores_sticky() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 390, height: 400 }}>
            <List
                each={[0, 1, 2, 3, 4]}
                stickyHeaderIndices={[0]}
                inverted={true}
                style={{ width: 390, height: 400 }}
            >{(item, index) => (
                <View style={{ height: 50 }} testID={`item-${index()}`}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    let screen = app.screen();
    let item0_container_frame = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-0 container");

    assert_eq!(
        item0_container_frame.origin.y.0, 350.0,
        "inverted+sticky: sticky ignored, item 0 rests at the bottom"
    );
    app.stop();
}

#[test]
fn inverted_tall_list_opens_on_item_zero() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 390, height: 400 }}>
            <List
                each={Array.from({ length: 20 }, (_, i) => i)}
                inverted={true}
                style={{ width: 390, height: 400 }}
            >{(item, index) => (
                <View style={{ height: 50 }} testID={`item-${index()}`}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);
    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    let screen = app.screen();
    let (scroll_y, viewport) = screen
        .views()
        .find(|(_, v)| v.kind == ViewKind::Scroll)
        .map(|(_, v)| (v.scroll_y, v.frame.size.height.0))
        .expect("list");
    let item0 = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-0 container");
    // 20 * 50 = 1000. The viewport sits on the end, so item 0 is on screen.
    assert_eq!(scroll_y, 600.0, "scroll y {scroll_y}");
    assert!(
        item0.origin.y.0 >= scroll_y && item0.origin.y.0 < scroll_y + viewport,
        "item 0 at {} is outside {scroll_y}..{}",
        item0.origin.y.0,
        scroll_y + viewport
    );

    app.stop();
}
