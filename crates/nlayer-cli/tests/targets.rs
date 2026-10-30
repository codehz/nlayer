//! `nlayer test --target` on the three hosts, plus gallery and the data
//! channel (resource and storage) on each.
//!
//! One binary, one lock: the simulator and the emulator cannot be driven
//! twice at once, and `cargo test` would otherwise run these in parallel.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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

/// A copy of the app fixture `name`, for one run.
fn app_fixture(name: &str) -> PathBuf {
    let dir = scratch(&format!("target-{name}"));
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    for file in ["App.tsx", "App.test.tsx", "Web.test.tsx", "quote.json"] {
        let source = from.join(file);
        if source.exists() {
            fs::copy(source, dir.join(file)).unwrap();
        }
    }
    dir
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
fn the_browser_takes_a_line_break_in_a_multiline_input() {
    let _guard = lock();
    let dir = app_fixture("linebreak");
    let report = run(&dir.join("App.test.tsx"), Some(TestTarget::Web));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "web: {report}");
}

#[test]
fn the_simulator_takes_a_line_break_in_a_multiline_input() {
    let _guard = lock();
    let dir = app_fixture("linebreak");
    let report = run(&dir.join("App.test.tsx"), Some(TestTarget::Ios));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "ios: {report}");
}

#[test]
fn the_emulator_takes_a_line_break_in_a_multiline_input() {
    let _guard = lock();
    let dir = app_fixture("linebreak");
    let report = run(&dir.join("App.test.tsx"), Some(TestTarget::Android));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "android: {report}");
}

#[test]
fn the_browser_renders_and_scrolls_a_long_list() {
    let _guard = lock();
    let dir = app_fixture("list");
    let report = run(&dir.join("App.test.tsx"), Some(TestTarget::Web));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "web: {report}");
}

#[test]
fn the_browser_opens_a_list_at_initial_scroll_index() {
    let _guard = lock();
    let dir = app_fixture("initial");
    let report = run(&dir.join("App.test.tsx"), Some(TestTarget::Web));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "web: {report}");
}

#[test]
fn the_simulator_long_presses_a_view() {
    let _guard = lock();
    let dir = app_fixture("longpress");
    let report = run(&dir.join("App.test.tsx"), Some(TestTarget::Ios));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "ios: {report}");
}

#[test]
fn the_browser_long_presses_a_view() {
    let _guard = lock();
    let dir = app_fixture("longpress");
    let report = run(&dir.join("App.test.tsx"), Some(TestTarget::Web));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "web: {report}");
}

#[test]
fn the_emulator_animates_a_view() {
    let _guard = lock();
    let dir = app_fixture("animation");
    let report = run(&dir.join("Web.test.tsx"), Some(TestTarget::Android));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "android: {report}");
}

#[test]
fn the_browser_animates_a_view() {
    let _guard = lock();
    let dir = app_fixture("animation");
    let report = run(&dir.join("Web.test.tsx"), Some(TestTarget::Web));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "web: {report}");
}

#[test]
fn the_emulator_long_presses_a_view() {
    let _guard = lock();
    let dir = app_fixture("longpress");
    let report = run(&dir.join("App.test.tsx"), Some(TestTarget::Android));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "android: {report}");
}

#[test]
fn the_simulator_animates_a_view() {
    let _guard = lock();
    let dir = app_fixture("animation");
    let report = run(&dir.join("Web.test.tsx"), Some(TestTarget::Ios));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "ios: {report}");
}

#[test]
fn the_simulator_renders_and_scrolls_a_long_list() {
    let _guard = lock();
    let dir = app_fixture("list");
    let report = run(&dir.join("App.test.tsx"), Some(TestTarget::Ios));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "ios: {report}");
}

#[test]
fn the_emulator_renders_and_scrolls_a_long_list() {
    let _guard = lock();
    let dir = app_fixture("list");
    let report = run(&dir.join("App.test.tsx"), Some(TestTarget::Android));
    let _ = fs::remove_dir_all(&dir);
    assert!(report.ok(), "android: {report}");
}

#[test]
fn the_java_host_tests_pass_on_the_emulator() {
    let _guard = lock();
    let report = nlayer_cli::android_java_tests()
        .unwrap_or_else(|error| panic!("android Java tests: {error}"));
    assert!(
        report.failures.is_empty(),
        "android Java tests: {}",
        report.failures.join("\n")
    );
    assert_eq!(report.passed, report.total, "android Java tests");
    assert!(report.total > 0, "android Java tests: no tests ran");
}

#[test]
fn the_ios_host_tests_pass_in_the_simulator() {
    let _guard = lock();
    let report =
        nlayer_cli::ios_host_tests().unwrap_or_else(|error| panic!("iOS host tests: {error}"));
    assert!(
        report.failures.is_empty(),
        "iOS host tests:\n{}",
        report.failures.join("\n")
    );
    assert_eq!(report.passed, report.total, "iOS host tests");
    assert!(report.total > 0, "iOS host tests: no tests ran");
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

#[test]
fn the_data_channel_runs_on_each_target() {
    let _guard = lock();
    for name in ["data", "resource", "adapt"] {
        let dir = app_fixture(name);
        for target in [
            Some(TestTarget::Web),
            Some(TestTarget::Ios),
            Some(TestTarget::Android),
        ] {
            let report = run(&dir.join("App.test.tsx"), target);
            assert!(report.ok(), "{name} on {target:?}: {report}");
        }
        let _ = fs::remove_dir_all(&dir);
    }
}

/// The SDK's newest `aapt2`.
fn aapt2() -> PathBuf {
    let root = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .find_map(std::env::var_os)
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Android/sdk"))
        })
        .expect("the Android SDK (ANDROID_HOME)");
    let mut builds: Vec<PathBuf> = fs::read_dir(root.join("build-tools"))
        .expect("Android build-tools")
        .map(|entry| entry.unwrap().path())
        .collect();
    builds.sort();
    builds.pop().expect("Android build-tools").join("aapt2")
}

#[test]
fn packages_build_for_each_target() {
    let _guard = lock();
    let entry = gallery();
    let dir = entry.parent().expect("the gallery directory").to_path_buf();
    let out = scratch("packages");

    let web = out.join("web");
    nlayer_cli::package_web(&dir, &web).expect("the web package");
    let page = fs::read_to_string(web.join("index.html")).unwrap();
    assert!(page.contains("dev.nlayer.gallery"), "{page}");
    assert!(page.contains("\"mark.png\":\"assets/"), "{page}");
    assert!(web.join("bundle.js").is_file());
    assert!(web.join("bundle.js.map").is_file());
    assert!(web.join("nlayer.wasm").is_file());

    let apk = out.join("app.apk");
    nlayer_cli::package_android(&nlayer_cli::AndroidPackageOptions {
        entry: dir.clone(),
        out: apk.clone(),
        keystore: None,
        alias: None,
        store_pass: None,
    })
    .expect("the Android package");
    let badging = Command::new(aapt2())
        .args(["dump", "badging"])
        .arg(&apk)
        .output()
        .expect("aapt2");
    let badging = String::from_utf8_lossy(&badging.stdout);
    assert!(badging.contains("name='dev.nlayer.gallery'"), "{badging}");
    assert!(badging.contains("versionCode='1'"), "{badging}");
    assert!(badging.contains("versionName='1.0.0'"), "{badging}");
    let mut zip = zip::ZipArchive::new(fs::File::open(&apk).unwrap()).unwrap();
    assert!(zip.by_name("assets/app.js").is_ok(), "the bundled app");
    assert!(zip.by_name("assets/mark.png").is_ok(), "the bundled image");
    for abi in ["arm64-v8a", "armeabi-v7a", "x86_64"] {
        assert!(
            zip.by_name(&format!("lib/{abi}/libnlayer.so")).is_ok(),
            "{abi}"
        );
    }

    let bundle = out.join("Nlayer.app");
    nlayer_cli::package_ios(&dir, &bundle).expect("the iOS package");
    let info = plist::Value::from_file(bundle.join("Info.plist")).expect("Info.plist");
    let info = info.as_dictionary().expect("a dictionary");
    assert_eq!(
        info["CFBundleIdentifier"].as_string(),
        Some("dev.nlayer.gallery")
    );
    assert_eq!(info["CFBundleVersion"].as_string(), Some("1"));
    assert!(bundle.join("app.js").is_file());
    assert!(bundle.join("mark.png").is_file());
    assert!(
        !bundle.join("_CodeSignature").exists(),
        "the .app is unsigned"
    );

    let _ = fs::remove_dir_all(&out);
}
