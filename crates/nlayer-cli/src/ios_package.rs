// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
//! Unsigned iOS device applications.
//!
//! The app bundle and its images go into the `.app` next to the runner, so
//! the runner reads them from its own directory (the same path a development
//! build uses when no pushed `app.js` exists). The app icon is compiled by
//! Xcode's `actool`; `nlayer.json` supplies the identifiers, version and
//! icon. Signing is the application's own step and is not done here.

use std::{
    error::Error,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use nlayer_bundler::{BundleOptions, bundle};
use plist::{Dictionary, Value};
use serde::Deserialize;

use crate::{resolve_entry, run::workspace_root};

/// The deployment target of the device runner.
const MINIMUM_OS: &str = "15.0";
/// The runner executable inside the `.app`.
const EXECUTABLE: &str = "nlayer-ios-app";
/// The runtime's Cargo target and its build directory.
const TRIPLE: &str = "aarch64-apple-ios";

/// `nlayer.json`: only the fields packaging reads.
#[derive(Debug, Deserialize)]
struct Config {
    name: String,
    id: String,
    version: String,
    build: u32,
    /// A square PNG, usually 1024×1024, relative to `nlayer.json`.
    icon: Option<PathBuf>,
}

/// Writes an unsigned device `.app` for the app `entry` belongs to.
pub fn package_ios(entry: &Path, out: &Path) -> Result<(), Box<dyn Error>> {
    let entry = resolve_entry(entry)?;
    let root = entry.parent().ok_or("entry has no parent directory")?;
    let config: Config = serde_json::from_slice(&fs::read(root.join("nlayer.json"))?)?;
    if out.exists() {
        return Err(format!("output already exists: {}", out.display()).into());
    }
    let built = bundle(&BundleOptions {
        entry: entry.clone(),
        minify: true,
        test: false,
    })?;
    let runner = build_runner()?;

    fs::create_dir_all(out)?;
    fs::copy(runner, out.join(EXECUTABLE))?;
    fs::write(out.join("app.js"), built.code)?;
    for path in built.assets {
        if path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(format!("invalid asset path: {}", path.display()).into());
        }
        let dest = out.join(&path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(root.join(path), dest)?;
    }
    let mut info = info_plist(&config);
    if let Some(icon) = &config.icon {
        info.extend(compile_icon(&root.join(icon), out)?);
    }
    Value::Dictionary(info).to_file_xml(out.join("Info.plist"))?;
    Ok(())
}

/// The device runner: built from a cargo checkout's workspace, or the
/// installed runtime's `iphoneos/nlayer-ios-app`.
fn build_runner() -> Result<PathBuf, Box<dyn Error>> {
    let Some(workspace) = workspace_root() else {
        let runtime = crate::runtime::ios()?
            .ok_or("no iOS runtime: install @nlayer/runtime-ios, or run nlayer from a checkout")?;
        return match runtime.device {
            Some(runner) => Ok(runner),
            None => Err(
                "the iOS runtime has no device runner: runtime/ios/iphoneos/nlayer-ios-app".into(),
            ),
        };
    };
    let target = workspace.join("target/nlayer-ios-device");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .env("IPHONEOS_DEPLOYMENT_TARGET", MINIMUM_OS)
        .args([
            "build",
            "--release",
            "-p",
            "nlayer-platform-ios",
            "--bin",
            EXECUTABLE,
            "--target",
            TRIPLE,
            "--target-dir",
        ])
        .arg(&target)
        .current_dir(&workspace)
        .status()
        .map_err(|error| format!("cargo: {error}"))?;
    if !status.success() {
        return Err(format!(
            "the iOS runtime failed to build ({TRIPLE}; `rustup target add {TRIPLE}`)"
        )
        .into());
    }
    let runner = target.join(TRIPLE).join("release").join(EXECUTABLE);
    if runner.is_file() {
        Ok(runner)
    } else {
        Err(format!("iOS runtime {} missing", runner.display()).into())
    }
}

fn info_plist(config: &Config) -> Dictionary {
    let mut info = Dictionary::new();
    let build = config.build.to_string();
    let entries = [
        ("CFBundleExecutable", EXECUTABLE),
        ("CFBundleIdentifier", &config.id),
        ("CFBundleName", &config.name),
        ("CFBundleDisplayName", &config.name),
        ("CFBundlePackageType", "APPL"),
        ("CFBundleShortVersionString", &config.version),
        ("CFBundleVersion", &build),
        ("MinimumOSVersion", MINIMUM_OS),
    ];
    for (key, value) in entries {
        info.insert(key.into(), Value::String(value.into()));
    }
    info.insert(
        "CFBundleSupportedPlatforms".into(),
        Value::Array(vec![Value::String("iPhoneOS".into())]),
    );
    info.insert(
        "UIDeviceFamily".into(),
        Value::Array(vec![1.into(), 2.into()]),
    );
    // A launch screen entry opts into the full screen (no letterboxing).
    info.insert(
        "UILaunchScreen".into(),
        Value::Dictionary(Dictionary::new()),
    );
    info.insert(
        "UIApplicationSupportsMultipleScenes".into(),
        Value::Boolean(false),
    );
    // ProMotion iPhones run display links above 60 Hz only with this.
    info.insert(
        "CADisableMinimumFrameDurationOnPhone".into(),
        Value::Boolean(true),
    );
    let mut network = Dictionary::new();
    network.insert("NSAllowsArbitraryLoads".into(), Value::Boolean(true));
    info.insert("NSAppTransportSecurity".into(), Value::Dictionary(network));
    info
}

/// Compiles `icon` into `app`'s `Assets.car` with `actool` and returns the
/// Info.plist keys it adds.
fn compile_icon(icon: &Path, app: &Path) -> Result<Dictionary, Box<dyn Error>> {
    const CATALOG: &str = r#"{"info":{"author":"nlayer","version":1}}"#;
    // One 1024 pt image; actool derives every size (Xcode 14+ catalogs).
    const ICONSET: &str = r#"{"images":[{"filename":"icon.png","idiom":"universal","platform":"ios","size":"1024x1024"}],"info":{"author":"nlayer","version":1}}"#;
    let work = app.with_extension("xcassets-work");
    let catalog = work.join("Assets.xcassets");
    let set = catalog.join("AppIcon.appiconset");
    fs::create_dir_all(&set)?;
    fs::copy(icon, set.join("icon.png")).map_err(|error| format!("{}: {error}", icon.display()))?;
    fs::write(catalog.join("Contents.json"), CATALOG)?;
    fs::write(set.join("Contents.json"), ICONSET)?;
    let partial = work.join("partial.plist");
    let output = Command::new("xcrun")
        .args(["actool", "--compile"])
        .arg(app)
        .args([
            "--platform",
            "iphoneos",
            "--minimum-deployment-target",
            MINIMUM_OS,
            "--app-icon",
            "AppIcon",
            "--output-partial-info-plist",
        ])
        .arg(&partial)
        .args([
            "--target-device",
            "iphone",
            "--target-device",
            "ipad",
            "--output-format",
            "human-readable-text",
        ])
        .arg(&catalog)
        .output()
        .map_err(|error| format!("xcrun actool (is Xcode installed?): {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "actool failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let keys = match Value::from_file(&partial)? {
        Value::Dictionary(keys) => keys,
        _ => Dictionary::new(),
    };
    fs::remove_dir_all(work)?;
    Ok(keys)
}
