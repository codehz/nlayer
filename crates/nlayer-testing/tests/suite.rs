//! `nlayer test`: a test file opens the app and asserts its text and layout.

use std::fs;

use nlayer_testing::run;

fn app(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nlayer-suite-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for (path, source) in files {
        fs::write(dir.join(path), source).unwrap();
    }
    dir
}

const APP: &str = r#"
import { mount, signal } from "@nlayer/runtime";
import { Input, Text, View } from "@nlayer/ui";

function App() {
    const count = signal(0);
    const name = signal("");
    return (
        <View style={{ width: 200, height: 80 }} onPress={() => count.set(count() + 1)}>
            <Text>Count: {count()}</Text>
            <Input value={name()} placeholder="Name" onChange={(value) => name.set(value)} />
        </View>
    );
}
mount(App);
"#;

#[test]
fn a_file_asserts_text_layout_and_input() {
    let dir = app(
        "pass",
        &[
            ("App.tsx", APP),
            (
                "App.test.tsx",
                r#"
import { expect, open, test } from "@nlayer/test";

test("a press updates the text and the frame", () => {
    const app = open("./App.tsx", { width: 200, height: 80 });
    expect(app.texts()).toEqual(["Count: 0"]);
    const tree = app.tree();
    expect(tree.children.length).toBe(1);
    const view = tree.children[0];
    expect(view.kind).toBe("view");
    expect(view.frame).toEqual({ x: 0, y: 0, width: 200, height: 80 });
    expect(view.children[0].text).toBe("Count: 0");
    expect(view.children[0].frame.x).toBe(0);
    app.press("Count: 0");
    expect(app.texts()).toEqual(["Count: 1"]);
    app.type("Name", "ada");
    expect(app.inputs()).toEqual(["ada"]);
});

test("a missing view fails the step", () => {
    const app = open("./App.tsx");
    expect(() => app.press("nope")).toThrow("nope");
});
"#,
            ),
        ],
    );
    let report = run(&dir, None).expect("the file runs");
    assert!(report.ok(), "{report}");
    assert_eq!(report.passed(), 2);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_assertion_is_reported_and_the_rest_still_run() {
    let dir = app(
        "fail",
        &[
            ("App.tsx", APP),
            (
                "App.test.tsx",
                r#"
import { expect, open, test } from "@nlayer/test";

test("wrong", () => {
    const app = open("./App.tsx");
    expect(app.texts()).toEqual(["nope"]);
});

test("right", () => {
    const app = open("./App.tsx");
    expect(app.texts()).toContain("Count: 0");
});
"#,
            ),
        ],
    );
    let report = run(&dir, None).expect("the file runs");
    assert!(!report.ok(), "{report}");
    assert_eq!(report.failures(), 1);
    assert_eq!(report.passed(), 1);
    let failed = &report.files[0].cases[0];
    assert_eq!(failed.name, "wrong");
    let message = failed.error.as_deref().expect("a failure");
    assert!(message.contains("nope"), "{message}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_filter_skips_other_tests() {
    let dir = app(
        "filter",
        &[
            ("App.tsx", APP),
            (
                "Only.test.ts",
                r#"
import { expect, test } from "@nlayer/test";

test("kept", () => {
    expect(1).toBe(1);
});

test("dropped", () => {
    throw new Error("ran");
});
"#,
            ),
        ],
    );
    let report = run(&dir, Some("kept")).expect("the file runs");
    assert!(report.ok(), "{report}");
    assert_eq!(report.passed(), 1);
    assert_eq!(report.files[0].cases[0].name, "kept");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_scroll_selector_moves_the_content_offset() {
    let dir = app(
        "scroll",
        &[
            (
                "App.tsx",
                r#"
import { mount } from "@nlayer/runtime";
import { Scroll, View } from "@nlayer/ui";

function App() {
    return (
        <Scroll testID="list" style={{ width: 100, height: 40 }}>
            <View style={{ width: 100, height: 200 }} />
        </Scroll>
    );
}
mount(App);
"#,
            ),
            (
                "App.test.tsx",
                r#"
import { expect, open, test } from "@nlayer/test";

test("scroll moves the offset toward the content", () => {
    const app = open("./App.tsx", { width: 100, height: 80 });
    app.scroll("list", 30);
    const scroll = app.tree().children[0];
    expect(scroll.kind).toBe("scroll");
    expect(scroll.testID).toBe("list");
    expect(scroll.offset).toEqual({ x: 0, y: 30 });
});
"#,
            ),
        ],
    );
    let report = run(&dir, None).expect("the file runs");
    assert!(report.ok(), "{report}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn type_focuses_the_field_and_dismiss_clears_it() {
    let dir = app(
        "focus",
        &[
            (
                "App.tsx",
                r#"
import { mount, signal } from "@nlayer/runtime";
import { Input, Keyboard, Text, View } from "@nlayer/ui";

function App() {
    const name = signal("");
    return (
        <View style={{ width: 200, height: 80 }}>
            <Input testID="name" value={name()} placeholder="Name" onChange={(value) => name.set(value)} />
            <Text testID="done" onPress={() => Keyboard.dismiss()}>done</Text>
        </View>
    );
}
mount(App);
"#,
            ),
            (
                "App.test.tsx",
                r#"
import { expect, open, test } from "@nlayer/test";

function field(node) {
    if (node.kind === "input") return node;
    for (const child of node.children ?? []) {
        const found = field(child);
        if (found) return found;
    }
    return null;
}

test("type focuses the input and dismiss clears it", () => {
    const app = open("./App.tsx", { width: 200, height: 80 });
    expect(field(app.tree()).focused).toBe(false);
    app.type("name", "a");
    expect(field(app.tree()).focused).toBe(true);
    expect(field(app.tree()).value).toBe("a");
    app.press("done");
    expect(field(app.tree()).focused).toBe(false);
    expect(field(app.tree()).value).toBe("a");
});
"#,
            ),
        ],
    );
    let report = run(&dir, None).expect("the file runs");
    assert!(report.ok(), "{report}");
    let _ = fs::remove_dir_all(&dir);
}
