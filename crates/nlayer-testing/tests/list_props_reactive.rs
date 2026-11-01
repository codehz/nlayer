//! Test that List second-layer props are reactive (not just mount-time).

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
fn sticky_header_indices_reactive() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, View, Text } from "@nlayer/ui";

const stickyEnabled = signal(false);

function App() {
  return (
    <View style={{ width: 390, height: 400 }}>
      <View style={{ height: 50 }}>
        <Text testID="toggle" onPress={() => stickyEnabled.set(!stickyEnabled())}>
          Sticky: {stickyEnabled() ? "on" : "off"}
        </Text>
      </View>
      <List
        testID="list"
        each={Array.from({ length: 30 }, (_, i) => i)}
        style={{ width: 390, height: 350 }}
        estimatedItemSize={50}
        stickyHeaderIndices={stickyEnabled() ? [0, 10, 20] : undefined}
      >{(item) => (
        <View testID={item() === 0 ? "item-0" : undefined} style={{ height: 50 }}>
          <Text>Row {item()}</Text>
        </View>
      )}</List>
    </View>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Sticky: off", Duration::from_secs(1)));

    let screen = app.screen();
    let list_id = screen
        .views()
        .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
        .expect("list");

    // Initially sticky is off, scroll to 100
    app.scroll(list_id, 0.0, 100.0);
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    let screen = app.screen();
    let item0_container = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-0 container");

    // Without sticky, item 0 container should be at offset 0 (not pinned, so y=0 relative to scroll)
    assert_eq!(
        item0_container.origin.y.0, 0.0,
        "Without sticky at offset=100, item-0 container should not be shifted"
    );

    // Now toggle sticky ON via signal
    assert!(app.press_text("Sticky: off"));
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    assert!(app.wait_for("Sticky: on", Duration::from_millis(500)));

    // Scroll is still at 100, now with sticky enabled
    let screen = app.screen();
    let item0_container = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-0 container after toggle");

    // With sticky enabled, item 0 should be pinned at y=100 (shift = offset - item_offset)
    assert_eq!(
        item0_container.origin.y.0, 100.0,
        "With sticky enabled at offset=100, item-0 container should be pinned at y=100"
    );

    app.stop();
}

#[test]
fn inverted_reactive() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, View, Text } from "@nlayer/ui";

const invertedEnabled = signal(false);

function App() {
  return (
    <View style={{ width: 390, height: 400 }}>
      <View style={{ height: 50 }}>
        <Text testID="toggle" onPress={() => invertedEnabled.set(!invertedEnabled())}>
          Inverted: {invertedEnabled() ? "on" : "off"}
        </Text>
      </View>
      <List
        testID="list"
        each={[0, 1, 2]}
        style={{ width: 390, height: 350 }}
        estimatedItemSize={50}
        inverted={invertedEnabled()}
      >{(item) => (
        <View testID={`item-${item()}`} style={{ height: 50 }}>
          <Text>Row {item()}</Text>
        </View>
      )}</List>
    </View>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Inverted: off", Duration::from_secs(1)));
    assert!(app.wait_for("Row 0", Duration::from_millis(500)));

    // Get baseline positions before toggle (normal order: item-0 above item-2)
    let screen = app.screen();
    let item0_y_before = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .map(|f| f.origin.y.0)
        .expect("item-0 before toggle");
    let item2_y_before = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-2"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .map(|f| f.origin.y.0)
        .expect("item-2 before toggle");

    assert!(
        item0_y_before < item2_y_before,
        "Before inverted: item-0 should be above item-2"
    );

    // Toggle inverted ON via signal
    assert!(app.press_text("Inverted: off"));
    std::thread::sleep(Duration::from_millis(200));
    app.refresh();

    assert!(app.wait_for("Inverted: on", Duration::from_millis(500)));

    // After toggle: inverted order (item-0 below item-2)
    let screen = app.screen();
    let item0_y_after = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .map(|f| f.origin.y.0)
        .expect("item-0 after toggle");
    let item2_y_after = screen
        .views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-2"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .map(|f| f.origin.y.0)
        .expect("item-2 after toggle");

    assert!(
        item0_y_after > item2_y_after,
        "After inverted toggle: item-0 ({}) should be below item-2 ({})",
        item0_y_after,
        item2_y_after
    );

    app.stop();
}

#[test]
fn snap_to_interval_reactive() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, View, Text } from "@nlayer/ui";

const snapEnabled = signal(false);

function App() {
  return (
    <View style={{ width: 390, height: 400 }}>
      <View style={{ height: 50 }}>
        <Text testID="toggle" onPress={() => snapEnabled.set(!snapEnabled())}>
          Snap: {snapEnabled() ? "on" : "off"}
        </Text>
      </View>
      <List
        testID="list"
        each={Array.from({ length: 30 }, (_, i) => i)}
        style={{ width: 390, height: 350 }}
        estimatedItemSize={50}
        snapToInterval={snapEnabled() ? 50 : undefined}
      >{(item) => (
        <View style={{ height: 50 }}>
          <Text>Row {item()}</Text>
        </View>
      )}</List>
    </View>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Snap: off", Duration::from_secs(1)));

    // Toggle snap ON via signal
    assert!(app.press_text("Snap: off"));
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    assert!(app.wait_for("Snap: on", Duration::from_millis(500)));

    // The prop was set reactively (no crash, no warning)
    app.stop();
}

#[test]
fn refreshing_reactive() {
    let source = r#"
import { mount, signal } from "@nlayer/runtime";
import { List, View, Text } from "@nlayer/ui";

const refreshing = signal(false);

function App() {
  return (
    <View style={{ width: 390, height: 400 }}>
      <View style={{ height: 50 }}>
        <Text testID="toggle" onPress={() => refreshing.set(!refreshing())}>
          Refreshing: {refreshing() ? "on" : "off"}
        </Text>
      </View>
      <List
        testID="list"
        each={[0, 1, 2]}
        style={{ width: 390, height: 350 }}
        estimatedItemSize={50}
        refreshing={refreshing()}
      >{(item) => (
        <View style={{ height: 50 }}>
          <Text>Row {item()}</Text>
        </View>
      )}</List>
    </View>
  );
}
mount(App);
    "#;
    let mut app = app(source);
    assert!(app.wait_for("Refreshing: off", Duration::from_secs(1)));

    // Toggle refreshing ON via signal
    assert!(app.press_text("Refreshing: off"));
    std::thread::sleep(Duration::from_millis(100));
    app.refresh();

    assert!(app.wait_for("Refreshing: on", Duration::from_millis(500)));

    // The prop was set reactively (no crash, no warning)
    app.stop();
}
