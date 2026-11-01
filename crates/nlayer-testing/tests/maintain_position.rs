//! Test maintainVisibleContentPosition: keeping the first visible item in place
//! when prepending, and auto-scrolling to bottom when near the end.

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
fn prepend_maintains_visible_item_position() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    const items = signal([10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20]);
    const prepend = () => {
        const current = items();
        items.set([current[0] - 1, ...current]);
    };
    return (
        <View style={{ width: 390, height: 844 }}>
            <List
                each={items()}
                style={{ width: 390, height: 400 }}
                maintainVisibleContentPosition={{
                    minIndexForVisible: 0
                }}
            >{(item) => (
                <View style={{ height: 50 }} testID={`item-${item()}`}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
            <View onPress={prepend} testID="prepend">
                <Text>Prepend</Text>
            </View>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    assert!(app.wait_for("Item 10", Duration::from_secs(1)));

    let screen = app.screen();
    let list_id = screen
        .views()
        .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
        .expect("list");

    // Scroll down so item 12 is at the top
    app.scroll(list_id, 0.0, 100.0);
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    // Get item 12's position before prepend
    let screen = app.screen();
    let item_12_before = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-12"))
        .and_then(|(id, _)| screen.frame(id))
        .expect("item-12 before prepend");

    // Prepend an item (item 9)
    assert!(app.press_text("Prepend"));
    assert!(app.wait_for("Item 9", Duration::from_secs(1)));

    // Get item 12's position after prepend - should be same
    let screen = app.screen();
    let item_12_after = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-12"))
        .and_then(|(id, _)| screen.frame(id))
        .expect("item-12 after prepend");

    assert_eq!(
        item_12_before.origin.y.0, item_12_after.origin.y.0,
        "Item 12 position should be maintained after prepend"
    );

    app.stop();
}

#[test]
fn prepend_respects_min_index_for_visible() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    const items = signal([10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20]);
    const prepend = () => {
        const current = items();
        items.set([current[0] - 1, ...current]);
    };
    return (
        <View style={{ width: 390, height: 844 }}>
            <List
                each={items()}
                style={{ width: 390, height: 400 }}
                maintainVisibleContentPosition={{
                    minIndexForVisible: 5
                }}
            >{(item) => (
                <View style={{ height: 50 }} testID={`item-${item()}`}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
            <View onPress={prepend} testID="prepend">
                <Text>Prepend</Text>
            </View>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    assert!(app.wait_for("Item 10", Duration::from_secs(1)));

    let list_id = {
        let screen = app.screen();
        screen
            .views()
            .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
            .expect("list")
    };

    // Scroll down so we're viewing items starting at index 2 (item 12)
    // Since we're prepending at position 0, and 0 <= minIndexForVisible (5),
    // position WILL be maintained (offset increases by 50px)
    app.scroll(list_id, 0.0, 100.0);
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    let offset_before = app
        .screen()
        .view(list_id)
        .map(|v| v.scroll_y)
        .unwrap_or(0.0);

    // Prepend an item
    assert!(app.press_text("Prepend"));
    assert!(app.wait_for("Item 9", Duration::from_secs(1)));

    // Offset should increase by 50px to maintain visible content
    // (because splice at=0 is <= minIndexForVisible=5)
    let offset_after = app
        .screen()
        .view(list_id)
        .map(|v| v.scroll_y)
        .unwrap_or(0.0);

    assert!(
        (offset_after - offset_before - 50.0).abs() < 5.0,
        "Offset should increase by 50px to maintain position: before={}, after={}",
        offset_before,
        offset_after
    );

    app.stop();
}

#[test]
fn autoscroll_to_bottom_when_near_end() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    const items = signal([5, 6, 7, 8, 9, 10, 11, 12, 13, 14]);
    const prepend = () => {
        const current = items();
        items.set([current[0] - 1, ...current]);
    };
    return (
        <View style={{ width: 390, height: 844 }}>
            <List
                each={items()}
                style={{ width: 390, height: 400 }}
                maintainVisibleContentPosition={{
                    minIndexForVisible: 0,
                    autoscrollToBottomThreshold: 100
                }}
            >{(item) => (
                <View style={{ height: 50 }} testID={`item-${item()}`}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
            <View onPress={prepend} testID="prepend">
                <Text>Prepend</Text>
            </View>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    assert!(app.wait_for("Item 5", Duration::from_secs(1)));

    let list_id = {
        let screen = app.screen();
        screen
            .views()
            .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
            .expect("list")
    };

    // Scroll to near the bottom (within 100px threshold)
    // 10 items × 50px = 500px content, viewport 400px, so max offset = 100px
    // Scroll to offset 80 (within 100px of bottom: 500 - 400 - 80 = 20px from bottom)
    app.scroll(list_id, 0.0, 80.0);
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    // Prepend an item (item 4)
    assert!(app.press_text("Prepend"));
    assert!(app.wait_for("Item 4", Duration::from_secs(1)));

    // Should auto-scroll to bottom (new content: 11 items × 50 = 550, viewport 400, max offset 150)
    let offset_after = app
        .screen()
        .view(list_id)
        .map(|v| v.scroll_y)
        .unwrap_or(0.0);

    assert!(
        offset_after >= 140.0,
        "Should auto-scroll to near bottom after prepend: offset={}, expected ~150",
        offset_after
    );
}

#[test]
fn no_autoscroll_when_not_near_bottom() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    const items = signal([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    const prepend = () => {
        const current = items();
        items.set([current[0] - 1, ...current]);
    };
    return (
        <View style={{ width: 390, height: 844 }}>
            <List
                each={items()}
                style={{ width: 390, height: 400 }}
                maintainVisibleContentPosition={{
                    minIndexForVisible: 0,
                    autoscrollToBottomThreshold: 50
                }}
            >{(item) => (
                <View style={{ height: 50 }} testID={`item-${item()}`}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
            <View onPress={prepend} testID="prepend">
                <Text>Prepend</Text>
            </View>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    let list_id = {
        let screen = app.screen();
        screen
            .views()
            .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
            .expect("list")
    };

    // Scroll to middle (not near bottom - threshold is 50px)
    // 16 items × 50px = 800px, viewport 400px, max offset 400px
    // Scroll to 200px (200px from bottom, > 50px threshold)
    app.scroll(list_id, 0.0, 200.0);
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    let offset_before = app
        .screen()
        .view(list_id)
        .map(|v| v.scroll_y)
        .unwrap_or(0.0);

    // Prepend an item
    assert!(app.press_text("Prepend"));
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    // Should maintain visible content, not auto-scroll to bottom
    let offset_after = app
        .screen()
        .view(list_id)
        .map(|v| v.scroll_y)
        .unwrap_or(0.0);

    // Offset should increase by 50px (one item prepended) to maintain visible content
    assert!(
        (offset_after - offset_before - 50.0).abs() < 5.0,
        "Should maintain visible content, not auto-scroll: before={}, after={}, delta={}",
        offset_before,
        offset_after,
        offset_after - offset_before
    );

    app.stop();
}

#[test]
fn prepend_at_top_maintains_first_item() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    const items = signal([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    const prepend = () => {
        const current = items();
        items.set([current[0] - 1, ...current]);
    };
    return (
        <View style={{ width: 390, height: 844 }}>
            <List
                each={items()}
                style={{ width: 390, height: 400 }}
                maintainVisibleContentPosition={{
                    minIndexForVisible: 0
                }}
            >{(item) => (
                <View style={{ height: 50 }} testID={`item-${item()}`}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
            <View onPress={prepend} testID="prepend">
                <Text>Prepend</Text>
            </View>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    // At top (offset 0), item 0 is visible
    let screen = app.screen();
    let item_0_before = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(id, _)| screen.frame(id))
        .expect("item-0 before prepend");

    assert_eq!(
        item_0_before.origin.y.0, 0.0,
        "Item 0 should be at top initially"
    );

    // Prepend item -1
    assert!(app.press_text("Prepend"));
    assert!(app.wait_for("Item -1", Duration::from_secs(1)));

    // Item 0 should still be visible at same position (maintained)
    let screen = app.screen();
    let item_0_after = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(id, _)| screen.frame(id))
        .expect("item-0 after prepend");

    assert_eq!(
        item_0_before.origin.y.0, item_0_after.origin.y.0,
        "Item 0 should maintain its position after prepend"
    );

    app.stop();
}
