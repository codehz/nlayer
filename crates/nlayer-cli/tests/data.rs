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

const APP: &str = include_str!("fixtures/data/App.tsx");

const TESTS: &str = include_str!("fixtures/data/App.test.tsx");

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
    fs::write(dir.join("quote.json"), "{\"message\":\"hello\"}\n").unwrap();
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

const RESOURCE_APP: &str = include_str!("fixtures/resource/App.tsx");

const RESOURCE_TESTS: &str = include_str!("fixtures/resource/App.test.tsx");

#[test]
fn resource_loads_reloads_and_reports_failures() {
    let _fixture = FIXTURE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("resource");
    fs::write(dir.join("App.tsx"), RESOURCE_APP).unwrap();
    fs::write(dir.join("App.test.tsx"), RESOURCE_TESTS).unwrap();
    let report = test(&TestOptions {
        path: dir.clone(),
        filter: None,
        target: None,
    })
    .expect("the test runs");
    assert!(report.ok(), "{report}");
    let _ = fs::remove_dir_all(&dir);
}

const ADAPT_APP: &str = include_str!("fixtures/adapt/App.tsx");

const ADAPT_TESTS: &str = include_str!("fixtures/adapt/App.test.tsx");

#[test]
fn the_scheme_and_accessibility_run_on_the_headless_engine() {
    let _fixture = FIXTURE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("adapt");
    fs::write(dir.join("App.tsx"), ADAPT_APP).unwrap();
    fs::write(dir.join("App.test.tsx"), ADAPT_TESTS).unwrap();
    let report = test(&TestOptions {
        path: dir.clone(),
        filter: None,
        target: None,
    })
    .expect("the test runs");
    assert!(report.ok(), "{report}");
    let _ = fs::remove_dir_all(&dir);
}

const INPUT_APP: &str = include_str!("fixtures/input/App.tsx");

const INPUT_TESTS: &str = include_str!("fixtures/input/App.test.tsx");

#[test]
fn a_multiline_input_reports_selection_and_composition() {
    let _fixture = FIXTURE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("input");
    fs::write(dir.join("App.tsx"), INPUT_APP).unwrap();
    fs::write(dir.join("App.test.tsx"), INPUT_TESTS).unwrap();
    let report = test(&TestOptions {
        path: dir.clone(),
        filter: None,
        target: None,
    })
    .expect("the test runs");
    assert!(report.ok(), "{report}");
    let _ = fs::remove_dir_all(&dir);
}

const LIST_APP: &str = include_str!("fixtures/list/App.tsx");

const LIST_TESTS: &str = include_str!("fixtures/list/App.test.tsx");

#[test]
fn a_long_list_renders_windows_and_scrolls_on_the_headless_engine() {
    let _fixture = FIXTURE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("list");
    fs::write(dir.join("App.tsx"), LIST_APP).unwrap();
    fs::write(dir.join("App.test.tsx"), LIST_TESTS).unwrap();
    let report = test(&TestOptions {
        path: dir.clone(),
        filter: None,
        target: None,
    })
    .expect("the test runs");
    assert!(report.ok(), "{report}");
    let _ = fs::remove_dir_all(&dir);
}

const COLUMNS_APP: &str = include_str!("fixtures/columns/App.tsx");

const COLUMNS_TESTS: &str = include_str!("fixtures/columns/App.test.tsx");

#[test]
fn a_list_lays_items_out_in_columns() {
    let _fixture = FIXTURE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("columns");
    fs::write(dir.join("App.tsx"), COLUMNS_APP).unwrap();
    fs::write(dir.join("App.test.tsx"), COLUMNS_TESTS).unwrap();
    let report = test(&TestOptions {
        path: dir.clone(),
        filter: None,
        target: None,
    })
    .expect("the test runs");
    assert!(report.ok(), "{report}");
    let _ = fs::remove_dir_all(&dir);
}

const INITIAL_APP: &str = include_str!("fixtures/initial/App.tsx");

const INITIAL_TESTS: &str = include_str!("fixtures/initial/App.test.tsx");

#[test]
fn a_list_opens_at_initial_scroll_index() {
    let _fixture = FIXTURE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("initial");
    fs::write(dir.join("App.tsx"), INITIAL_APP).unwrap();
    fs::write(dir.join("App.test.tsx"), INITIAL_TESTS).unwrap();
    let report = test(&TestOptions {
        path: dir.clone(),
        filter: None,
        target: None,
    })
    .expect("the test runs");
    assert!(report.ok(), "{report}");
    let _ = fs::remove_dir_all(&dir);
}
