//! The `nlayer` binary: `--filter`, the steps on standard input, and a type
//! package warning that does not fail the command.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const APP: &str = r#"
import { mount, signal } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    const count = signal(0);
    return (
        <View style={{ width: 80, height: 40 }} onPress={() => count.set(count() + 1)}>
            <Text>Count: {count()}</Text>
        </View>
    );
}
mount(App);
"#;

const TESTS: &str = r#"
import { expect, open, test } from "@nlayer/test";

test("alpha shows the count", () => {
    const app = open("./App.tsx");
    expect(app.texts()).toEqual(["Count: 0"]);
});

test("beta shows the count", () => {
    const app = open("./App.tsx");
    expect(app.texts()).toEqual(["Count: 0"]);
});
"#;

fn scratch(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    let dir =
        std::env::temp_dir().join(format!("nlayer-cli-{label}-{}-{nanos}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("App.tsx"), APP).unwrap();
    dir
}

fn run(path: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nlayer"));
    command.arg("test").arg(path).args(args);
    command.output().expect("nlayer test")
}

fn steps(entry: &Path, steps: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nlayer"))
        .arg("test")
        .arg(entry)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn nlayer test");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(steps.as_bytes())
        .expect("write the steps");
    child.wait_with_output().expect("nlayer test")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn filter_runs_only_matching_tests() {
    let dir = scratch("filter");
    fs::write(dir.join("App.test.tsx"), TESTS).unwrap();

    let all = run(&dir, &[]);
    assert!(all.status.success(), "{}", text(&all.stderr));
    assert!(text(&all.stdout).contains("2 passed, 0 failed"));

    let one = run(&dir, &["--filter", "alpha"]);
    let stdout = text(&one.stdout);
    assert!(one.status.success(), "{stdout}{}", text(&one.stderr));
    assert!(stdout.contains("alpha shows the count"), "{stdout}");
    assert!(!stdout.contains("beta shows the count"), "{stdout}");
    assert!(stdout.contains("1 passed, 0 failed"), "{stdout}");

    let none = run(&dir, &["--filter", "nothing matches"]);
    let stdout = text(&none.stdout);
    assert!(none.status.success(), "{stdout}{}", text(&none.stderr));
    assert!(stdout.contains("no tests matched"), "{stdout}");
    assert!(stdout.contains("0 passed, 0 failed"), "{stdout}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn standard_input_steps_drive_the_opened_app() {
    let dir = scratch("stdin");
    let entry = dir.join("App.tsx");

    let output = steps(
        &entry,
        "app.press(\"Count: 0\");\nexpect(app.texts()).toEqual([\"Count: 1\"]);\n",
    );
    let stdout = text(&output.stdout);
    assert!(output.status.success(), "{stdout}{}", text(&output.stderr));
    assert!(stdout.contains("ok   App.tsx  steps"), "{stdout}");
    assert!(stdout.contains("1 passed, 0 failed"), "{stdout}");

    let output = steps(&entry, "expect(app.texts()).toEqual([\"missing\"]);\n");
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("fail App.tsx  steps"), "{stdout}");
    assert!(stdout.contains("0 passed, 1 failed"), "{stdout}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_type_package_mismatch_warns_and_exits_zero() {
    let dir = scratch("types");
    fs::write(dir.join("App.test.tsx"), TESTS).unwrap();
    let package = dir.join("node_modules/@nlayer/runtime");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("package.json"), r#"{"version":"9.9.9"}"#).unwrap();

    let output = run(&dir, &[]);
    let stderr = text(&output.stderr);
    let stdout = text(&output.stdout);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(stderr.contains("@nlayer/runtime 9.9.9"), "{stderr}");
    assert!(
        stderr.contains("editor types may not match this binary"),
        "{stderr}"
    );
    assert!(stdout.contains("2 passed, 0 failed"), "{stdout}");

    let _ = fs::remove_dir_all(&dir);
}
