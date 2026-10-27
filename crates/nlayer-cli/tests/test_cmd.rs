//! `nlayer test` runs a test file.

use std::fs;

use nlayer_cli::{TestOptions, test};

#[test]
fn the_command_runs_a_test_file() {
    let dir = std::env::temp_dir().join(format!("nlayer-cli-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("App.tsx"),
        r#"
import { mount } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    return <View style={{ width: 10, height: 10 }}><Text>hello</Text></View>;
}
mount(App);
"#,
    )
    .unwrap();
    fs::write(
        dir.join("App.test.tsx"),
        r#"
import { expect, open, test } from "@nlayer/test";

test("shows hello", () => {
    const app = open("./App.tsx");
    expect(app.texts()).toEqual(["hello"]);
    expect(app.tree().children[0].children[0].kind).toBe("text");
});
"#,
    )
    .unwrap();
    let report = test(&TestOptions {
        path: dir.join("App.test.tsx"),
        filter: None,
    })
    .expect("it runs");
    assert!(report.ok(), "{report}");
    assert_eq!(report.files[0].name, "App.test.tsx");
    let _ = fs::remove_dir_all(&dir);
}
