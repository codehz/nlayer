use nlayer_bundler::compile_module;
use nlayer_core::geometry::{Points, Size};
use nlayer_testing::Headless;
use std::time::Duration;
use nlayer_core::ViewKind;

fn app(source: &str) -> Headless {
    let bundle = compile_module("test.tsx", source).unwrap();
    let viewport = Size::new(Points(390.0), Points(844.0));
    Headless::start(&bundle, viewport).unwrap()
}

#[test]
fn sticky_at_mount() {
    // Test sticky headers when set at mount time (not toggled)
    let source = r#"
import { mount } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 390, height: 844 }}>
            <List
                testID="list"
                each={Array.from({ length: 30 }, (_, i) => i)}
                stickyHeaderIndices={[0]}
                style={{ width: 390, height: 800 }}
            >{(item, index) => (
                <View style={{ height: 50 }} testID={index() === 0 ? "item-0" : undefined}>
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
    
    // Scroll to 100
    app.scroll(list_id, 0.0, 100.0);
    
    let screen = app.screen();
    let item0_y = screen.views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .map(|f| f.origin.y.0)
        .expect("item-0 container");
    
    assert_eq!(item0_y, 100.0, 
        "With sticky at mount + scroll(100), item-0 should be pinned at y=100");
    
    app.stop();
}

#[test]
fn inverted_at_mount() {
    // Test inverted list when set at mount time (not toggled)
    let source = r#"
import { mount } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

function App() {
    return (
        <View style={{ width: 390, height: 844 }}>
            <List
                testID="list"
                each={[0, 1, 2, 3, 4]}
                inverted={true}
                style={{ width: 390, height: 200 }}
            >{(item) => (
                <View style={{ height: 32 }} testID={`item-${item()}`}>
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
    
    // With inverted: item-0 at bottom, item-4 at top
    let item0_y = screen.views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-0"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .map(|f| f.origin.y.0)
        .expect("item-0");
    let item4_y = screen.views()
        .find(|(_, v)| v.test_id.as_deref() == Some("item-4"))
        .and_then(|(_, v)| v.parent)
        .and_then(|parent_id| screen.frame(parent_id))
        .map(|f| f.origin.y.0)
        .expect("item-4");
    
    assert!(item0_y > item4_y, 
        "With inverted=true at mount, item-0 ({}) should be below item-4 ({})", 
        item0_y, item4_y);
    
    app.stop();
}
