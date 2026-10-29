//! `nlayer run --target ios`.
//!
//! Installs the UIKit host in a simulator the first time (and again when
//! that binary changes). The app's bundle is written into the host's data
//! container, not into the signed `.app`, so a rebuild restarts the process
//! and drops its state without `simctl install`. A failed rebuild is
//! printed and the running process is left alone.

use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Sender, channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use nlayer_bundler::{Bundle, BundleOptions, bundle};
use serde::Deserialize;

use crate::{
    RunError, resolve_entry,
    run::{Watch, consider, consider_tree, workspace_root},
};

pub(crate) const BUNDLE_ID: &str = "dev.nlayer.ios";
const EXECUTABLE: &str = "nlayer-ios-app";

static STOP: AtomicBool = AtomicBool::new(false);

fn arm_signals() {
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
}

extern "C" fn on_signal(_signal: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

/// What `nlayer run --target ios` launches.
#[derive(Debug, Clone)]
pub struct IosOptions {
    /// An entry module, or a directory whose `App.tsx` is the entry.
    pub entry: PathBuf,
    /// Minify the bundle.
    pub release: bool,
}

/// A simulator app. Dropping it terminates the process.
#[must_use = "dropping the session terminates the simulator app"]
pub struct IosSession {
    udid: String,
    stop: Arc<AtomicBool>,
    stop_tx: Option<Sender<()>>,
    watch: Option<JoinHandle<()>>,
}

impl IosSession {
    /// Blocks until Ctrl-C or SIGTERM. Drop the session after this returns
    /// so the simulator process is terminated.
    pub fn park(&self) {
        arm_signals();
        while !self.stop.load(Ordering::Relaxed) && !STOP.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for IosSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.watch.take() {
            let _ = thread.join();
        }
        let _ = simctl(&["terminate", &self.udid, BUNDLE_ID]);
    }
}

/// Builds `options`, installs the host when its binary is not already the
/// one in the simulator, writes the bundle and launches it.
///
/// The first build finishes before this returns. Later builds run on a
/// background thread. A compile error does not restart the app.
pub fn run_ios(options: IosOptions) -> Result<IosSession, RunError> {
    let entry = resolve_entry(&options.entry)?;
    let app_dir = entry
        .parent()
        .ok_or_else(|| RunError::message("entry has no parent directory"))?
        .to_path_buf();
    let bundled = bundle(&BundleOptions {
        entry: entry.clone(),
        minify: options.release,
        test: false,
    })
    .map_err(|error| RunError::message(error.colored))?;
    let host = ios_host()?;
    let (udid, name) = simulator()?;
    let app = package(&host)?;
    if host_changed(&udid, &host)? {
        eprintln!("installing {BUNDLE_ID}");
        simctl(&["install", &udid, &app.to_string_lossy()])?;
        record_install(&udid, &host)?;
    }
    let docs = data_container(&udid)?.join("Documents");
    publish(&docs, &app_dir, &bundled)?;
    relaunch(&udid)?;
    eprintln!("launched on {name}");
    let mut watch = Watch::new(entry, app_dir.clone(), options.release, &bundled);
    let stop = Arc::new(AtomicBool::new(false));
    let (stop_tx, stop_rx) = channel();
    let shared = Arc::new(Shared {
        app_dir,
        udid: udid.clone(),
        docs,
    });
    let watch = thread::Builder::new()
        .name("nlayer-ios".to_owned())
        .spawn(move || {
            watch.run(stop_rx, move |bundled| {
                if publish(&shared.docs, &shared.app_dir, bundled)? {
                    relaunch(&shared.udid)?;
                    eprintln!("rebuilt in {:.0} ms", bundled.elapsed_ms);
                }
                Ok(())
            });
        })
        .map_err(|error| RunError::message(format!("watch thread: {error}")))?;
    Ok(IosSession {
        udid,
        stop,
        stop_tx: Some(stop_tx),
        watch: Some(watch),
    })
}

struct Shared {
    app_dir: PathBuf,
    udid: String,
    docs: PathBuf,
}

/// Writes the bundle and its imported assets. Returns whether anything
/// the running app would read was replaced.
///
/// Deletes `nlayer-test.json` when a previous `nlayer test` left one.
/// That file pins the viewport; `run` follows the window.
pub(crate) fn publish(docs: &Path, app_dir: &Path, bundled: &Bundle) -> Result<bool, RunError> {
    fs::create_dir_all(docs)?;
    let mut changed = write_if_different(&docs.join("app.js"), bundled.code.as_bytes())?;
    for rel in &bundled.assets {
        let Some(rel) = asset_rel(rel) else {
            return Err(RunError::message(format!(
                "asset path escapes the app directory: {}",
                rel.display()
            )));
        };
        let to = docs.join(rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = fs::read(app_dir.join(rel))?;
        if write_if_different(&to, &bytes)? {
            changed = true;
        }
    }
    let pin = docs.join("nlayer-test.json");
    match fs::remove_file(&pin) {
        Ok(()) => changed = true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(changed)
}

fn write_if_different(path: &Path, bytes: &[u8]) -> Result<bool, RunError> {
    if fs::read(path).is_ok_and(|have| have == bytes) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
    }
    fs::rename(&tmp, path)?;
    Ok(true)
}

fn asset_rel(path: &Path) -> Option<&Path> {
    let mut parts = path.components();
    if parts.any(|part| {
        matches!(
            part,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        None
    } else {
        Some(path)
    }
}

pub(crate) fn relaunch(udid: &str) -> Result<(), RunError> {
    simctl(&["launch", "--terminate-running-process", udid, BUNDLE_ID])?;
    Ok(())
}

/// The installed app is replaced only when it is missing or was installed
/// from a different host binary. A JS rebuild does not come through here.
pub(crate) fn host_changed(udid: &str, host: &Path) -> Result<bool, RunError> {
    if simctl_ok(&["get_app_container", udid, BUNDLE_ID]).is_none() {
        return Ok(true);
    }
    let recorded = fs::read_to_string(install_stamp(udid)).unwrap_or_default();
    Ok(recorded.trim() != host_id(host)?)
}

pub(crate) fn record_install(udid: &str, host: &Path) -> Result<(), RunError> {
    let path = install_stamp(udid);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, host_id(host)?)?;
    Ok(())
}

fn install_stamp(udid: &str) -> PathBuf {
    std::env::temp_dir().join(format!("nlayer-ios-host/installed-{udid}"))
}

fn host_id(path: &Path) -> Result<String, RunError> {
    use std::hash::{Hash, Hasher};
    let bytes = fs::read(path)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(format!("{}:{}", bytes.len(), hasher.finish()))
}

pub(crate) fn ios_host() -> Result<PathBuf, RunError> {
    if let Some(path) = std::env::var_os("NLAYER_IOS_APP") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(RunError::message(format!(
            "NLAYER_IOS_APP {} is not a file",
            path.display()
        )));
    }
    let Some(workspace) = workspace_root() else {
        return Err(RunError::message(
            "no iOS runtime. Set NLAYER_IOS_APP, or run nlayer from a checkout",
        ));
    };
    let sources = newest_source(&workspace);
    if let Some(path) = fresh_host(&workspace, sources) {
        return Ok(path);
    }
    eprintln!("building the iOS runtime");
    let target = workspace.join("target/nlayer-ios");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args([
            "build",
            "-p",
            "nlayer-platform-ios",
            "--bin",
            EXECUTABLE,
            "--target",
            "aarch64-apple-ios-sim",
            "--target-dir",
        ])
        .arg(&target)
        .current_dir(&workspace)
        .status()
        .map_err(|error| RunError::message(format!("cargo: {error}")))?;
    if !status.success() {
        return Err(RunError::message(
            "the iOS runtime failed to build (aarch64-apple-ios-sim; `rustup target add aarch64-apple-ios-sim`)",
        ));
    }
    let built = target.join("aarch64-apple-ios-sim/debug").join(EXECUTABLE);
    if built.is_file() {
        Ok(built)
    } else {
        Err(RunError::message(format!(
            "iOS runtime {} missing",
            built.display()
        )))
    }
}

fn newest_source(workspace: &Path) -> SystemTime {
    let mut newest = UNIX_EPOCH;
    consider(&workspace.join("Cargo.toml"), &mut newest);
    consider(&workspace.join("Cargo.lock"), &mut newest);
    consider(&workspace.join(".cargo/config.toml"), &mut newest);
    for name in [
        "nlayer-core",
        "nlayer-js",
        "nlayer-engine",
        "nlayer-platform-ios",
    ] {
        consider_tree(&workspace.join("crates").join(name), &mut newest);
    }
    newest
}

fn fresh_host(workspace: &Path, sources: SystemTime) -> Option<PathBuf> {
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for path in [
        workspace
            .join("target/aarch64-apple-ios-sim/debug")
            .join(EXECUTABLE),
        workspace
            .join("target/nlayer-ios/aarch64-apple-ios-sim/debug")
            .join(EXECUTABLE),
    ] {
        let Ok(modified) = fs::metadata(&path).and_then(|meta| meta.modified()) else {
            continue;
        };
        if modified >= sources && best.as_ref().is_none_or(|(when, _)| modified > *when) {
            best = Some((modified, path));
        }
    }
    best.map(|(_, path)| path)
}

pub(crate) fn package(host: &Path) -> Result<PathBuf, RunError> {
    let root = std::env::temp_dir().join("nlayer-ios-host");
    let app = root.join("Nlayer.app");
    fs::create_dir_all(&app)?;
    let id = host_id(host)?;
    let packaged = fs::read_to_string(root.join("packaged-id")).unwrap_or_default();
    let plist_changed = write_if_different(&app.join("Info.plist"), info_plist().as_bytes())?;
    let exe = app.join(EXECUTABLE);
    if packaged.trim() != id
        || plist_changed
        || !exe.is_file()
        || !app.join("_CodeSignature").exists()
    {
        let tmp = app.join(format!("{EXECUTABLE}.tmp"));
        fs::copy(host, &tmp)?;
        fs::rename(&tmp, &exe)?;
        let status = Command::new("codesign")
            .args(["--force", "--sign", "-", "--timestamp=none"])
            .arg(&app)
            .status()
            .map_err(|error| RunError::message(format!("codesign: {error}")))?;
        if !status.success() {
            return Err(RunError::message(format!(
                "codesign failed for {}",
                app.display()
            )));
        }
        fs::write(root.join("packaged-id"), id)?;
    }
    Ok(app)
}

fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>{EXECUTABLE}</string>
    <key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
    <key>CFBundleName</key><string>nlayer</string>
    <key>CFBundleDisplayName</key><string>nlayer</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>UIApplicationSupportsMultipleScenes</key><false/>
    <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
    <key>MinimumOSVersion</key><string>15.0</string>
    <key>CFBundleSupportedPlatforms</key><array><string>iPhoneSimulator</string></array>
    <key>UILaunchScreen</key><dict/>
    <key>CADisableMinimumFrameDurationOnPhone</key><true/>
    <key>NSAppTransportSecurity</key><dict><key>NSAllowsArbitraryLoads</key><true/></dict>
</dict>
</plist>
"#
    )
}

pub(crate) fn simulator() -> Result<(String, String), RunError> {
    let list: DeviceList =
        serde_json::from_str(&simctl(&["list", "devices", "available", "--json"])?)
            .map_err(|error| RunError::message(format!("simctl device list: {error}")))?;
    let device = select(&list, None).map_err(RunError::message)?;
    let udid = device.udid.clone();
    let name = device.name.clone();
    if device.state != "Booted" {
        eprintln!("booting {name}");
        simctl(&["boot", &udid])?;
    }
    simctl(&["bootstatus", &udid, "-b"])?;
    show_simulators();
    Ok((udid, name))
}
fn show_simulators() {
    let quiet = |command: &mut Command| {
        command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    if quiet(Command::new("open").args(["-b", "com.apple.iphonesimulator"])) {
        return;
    }
    let Ok(developer) = Command::new("xcode-select").arg("-p").output() else {
        return;
    };
    let developer = String::from_utf8_lossy(&developer.stdout);
    let hub = Path::new(developer.trim()).join("../Applications/DeviceHub.app");
    if hub.exists() {
        let _ = quiet(Command::new("open").arg(&hub));
    }
}

#[derive(Deserialize)]
struct DeviceList {
    devices: std::collections::BTreeMap<String, Vec<Device>>,
}

#[derive(Deserialize)]
struct Device {
    udid: String,
    name: String,
    state: String,
    #[serde(rename = "isAvailable", default)]
    available: bool,
}

fn select<'a>(list: &'a DeviceList, wanted: Option<&str>) -> Result<&'a Device, String> {
    let mut devices: Vec<&Device> = list
        .devices
        .iter()
        .filter(|(runtime, _)| runtime.contains("SimRuntime.iOS"))
        .flat_map(|(_, devices)| devices)
        .filter(|device| device.available)
        .collect();
    devices.reverse();
    let device = match wanted {
        Some(wanted) => devices
            .into_iter()
            .find(|device| device.udid == wanted || device.name == wanted)
            .ok_or_else(|| format!("no available simulator named {wanted}"))?,
        None => devices
            .iter()
            .copied()
            .find(|device| device.state == "Booted")
            .or_else(|| {
                devices
                    .iter()
                    .copied()
                    .find(|device| device.name.starts_with("iPhone") && device.name.contains("Pro"))
            })
            .or_else(|| {
                devices
                    .iter()
                    .copied()
                    .find(|device| device.name.starts_with("iPhone"))
            })
            .ok_or_else(|| {
                "no iOS simulator available: install one with `xcodebuild -downloadPlatform iOS`"
                    .to_owned()
            })?,
    };
    Ok(device)
}

pub(crate) fn data_container(udid: &str) -> Result<PathBuf, RunError> {
    let out = simctl(&["get_app_container", udid, BUNDLE_ID, "data"])?;
    let path = PathBuf::from(out.trim());
    if path.is_dir() {
        Ok(path)
    } else {
        Err(RunError::message(format!(
            "simulator data container is not a directory: {}",
            path.display()
        )))
    }
}

pub(crate) fn simctl(args: &[&str]) -> Result<String, RunError> {
    let output = Command::new("xcrun")
        .arg("simctl")
        .args(args)
        .output()
        .map_err(|error| {
            RunError::message(format!(
                "xcrun simctl {} (is Xcode installed?): {error}",
                args.first().copied().unwrap_or("")
            ))
        })?;
    if !output.status.success() {
        return Err(RunError::message(format!(
            "simctl {} failed: {}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn simctl_ok(args: &[&str]) -> Option<String> {
    simctl(args).ok()
}

#[cfg(test)]
mod tests {
    use super::{DeviceList, asset_rel, host_id, info_plist, publish, select};
    use nlayer_bundler::Bundle;

    #[test]
    fn the_plist_opts_into_a_full_screen_ios_15_app() {
        let text = info_plist();
        assert!(text.contains("<string>nlayer-ios-app</string>"), "{text}");
        assert!(text.contains("<string>dev.nlayer.ios</string>"), "{text}");
        assert!(text.contains("<string>15.0</string>"), "{text}");
        assert!(text.contains("<key>UILaunchScreen</key><dict/>"), "{text}");
        assert!(
            text.contains("<key>UIApplicationSupportsMultipleScenes</key><false/>"),
            "{text}"
        );
        assert!(!text.contains("UISceneConfigurations"), "{text}");
        assert!(text.contains("NSAllowsArbitraryLoads"), "{text}");
    }

    #[test]
    fn a_booted_simulator_wins_and_a_name_selects_one() {
        let list: DeviceList = serde_json::from_str(
            r#"{
              "devices": {
                "com.apple.CoreSimulator.SimRuntime.iOS-17-0": [
                  {"udid": "old", "name": "iPhone 15 Pro", "state": "Shutdown", "isAvailable": true}
                ],
                "com.apple.CoreSimulator.SimRuntime.iOS-18-0": [
                  {"udid": "pro", "name": "iPhone 16 Pro", "state": "Shutdown", "isAvailable": true},
                  {"udid": "boot", "name": "iPhone 16", "state": "Booted", "isAvailable": true},
                  {"udid": "gone", "name": "iPhone 16", "state": "Booted", "isAvailable": false}
                ]
              }
            }"#,
        )
        .unwrap();
        assert_eq!(select(&list, None).unwrap().udid, "boot");
        assert_eq!(select(&list, Some("iPhone 16 Pro")).unwrap().udid, "pro");
        assert_eq!(select(&list, Some("old")).unwrap().udid, "old");
        assert!(select(&list, Some("iPad")).is_err());
    }

    #[test]
    fn without_a_booted_simulator_the_newest_iphone_pro_is_used() {
        let list: DeviceList = serde_json::from_str(
            r#"{
              "devices": {
                "com.apple.CoreSimulator.SimRuntime.iOS-17-0": [
                  {"udid": "old-pro", "name": "iPhone 15 Pro", "state": "Shutdown", "isAvailable": true}
                ],
                "com.apple.CoreSimulator.SimRuntime.iOS-18-0": [
                  {"udid": "plain", "name": "iPhone 16", "state": "Shutdown", "isAvailable": true},
                  {"udid": "pro", "name": "iPhone 16 Pro", "state": "Shutdown", "isAvailable": true}
                ]
              }
            }"#,
        )
        .unwrap();
        assert_eq!(select(&list, None).unwrap().udid, "pro");
    }

    #[test]
    fn an_asset_cannot_escape_the_data_container() {
        assert!(asset_rel(std::path::Path::new("img/a.png")).is_some());
        assert!(asset_rel(std::path::Path::new("../a.png")).is_none());
        assert!(asset_rel(std::path::Path::new("/tmp/a.png")).is_none());
    }

    #[test]
    fn identical_host_binaries_share_an_id() {
        let dir = std::env::temp_dir().join(format!("nlayer-ios-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let left = dir.join("a");
        let right = dir.join("b");
        std::fs::write(&left, b"host").unwrap();
        std::fs::write(&right, b"host").unwrap();
        assert_eq!(host_id(&left).unwrap(), host_id(&right).unwrap());
        std::fs::write(&right, b"host!").unwrap();
        assert_ne!(host_id(&left).unwrap(), host_id(&right).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn run_drops_a_leftover_test_pin() {
        let dir = std::env::temp_dir().join(format!("nlayer-ios-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let docs = dir.join("Documents");
        let app = dir.join("app");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(
            docs.join("nlayer-test.json"),
            b"{\"width\":390,\"height\":844}",
        )
        .unwrap();
        let bundled = Bundle {
            code: "app".to_owned(),
            source_map: String::new(),
            assets: Vec::new(),
            watch_files: Vec::new(),
            elapsed_ms: 0.0,
        };
        assert!(publish(&docs, &app, &bundled).unwrap());
        assert!(!docs.join("nlayer-test.json").exists());
        assert!(!publish(&docs, &app, &bundled).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
