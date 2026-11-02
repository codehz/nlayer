//! Test viewport="parent": list uses parent scroll view's viewport

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
fn parent_viewport_renders_items() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { List, Scroll, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 390, height: 844 }}>
            <Scroll style={{ width: 390, height: 400 }} testID="scroll">
                <List
                    testID="vp-list"
                    each={Array.from({ length: 20 }, (_, i) => i)}
                    style={{ width: 390 }}
                    viewport="parent"
                >{(item) => (
                    <View style={{ height: 50 }} testID={`item-${item()}`}>
                        <Text>{`Item ${item()}`}</Text>
                    </View>
                )}</List>
            </Scroll>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    // Verify items render
    let screen = app.screen();
    let item_count = screen
        .views()
        .filter(|(_, v)| v.test_id.as_deref().is_some_and(|t| t.starts_with("item-")))
        .count();

    assert!(item_count > 0, "Should render some items");
    assert!(item_count < 20, "Should not render all items (windowing)");

    // The list is as long as its content and clips: the scroll view around
    // it scrolls over the whole list.
    let list = screen
        .views()
        .find_map(|(_, v)| (v.test_id.as_deref() == Some("vp-list")).then_some(v))
        .expect("the list");
    assert_eq!(list.kind, ViewKind::View);
    assert_eq!(list.frame.size.height.0, 1000.0);
    let scroll = screen
        .views()
        .find_map(|(_, v)| (v.test_id.as_deref() == Some("scroll")).then_some(v))
        .expect("the scroll view");
    assert_eq!(
        scroll.scroll_content.expect("content size").height.0,
        1000.0
    );

    app.stop();
}

#[test]
fn parent_viewport_renders_only_visible_rows() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { List, Scroll, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 390, height: 844 }}>
            <Scroll style={{ width: 390, height: 400 }} testID="scroll">
                <List
                    each={Array.from({ length: 50 }, (_, i) => i)}
                    style={{ width: 390 }}
                    viewport="parent"
                >{(item) => (
                    <View style={{ height: 50 }} testID={`item-${item()}`}>
                        <Text>{`Item ${item()}`}</Text>
                    </View>
                )}</List>
            </Scroll>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    // Count rendered items (should be viewport + overscan)
    let screen = app.screen();
    let rendered_count = screen
        .views()
        .filter(|(_, v)| v.test_id.as_deref().is_some_and(|t| t.starts_with("item-")))
        .count();

    // With 400px viewport and 50px items, visible = 8 items
    // With overscan (1.0 viewport = 400px = 8 more items), total ~16 items
    assert!(
        rendered_count < 30,
        "Should render only visible + overscan items, not all 50: rendered {}",
        rendered_count
    );
    assert!(
        rendered_count >= 8,
        "Should render at least visible items: rendered {}",
        rendered_count
    );

    app.stop();
}

#[test]
fn parent_viewport_scrolls_with_parent() {
    let source = r#"
import { mount } from "@nlayer/runtime";
import { List, Scroll, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 390, height: 844 }}>
            <Scroll style={{ width: 390, height: 400 }} testID="scroll">
                <List
                    each={Array.from({ length: 30 }, (_, i) => i)}
                    style={{ width: 390 }}
                    viewport="parent"
                >{(item) => (
                    <View style={{ height: 50 }} testID={`item-${item()}`}>
                        <Text>{`Item ${item()}`}</Text>
                    </View>
                )}</List>
            </Scroll>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    let screen = app.screen();

    // Find the parent scroll view
    let scroll_id = screen
        .views()
        .find_map(|(id, v)| {
            (v.kind == ViewKind::Scroll && v.test_id.as_deref() == Some("scroll")).then_some(id)
        })
        .expect("scroll");

    // Scroll the parent
    app.scroll(scroll_id, 0.0, 300.0);
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    // Should now see items further down
    assert!(
        app.texts()
            .iter()
            .any(|t| t.contains("Item 6") || t.contains("Item 7") || t.contains("Item 8")),
        "Should see items further down after scrolling: {:?}",
        app.texts()
    );

    app.stop();
}
