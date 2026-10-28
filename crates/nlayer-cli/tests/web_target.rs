//! `nlayer test --target web` drives the opened app in a headless browser.

use std::fs;

use nlayer_cli::{TestOptions, TestTarget, test};

#[test]
fn the_browser_presses_types_and_scrolls() {
    let dir = std::env::temp_dir().join(format!("nlayer-web-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("App.tsx"), APP).unwrap();
    fs::write(dir.join("App.test.tsx"), TEST).unwrap();
    let report = test(&TestOptions {
        path: dir.join("App.test.tsx"),
        filter: None,
        target: Some(TestTarget::Web),
    })
    .expect("the browser runs");
    assert!(report.ok(), "{report}");
    let _ = fs::remove_dir_all(&dir);
}

const APP: &str = r#"
import { mount, signal } from "@nlayer/runtime";
import { Input, Scroll, Text, View } from "@nlayer/ui";

function App() {
    const count = signal(0);
    const name = signal("");
    const note = signal("idle");
    return (
        <View style={{ width: 200, height: 220 }}>
            <View testID="tap" onPress={() => count.set(count() + 1)} style={{ height: 24 }}>
                <Text>Count: {count()}</Text>
            </View>
            <Input
                testID="name"
                value={name()}
                placeholder="Name"
                onChange={(value) => name.set(value)}
                style={{ height: 24 }}
            />
            <Scroll testID="list" style={{ width: 200, height: 40 }}>
                <View style={{ width: 200, height: 200 }} />
            </Scroll>
            <Text>
                see <Text testID="docs" onPress={() => note.set("opened")}>docs</Text>
            </Text>
            <Text>{note()}</Text>
            <View testID="pad" style={{ paddingTop: "env(safe-area-inset-top)", width: 80, height: 40 }}>
                <Text>pad</Text>
            </View>
        </View>
    );
}
mount(App);
"#;

const TEST: &str = r#"
import { expect, open, test } from "@nlayer/test";

function byId(node, id) {
    if (node.testID === id) return node;
    for (const child of node.children) {
        const found = byId(child, id);
        if (found) return found;
    }
    return null;
}

test("the page takes a press, keys, a scroll, and the pinned inset", () => {
    const app = open("./App.tsx", {
        width: 200,
        height: 220,
        safeArea: { top: 12, right: 0, bottom: 0, left: 0 },
    });
    expect(app.texts()).toContain("Count: 0");
    app.press("Count: 0");
    expect(app.texts()).toContain("Count: 1");
    app.type("Name", "ada");
    expect(app.inputs()).toEqual(["ada"]);
    app.scroll("list", 30);
    const scroll = byId(app.tree(), "list");
    expect(scroll.kind).toBe("scroll");
    expect(scroll.offset).toEqual({ x: 0, y: 30 });
    app.press("id:docs");
    expect(app.texts()).toContain("opened");
    const pad = byId(app.tree(), "pad");
    expect(pad.children[0].frame.y).toBe(12);
});
"#;
