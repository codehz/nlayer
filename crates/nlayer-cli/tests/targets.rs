//! `nlayer test --target` on the three hosts, plus gallery on each.
//!
//! One binary, one lock: the simulator and the emulator cannot be driven
//! twice at once, and `cargo test` would otherwise run these in parallel.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nlayer_cli::{TestOptions, TestTarget, test};

const APP: &str = include_str!("fixtures/App.tsx");
const TEST: &str = include_str!("fixtures/App.test.tsx");

fn lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn run(path: &Path, target: Option<TestTarget>) -> nlayer_testing::Report {
    test(&TestOptions {
        path: path.to_path_buf(),
        filter: None,
        target,
    })
    .unwrap_or_else(|error| panic!("{target:?}: {error}"))
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nlayer-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("App.tsx"), APP).unwrap();
    fs::write(dir.join("App.test.tsx"), TEST).unwrap();
    dir
}

fn assert_small(target: Option<TestTarget>, label: &str) {
    let _guard = lock();
    let dir = scratch(label);
    let report = run(&dir.join("App.test.tsx"), target);
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "{target:?}: {report}");
}

fn gallery() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/gallery/App.test.tsx")
}

#[test]
fn the_browser_presses_types_and_scrolls() {
    assert_small(Some(TestTarget::Web), "web");
}

#[test]
fn the_simulator_presses_types_and_scrolls() {
    assert_small(Some(TestTarget::Ios), "ios");
}

#[test]
fn the_emulator_presses_types_and_scrolls() {
    assert_small(Some(TestTarget::Android), "android");
}

#[test]
fn gallery_runs_on_each_target() {
    let _guard = lock();
    let path = gallery();
    assert!(path.is_file(), "missing {}", path.display());
    for target in [
        None,
        Some(TestTarget::Web),
        Some(TestTarget::Ios),
        Some(TestTarget::Android),
    ] {
        let report = run(&path, target);
        assert!(report.ok(), "{target:?}: {report}");
    }
}
