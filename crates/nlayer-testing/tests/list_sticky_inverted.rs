//! Test sticky headers and inverted lists with numeric frame assertions.

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
    let item0_frame = screen.views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(id, _)| screen.frame(id))
        .expect("item-0 frame");
    
    assert_eq!(item0_frame.origin.y.0, 0.0, "sticky item 0 at offset=0: y=0");
    
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
    let list_id = screen.views()
        .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
        .expect("list");
    
    
    // Scroll to 100: item 0 (at=0, next=250) -> shift=100, y=100
    app.scroll(list_id, 0.0, 100.0);
    // Wait for content to update
    std::thread::sleep(Duration::from_millis(100));
    
    
    
    let screen = app.screen();
    let item0_container_frame = screen.views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-0 container");
    
    assert_eq!(item0_container_frame.origin.y.0, 100.0, 
        "sticky item 0 at offset=100: shift=(100-0).clamp(0,250)=100");
    
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
    let list_id = screen.views()
        .find_map(|(id, v)| (v.kind == ViewKind::Scroll).then_some(id))
        .expect("list");
    drop(screen);
    
    // Scroll to 300: with 20 items (1000pt content, 400pt viewport, max offset 600)
    // shift = (300-0).clamp(0, 250) = 250
    app.scroll(list_id, 0.0, 300.0);
    assert!(app.wait_for("Item 5", Duration::from_millis(300)));
    
    let screen = app.screen();
    let item0_container_frame = screen.views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-0 container");
    
    assert_eq!(item0_container_frame.origin.y.0, 250.0,
        "sticky item 0 at offset=300: shift clamped at 250");
    
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
    
    // Total=250, item0: at=0 -> 250-0-50=200, item4: at=200 -> 250-200-50=0
    // The user's View has testID, but its frame is relative to the cell container.
    let item0_container_frame = screen.views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-0 container");
    let item4_container_frame = screen.views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-4"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-4 container");
    
    assert_eq!(item0_container_frame.origin.y.0, 200.0, "inverted item 0: 250-0-50=200");
    assert_eq!(item4_container_frame.origin.y.0, 0.0, "inverted item 4: 250-200-50=0");
    
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
    let item0_container_frame = screen.views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .expect("item-0 container");
    
    assert_eq!(item0_container_frame.origin.y.0, 200.0, "inverted+sticky: sticky ignored, y=200");
    
    app.stop();
}
