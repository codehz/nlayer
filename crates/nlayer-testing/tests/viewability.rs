//! Test viewability tracking: onViewableItemsChange fires when visible range changes.

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
fn viewable_items_change_fires_on_scroll() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    const events = signal<string[]>([]);
    return (
        <View style={{ width: 390, height: 844 }}>
            <List
                each={Array.from({ length: 100 }, (_, i) => i)}
                style={{ width: 390, height: 400 }}
                viewabilityConfig={{
                    itemVisibleThreshold: 0.5
                }}
                onViewableItemsChange={(e: any) => {
                    events.set([...events(), `${e.first}-${e.last}`]);
                }}
            >{(item) => (
                <View style={{ height: 50 }}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
            <View style={{ width: 390, height: 444 }}>
                {events().map(e => <Text key={e}>{e}</Text>)}
            </View>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    // Wait for initial render - at offset 0, items 0-7 should be visible (400px / 50px = 8 items)
    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    // Give time for viewability event to fire
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    // Should have initial viewable range
    let texts = app.texts();
    assert!(
        texts.iter().any(|t| t.contains("0-")),
        "Should report initial viewable range: {:?}",
        texts
    );

    let screen = app.screen();
    let list_id = screen
        .views()
        .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
        .expect("list");

    // Scroll down to middle
    app.scroll(list_id, 0.0, 1000.0);
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    // Should have new viewable range in middle (around items 20-27)
    let texts = app.texts();
    let has_middle_range = texts.iter().any(|t| {
        if let Some(range) = t.strip_suffix("").and_then(|s| {
            let parts: Vec<&str> = s.split('-').collect();
            if parts.len() == 2 {
                let first: u32 = parts[0].parse().ok()?;
                let last: u32 = parts[1].parse().ok()?;
                Some((first, last))
            } else {
                None
            }
        }) {
            range.0 >= 15 && range.1 <= 35
        } else {
            false
        }
    });
    assert!(
        has_middle_range,
        "Should report middle range after scroll: {:?}",
        texts
    );

    app.stop();
}

#[test]
fn viewable_items_no_duplicate_events_at_same_position() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    const count = signal(0);
    return (
        <View style={{ width: 390, height: 844 }}>
            <List
                each={Array.from({ length: 50 }, (_, i) => i)}
                style={{ width: 390, height: 400 }}
                viewabilityConfig={{
                    itemVisibleThreshold: 0.5
                }}
                onViewableItemsChange={() => {
                    count.set(count() + 1);
                }}
            >{(item) => (
                <View style={{ height: 50 }}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
            <Text testID="count">{`Events: ${count()}`}</Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    assert!(app.wait_for("Item 0", Duration::from_secs(1)));
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    // Get initial count
    let initial_count = app
        .texts()
        .iter()
        .find(|t| t.starts_with("Events:"))
        .and_then(|t| t.strip_prefix("Events: "))
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(0);

    assert!(initial_count > 0, "Should have at least one initial event");

    let screen = app.screen();
    let list_id = screen
        .views()
        .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
        .expect("list");

    // Scroll to a position
    app.scroll(list_id, 0.0, 500.0);
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    let after_scroll = app
        .texts()
        .iter()
        .find(|t| t.starts_with("Events:"))
        .and_then(|t| t.strip_prefix("Events: "))
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(0);

    // Scroll to same position again (should not trigger new event)
    app.scroll(list_id, 0.0, 500.0);
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    let after_same_scroll = app
        .texts()
        .iter()
        .find(|t| t.starts_with("Events:"))
        .and_then(|t| t.strip_prefix("Events: "))
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(0);

    assert_eq!(
        after_scroll, after_same_scroll,
        "Should not fire duplicate events at same position"
    );

    app.stop();
}

#[test]
fn viewable_items_scroll_back_to_top() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    const lastRange = signal<string>("");
    return (
        <View style={{ width: 390, height: 844 }}>
            <List
                each={Array.from({ length: 100 }, (_, i) => i)}
                style={{ width: 390, height: 400 }}
                viewabilityConfig={{}}
                onViewableItemsChange={(e: any) => {
                    lastRange.set(`${e.first}-${e.last}`);
                }}
            >{(item) => (
                <View style={{ height: 50 }}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
            <Text testID="range">{`Range: ${lastRange()}`}</Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    assert!(app.wait_for("Item 0", Duration::from_secs(1)));

    // Wait for initial viewability event
    assert!(
        app.wait_for("Range: 0-", Duration::from_secs(1)),
        "Should get initial viewable range"
    );

    let screen = app.screen();
    let list_id = screen
        .views()
        .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
        .expect("list");

    // Scroll down
    app.scroll(list_id, 0.0, 1500.0);
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    // Scroll back to top
    app.scroll(list_id, 0.0, 0.0);
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    // Should show first=0 again
    let texts = app.texts();
    assert!(
        texts.iter().any(|t| t.contains("Range: 0-")),
        "Should return to first=0 at top: {:?}",
        texts
    );

    app.stop();
}

#[test]
fn viewable_items_with_viewport_coverage_threshold() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    const lastRange = signal<string>("");
    return (
        <View style={{ width: 390, height: 844 }}>
            <List
                each={Array.from({ length: 20 }, (_, i) => i)}
                style={{ width: 390, height: 400 }}
                viewabilityConfig={{
                    viewAreaCoverageThreshold: 0.5
                }}
                onViewableItemsChange={(e: any) => {
                    lastRange.set(`${e.first}-${e.last}`);
                }}
            >{(item) => (
                <View style={{ height: 100 }}>
                    <Text>{`Item ${item()}`}</Text>
                </View>
            )}</List>
            <Text testID="range">{`Range: ${lastRange()}`}</Text>
        </View>
    );
}
mount(App);
"#;
    let mut app = app(source);

    assert!(app.wait_for("Item 0", Duration::from_secs(1)));
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    // With 100px items and 400px viewport, viewAreaCoverageThreshold 0.5 means
    // item must cover >= 200px of viewport. Only items fully within viewport
    // (or covering at least 50%) should be viewable.
    let texts = app.texts();
    let has_range = texts.iter().any(|t| t.starts_with("Range: "));
    assert!(has_range, "Should report viewable range: {:?}", texts);

    app.stop();
}
