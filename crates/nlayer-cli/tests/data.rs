//! `storage` of `@nlayer/runtime` through a headless app: writes, reads
//! and a restart that reads the entries back.

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use nlayer_cli::{TestOptions, test};

const APP: &str = r#"
import { mount, signal, storage } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    const draft = signal("loading");
    storage.getItem("draft").then((value) => draft.set(value === null ? "none" : value));
    return (
        <View style={{ flex: 1 }}>
            <Text>{draft()}</Text>
            <View testID="save" style={{ width: 80, height: 40 }} onPress={() => { storage.setItem("draft", "kept").then(() => draft.set("kept")); }}>
                <Text>Save</Text>
            </View>
        </View>
    );
}
mount(App);
"#;

const TESTS: &str = r#"
import { expect, open, test } from "@nlayer/test";

test("storage survives a restart", () => {
    const first = open("./App.tsx", { dataDir: "./data" });
    first.waitFor("Save");
    expect(first.texts()).toContain("none");
    first.press("id:save");
    first.waitFor("kept");
    first.close();

    const second = open("./App.tsx", { dataDir: "./data" });
    second.waitFor("kept");
    expect(second.texts()).toContain("kept");
    second.close();
});
"#;

fn scratch(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "nlayer-data-{label}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("App.tsx"), APP).unwrap();
    fs::write(dir.join("App.test.tsx"), TESTS).unwrap();
    dir
}

#[test]
fn storage_reads_back_after_a_restart() {
    let dir = scratch("storage");
    let report = test(&TestOptions {
        path: dir.clone(),
        filter: None,
        target: None,
    })
    .expect("the test runs");
    assert!(report.ok(), "{report}");
    assert!(
        dir.join("data/storage.json").exists(),
        "the entries are kept in the data directory"
    );
    let _ = fs::remove_dir_all(&dir);
}
