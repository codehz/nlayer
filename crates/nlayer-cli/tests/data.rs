//! `storage` of `@nlayer/runtime` through a headless app: writes, reads
//! and a restart that reads the entries back.

use std::{
    fs,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use nlayer_cli::{TestOptions, test};

/// The fixture file server is one root per process: these tests take turns.
static FIXTURE: Mutex<()> = Mutex::new(());

const APP: &str = r#"
import { mount, signal, storage } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    const draft = signal("loading");
    const quota = signal("");
    storage.getItem("draft").then((value) => draft.set(value === null ? "none" : value));
    return (
        <View style={{ flex: 1 }}>
            <Text>{draft()}</Text>
            <Text>{quota()}</Text>
            <View testID="reset" style={{ width: 80, height: 40 }} onPress={() => { storage.removeItem("draft").then(() => draft.set("none")); }}>
                <Text>Reset</Text>
            </View>
            <View testID="save" style={{ width: 80, height: 40 }} onPress={() => { storage.setItem("draft", "kept").then(() => draft.set("kept")); }}>
                <Text>Save</Text>
            </View>
            <View testID="full" style={{ width: 80, height: 40 }} onPress={() => {
                storage.setItem("quota", "x".repeat(5 * 1024 * 1024))
                    .then(() => quota.set("wrote"))
                    .catch((error) => quota.set(`${error.name} ${error.code}`));
            }}>
                <Text>Full</Text>
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
    first.waitFor("Reset");
    // A device keeps its entries between runs: start from no draft.
    first.press("id:reset");
    first.waitFor("none");
    first.press("id:save");
    first.waitFor("kept");
    first.close();

    const second = open("./App.tsx", { dataDir: "./data" });
    second.waitFor("kept");
    expect(second.texts()).toContain("kept");
    second.close();
});

test("a write past the quota rejects with the code", () => {
    const app = open("./App.tsx", { dataDir: "./data" });
    app.waitFor("Full");
    app.press("id:full");
    app.waitFor("QuotaExceededError ERR_NLAYER_QUOTA_EXCEEDED");
    app.close();
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
    let _fixture = FIXTURE.lock().unwrap_or_else(|e| e.into_inner());
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

const RESOURCE_APP: &str = r#"
import { mount, resource, signal } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

declare const __nlayerTestOrigin: string | undefined;

const id = signal(0);
const fail = signal(false);
const n = signal(1);
const aborts = signal(0);
const user = resource(id, (value, info) => {
    info.signal.addEventListener("abort", () => aborts.set(aborts() + 1));
    if (fail()) return Promise.reject(new Error("boom"));
    const origin = typeof __nlayerTestOrigin === "string" ? __nlayerTestOrigin : "";
    return fetch(`${origin}/quote.json`)
        .then((response) => response.json())
        .then((body) => `${body.message} ${n()}`);
});

function App() {
    return (
        <View style={{ flex: 1 }}>
            <Text>{`${user.state()} ${user() ?? "-"} aborts ${aborts()}`}</Text>
            <View testID="load" style={{ width: 80, height: 40 }} onPress={() => id.set(1)}>
                <Text>Load</Text>
            </View>
            <View testID="fail" style={{ width: 80, height: 40 }} onPress={() => { fail.set(true); user.refetch(); }}>
                <Text>Fail</Text>
            </View>
            <View testID="again" style={{ width: 80, height: 40 }} onPress={() => { fail.set(false); n.set(n() + 1); user.refetch(); }}>
                <Text>Again</Text>
            </View>
        </View>
    );
}
mount(App);
"#;

const RESOURCE_TESTS: &str = r#"
import { expect, open, test } from "@nlayer/test";

test("a resource loads, fails keeping the value, and refetches", () => {
    const app = open("./App.tsx");
    app.waitFor("unresolved - aborts 0");
    app.press("id:load");
    app.waitFor("ready hello 1 aborts 0");
    app.press("id:fail");
    app.waitFor("errored hello 1 aborts 0");
    app.press("id:again");
    app.waitFor("ready hello 2 aborts 0");
    expect(app.texts()).toContain("ready hello 2 aborts 0");
    app.close();
});
"#;

#[test]
fn resource_loads_reloads_and_reports_failures() {
    let _fixture = FIXTURE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("resource");
    fs::write(dir.join("App.tsx"), RESOURCE_APP).unwrap();
    fs::write(dir.join("App.test.tsx"), RESOURCE_TESTS).unwrap();
    fs::write(dir.join("quote.json"), r#"{"message":"hello"}"#).unwrap();
    let report = test(&TestOptions {
        path: dir.clone(),
        filter: None,
        target: None,
    })
    .expect("the test runs");
    assert!(report.ok(), "{report}");
    let _ = fs::remove_dir_all(&dir);
}
