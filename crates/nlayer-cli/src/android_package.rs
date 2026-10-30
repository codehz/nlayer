// Copyright 2026 Longbridge
//
// Licensed under the Apache License, Version 2.0.
//
//! `nlayer package --target android`: an installable APK.
//!
//! The runtime is the same one `nlayer run` installs; this build carries the
//! app's bundle and images in the APK's `assets/` instead of pushing them
//! into the data directory, and takes the manifest's name, id, version and
//! icon from `nlayer.json`. A pushed `app.js` still wins at launch, so the
//! development path keeps working on an installed package.

use std::{
    error::Error,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use nlayer_bundler::{BundleOptions, bundle};
use serde::Deserialize;
use zip::CompressionMethod;

use crate::{
    android::{Abi, MIN_SDK, Sdk, TARGET_SDK, add_files, cargo_lib, debug_keystore, dex_host, run},
    resolve_entry,
    run::workspace_root,
};

/// The activity in the packaged APK (the Java package stays nlayer's; the
/// app's id only names the APK).
const ACTIVITY: &str = "dev.nlayer.android.HostActivity";

/// What `nlayer package --target android` writes.
#[derive(Debug, Clone)]
pub struct AndroidPackageOptions {
    /// An entry module, or a directory whose `App.tsx` is the entry.
    pub entry: PathBuf,
    /// The APK path.
    pub out: PathBuf,
    /// Sign with this keystore instead of the debug key.
    pub keystore: Option<PathBuf>,
    /// The key alias inside `keystore`.
    pub alias: Option<String>,
    /// The keystore and key password. `pass:`, `file:` and `env:` prefixes
    /// reach `apksigner` unchanged; anything else is a plaintext password.
    pub store_pass: Option<String>,
}

/// `nlayer.json`: only the fields packaging reads.
#[derive(Debug, Deserialize)]
struct Config {
    name: String,
    id: String,
    version: String,
    build: u32,
    /// A square PNG, relative to `nlayer.json`.
    icon: Option<PathBuf>,
}

/// Writes the APK.
pub fn package_android(options: &AndroidPackageOptions) -> Result<(), Box<dyn Error>> {
    let entry = resolve_entry(&options.entry)?;
    let root = entry.parent().ok_or("entry has no parent directory")?;
    let config: Config = serde_json::from_slice(&fs::read(root.join("nlayer.json"))?)?;
    if !valid_id(&config.id) {
        return Err(format!("nlayer.json id is not an application id: {}", config.id).into());
    }
    let built = bundle(&BundleOptions {
        entry: entry.clone(),
        minify: true,
        test: false,
    })?;
    let workspace = workspace_root().ok_or("no Android runtime: run nlayer from a checkout")?;
    let sdk = Sdk::find()?;

    let work = options.out.with_extension("work");
    if work.exists() {
        fs::remove_dir_all(&work)?;
    }
    let payload = work.join("assets");
    fs::create_dir_all(&payload)?;
    fs::write(payload.join("app.js"), &built.code)?;
    for rel in &built.assets {
        if rel
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(format!("invalid asset path: {}", rel.display()).into());
        }
        let dest = payload.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(root.join(rel), &dest)?;
    }

    let dex = dex_host(&sdk, &workspace)?;
    let mut libraries = Vec::new();
    for abi in Abi::all() {
        libraries.push((abi.name(), cargo_lib(&sdk, &workspace, &abi, true)?));
    }

    let manifest = work.join("AndroidManifest.xml");
    fs::write(&manifest, manifest_xml(&config, config.icon.is_some()))?;
    let base = work.join("base.apk");
    let mut link = Command::new(sdk.tool("aapt2"));
    link.arg("link")
        .arg("-o")
        .arg(&base)
        .arg("-I")
        .arg(sdk.android_jar()?)
        .arg("--manifest")
        .arg(&manifest)
        .arg("--min-sdk-version")
        .arg(MIN_SDK.to_string())
        .arg("--target-sdk-version")
        .arg(TARGET_SDK.to_string())
        .arg("-A")
        .arg(&payload);
    if let Some(icon) = &config.icon {
        link.arg(compile_icon(&work, &sdk, &root.join(icon))?);
    }
    run(&mut link)?;

    let paths: Vec<(String, PathBuf, CompressionMethod)> =
        std::iter::once(("classes.dex".to_owned(), dex, CompressionMethod::Deflated))
            .chain(libraries.iter().map(|(name, lib)| {
                (
                    format!("lib/{name}/libnlayer.so"),
                    lib.clone(),
                    CompressionMethod::Stored,
                )
            }))
            .collect();
    let files: Vec<(&str, &Path, CompressionMethod)> = paths
        .iter()
        .map(|(name, path, method)| (name.as_str(), path.as_path(), *method))
        .collect();
    let unaligned = work.join("unaligned.apk");
    add_files(&base, &unaligned, &files)?;

    let aligned = work.join("aligned.apk");
    run(Command::new(sdk.tool("zipalign"))
        .args(["-P", "16", "-f", "4"])
        .arg(&unaligned)
        .arg(&aligned))?;
    sign(&sdk, options, &aligned)?;

    if let Some(parent) = options.out.parent() {
        fs::create_dir_all(parent)?;
    }
    if options.out.exists() {
        fs::remove_file(&options.out)?;
    }
    fs::rename(&aligned, &options.out)?;
    fs::remove_dir_all(&work)?;
    Ok(())
}

/// Compiles `icon` into `work/resources.zip` for `aapt2 link`.
fn compile_icon(work: &Path, sdk: &Sdk, icon: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let res = work.join("res/mipmap-xxxhdpi");
    fs::create_dir_all(&res)?;
    fs::copy(icon, res.join("ic_launcher.png"))
        .map_err(|error| format!("{}: {error}", icon.display()))?;
    let compiled = work.join("resources.zip");
    let mut command = Command::new(sdk.tool("aapt2"));
    command
        .arg("compile")
        .arg("--dir")
        .arg(work.join("res"))
        .arg("-o")
        .arg(&compiled);
    run(&mut command)?;
    Ok(compiled)
}

fn sign(sdk: &Sdk, options: &AndroidPackageOptions, apk: &Path) -> Result<(), Box<dyn Error>> {
    let mut command = Command::new(sdk.tool("apksigner"));
    command.arg("sign");
    match &options.keystore {
        None => {
            command
                .arg("--ks")
                .arg(debug_keystore()?)
                .args(["--ks-key-alias", "androiddebugkey"])
                .args(["--ks-pass", "pass:android"])
                .args(["--key-pass", "pass:android"]);
        }
        Some(keystore) => {
            command.arg("--ks").arg(keystore);
            if let Some(alias) = &options.alias {
                command.args(["--ks-key-alias", alias]);
            }
            if let Some(pass) = &options.store_pass {
                let pass = if pass.contains(':') {
                    pass.clone()
                } else {
                    format!("pass:{pass}")
                };
                command
                    .args(["--ks-pass", &pass])
                    .args(["--key-pass", &pass]);
            }
        }
    }
    command.arg(apk);
    run(&mut command)?;
    Ok(())
}

fn manifest_xml(config: &Config, icon: bool) -> String {
    let label = escape(&config.name);
    let version = escape(&config.version);
    let icon = if icon {
        r#" android:icon="@mipmap/ic_launcher""#
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="{id}"
    android:versionCode="{build}" android:versionName="{version}">
    <uses-permission android:name="android.permission.INTERNET" />
    <application android:label="{label}"{icon} android:usesCleartextTraffic="true"
        android:extractNativeLibs="false" android:supportsRtl="true"
        android:theme="@android:style/Theme.Material.Light.NoActionBar">
        <activity android:name="{ACTIVITY}" android:exported="true"
            android:windowSoftInputMode="adjustNothing"
            android:configChanges="orientation|screenSize|screenLayout|smallestScreenSize|keyboard|keyboardHidden|navigation|uiMode|density|fontScale|locale|layoutDirection">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#,
        id = config.id.as_str(),
        build = config.build,
    )
}

/// A dotted application id: letters, digits and `_` in each segment, at
/// least two segments. `aapt2` refuses anything else.
fn valid_id(id: &str) -> bool {
    let mut segments = 0;
    for segment in id.split('.') {
        if segment.is_empty()
            || !segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            || segment.starts_with(|c: char| c.is_ascii_digit())
        {
            return false;
        }
        segments += 1;
    }
    segments >= 2
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            name: "Example & Co".to_owned(),
            id: "com.example.app".to_owned(),
            version: "1.2.3".to_owned(),
            build: 7,
            icon: None,
        }
    }

    #[test]
    fn an_application_id_needs_segments() {
        assert!(valid_id("com.example.app"));
        assert!(valid_id("org.example_1.app"));
        assert!(!valid_id("example"));
        assert!(!valid_id("com..app"));
        assert!(!valid_id("com.1example.app"));
        assert!(!valid_id("com.example-app"));
        assert!(!valid_id(""));
    }

    #[test]
    fn the_manifest_carries_the_config_and_an_optional_icon() {
        let plain = manifest_xml(&config(), false);
        assert!(plain.contains(r#"package="com.example.app""#), "{plain}");
        assert!(plain.contains(r#"android:versionCode="7""#), "{plain}");
        assert!(plain.contains(r#"android:versionName="1.2.3""#), "{plain}");
        assert!(
            plain.contains(r#"android:label="Example &amp; Co""#),
            "{plain}"
        );
        assert!(!plain.contains("mipmap"), "{plain}");
        let icon = manifest_xml(&config(), true);
        assert!(
            icon.contains(r#"android:icon="@mipmap/ic_launcher""#),
            "{icon}"
        );
    }
}
