//! `nlayer run --target android`.
//!
//! Builds `libnlayer.so` and the Java host, packages a debug APK with the
//! SDK's `aapt2` / `d8` / `zipalign` / `apksigner`, and installs it on an
//! emulator or a connected device. A rebuild pushes `app.js` and restarts
//! the activity. The APK is not reinstalled unless the host changed.
//!
//! Packaging follows `nsh-cli/src/android.rs`, cut to one ABI and the v1
//! host. There is no Gradle project and no dev server.

use std::{
    env, fs,
    io::Write as _,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use nlayer_bundler::{Bundle, BundleOptions, bundle};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    RunError, resolve_entry,
    run::{consider, consider_tree, stamp, watch_paths, workspace_root},
};

/// Application id. The activity is [`ACTIVITY`].
pub(crate) const PACKAGE: &str = "dev.nlayer.android";
const ACTIVITY: &str = "dev.nlayer.android.HostActivity";
const MIN_SDK: u32 = 28;
const TARGET_SDK: u32 = 35;
const POLL: Duration = Duration::from_millis(150);

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

/// What `nlayer run --target android` launches.
#[derive(Debug, Clone)]
pub struct AndroidOptions {
    /// An entry module, or a directory whose `App.tsx` is the entry.
    pub entry: PathBuf,
    /// Minify the bundle.
    pub release: bool,
}

/// A device session. Dropping it stops the watch and force-stops the app.
#[must_use = "dropping the session force-stops the Android app"]
pub struct AndroidSession {
    serial: String,
    stop: Arc<AtomicBool>,
    stop_tx: Option<Sender<()>>,
    watch: Option<JoinHandle<()>>,
}

impl AndroidSession {
    /// Blocks until Ctrl-C or SIGTERM.
    pub fn park(&self) {
        arm_signals();
        while !self.stop.load(Ordering::Relaxed) && !STOP.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for AndroidSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.watch.take() {
            let _ = thread.join();
        }
        if let Ok(sdk) = Sdk::find() {
            let _ = adb(&sdk, &self.serial, &["shell", "am", "force-stop", PACKAGE]);
        }
    }
}

/// Builds `options`, installs the host when it changed, pushes the bundle
/// and launches it.
///
/// The first build finishes before this returns. Later builds run on a
/// background thread. A compile error does not restart the app.
pub fn run_android(options: AndroidOptions) -> Result<AndroidSession, RunError> {
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
    let sdk = Sdk::find()?;
    let serial = device(&sdk)?;
    let abi = device_abi(&sdk, &serial)?;
    let apk = host_apk(&sdk, &abi, options.release)?;
    install_if_needed(&sdk, &serial, &apk)?;
    publish(&sdk, &serial, &app_dir, &bundled)?;
    relaunch(&sdk, &serial)?;
    eprintln!("launched on {serial}");
    let watched = watch_paths(&app_dir, Some(&bundled));
    let initial = stamp(&watched);
    let stop = Arc::new(AtomicBool::new(false));
    let (stop_tx, stop_rx) = channel();
    let shared = Arc::new(Shared {
        entry,
        app_dir,
        release: options.release,
        sdk,
        serial: serial.clone(),
        watched: Mutex::new(watched),
        stamp: Mutex::new(initial),
        stop: Arc::clone(&stop),
    });
    let watch = thread::Builder::new()
        .name("nlayer-android".to_owned())
        .spawn(move || watch_loop(shared, stop_rx))
        .map_err(|error| RunError::message(format!("watch thread: {error}")))?;
    Ok(AndroidSession {
        serial,
        stop,
        stop_tx: Some(stop_tx),
        watch: Some(watch),
    })
}

struct Shared {
    entry: PathBuf,
    app_dir: PathBuf,
    release: bool,
    sdk: Sdk,
    serial: String,
    watched: Mutex<Vec<PathBuf>>,
    stamp: Mutex<u64>,
    stop: Arc<AtomicBool>,
}

fn watch_loop(shared: Arc<Shared>, stop: Receiver<()>) {
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        let watched = shared.watched.lock().expect("android watch").clone();
        let next = stamp(&watched);
        let changed = *shared.stamp.lock().expect("android watch") != next;
        if changed {
            rebuild(&shared);
        }
        match stop.recv_timeout(POLL) {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn rebuild(shared: &Shared) {
    let built = bundle(&BundleOptions {
        entry: shared.entry.clone(),
        minify: shared.release,
        test: false,
    });
    let watched;
    let stamp = match built {
        Ok(bundled) => match publish(&shared.sdk, &shared.serial, &shared.app_dir, &bundled) {
            Ok(()) => {
                if let Err(error) = relaunch(&shared.sdk, &shared.serial) {
                    eprintln!("{error}");
                } else {
                    eprintln!("rebuilt in {:.0} ms", bundled.elapsed_ms);
                }
                watched = watch_paths(&shared.app_dir, Some(&bundled));
                stamp(&watched)
            }
            Err(error) => {
                eprintln!("{error}");
                watched = watch_paths(&shared.app_dir, Some(&bundled));
                stamp(&watched)
            }
        },
        Err(error) => {
            eprintln!("{}", error.colored);
            watched = watch_paths(&shared.app_dir, None);
            stamp(&watched)
        }
    };
    *shared.watched.lock().expect("android watch") = watched;
    *shared.stamp.lock().expect("android watch") = stamp;
}

pub(crate) struct Sdk {
    root: PathBuf,
    build_tools: PathBuf,
}

impl Sdk {
    pub(crate) fn find() -> Result<Self, RunError> {
        let root = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
            .iter()
            .find_map(env::var_os)
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Android/sdk"))
            })
            .filter(|path| path.exists())
            .ok_or_else(|| RunError::message("Android SDK not found: set ANDROID_HOME"))?;
        let build_tools = newest(&root.join("build-tools"))?;
        Ok(Self { root, build_tools })
    }

    fn tool(&self, name: &str) -> PathBuf {
        self.build_tools.join(name)
    }

    pub(crate) fn adb(&self) -> PathBuf {
        self.root.join("platform-tools/adb")
    }

    fn android_jar(&self) -> Result<PathBuf, RunError> {
        let jar = self
            .root
            .join(format!("platforms/android-{TARGET_SDK}/android.jar"));
        if jar.exists() {
            Ok(jar)
        } else {
            Err(RunError::message(format!(
                "{} missing: install platforms;android-{TARGET_SDK}",
                jar.display()
            )))
        }
    }
}

fn newest(dir: &Path) -> Result<PathBuf, RunError> {
    let mut best: Option<PathBuf> = None;
    let entries = fs::read_dir(dir).map_err(|_| {
        RunError::message(format!(
            "no {} — install the Android SDK build-tools",
            dir.display()
        ))
    })?;
    for entry in entries {
        let path = entry
            .map_err(|error| RunError::message(error.to_string()))?
            .path();
        if best.as_ref().is_none_or(|have| path > *have) {
            best = Some(path);
        }
    }
    best.ok_or_else(|| RunError::message(format!("{} is empty", dir.display())))
}

/// `serial` of an emulator, else the first connected device.
pub(crate) fn device(sdk: &Sdk) -> Result<String, RunError> {
    let output = Command::new(sdk.adb())
        .arg("devices")
        .output()
        .map_err(|error| RunError::message(format!("adb: {error}")))?;
    if !output.status.success() {
        return Err(RunError::message(format!(
            "adb devices: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let mut emulator = None;
    let mut other = None;
    for line in String::from_utf8_lossy(&output.stdout).lines().skip(1) {
        let mut parts = line.split_whitespace();
        let Some(serial) = parts.next() else { continue };
        if parts.next() != Some("device") {
            continue;
        }
        if serial.starts_with("emulator-") {
            emulator.get_or_insert_with(|| serial.to_owned());
        } else {
            other.get_or_insert_with(|| serial.to_owned());
        }
    }
    emulator
        .or(other)
        .ok_or_else(|| RunError::message("no Android emulator or device. Start one, then re-run"))
}

pub(crate) fn device_abi(sdk: &Sdk, serial: &str) -> Result<Abi, RunError> {
    let output = adb(sdk, serial, &["shell", "getprop", "ro.product.cpu.abi"])?;
    Abi::parse(output.trim()).ok_or_else(|| {
        RunError::message(format!(
            "unsupported device ABI {output:?} (arm64-v8a, armeabi-v7a or x86_64)"
        ))
    })
}

pub(crate) struct Abi {
    name: &'static str,
    triple: &'static str,
    clang: &'static str,
}

impl Abi {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "arm64-v8a" => Some(Self {
                name: "arm64-v8a",
                triple: "aarch64-linux-android",
                clang: "aarch64-linux-android",
            }),
            "armeabi-v7a" => Some(Self {
                name: "armeabi-v7a",
                triple: "armv7-linux-androideabi",
                clang: "armv7a-linux-androideabi",
            }),
            "x86_64" => Some(Self {
                name: "x86_64",
                triple: "x86_64-linux-android",
                clang: "x86_64-linux-android",
            }),
            _ => None,
        }
    }
}

pub(crate) fn host_apk(sdk: &Sdk, abi: &Abi, release: bool) -> Result<PathBuf, RunError> {
    let workspace = workspace_root()
        .ok_or_else(|| RunError::message("no Android runtime. Run nlayer from a checkout"))?;
    let profile = if release { "release" } else { "debug" };
    let apk = workspace
        .join("target/nlayer-android")
        .join(profile)
        .join(format!("nlayer-{}.apk", abi.name));
    let sources = newest_source(&workspace);
    if apk
        .metadata()
        .and_then(|meta| meta.modified())
        .is_ok_and(|modified| modified >= sources)
    {
        return Ok(apk);
    }
    eprintln!("building the Android runtime ({})", abi.name);
    let library = cargo_lib(sdk, &workspace, abi, release)?;
    let dex = dex_host(sdk, &workspace)?;
    if let Some(parent) = apk.parent() {
        fs::create_dir_all(parent)?;
    }
    package(sdk, &library, &dex, abi, &apk)?;
    Ok(apk)
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
        "nlayer-platform-android",
    ] {
        consider_tree(&workspace.join("crates").join(name), &mut newest);
    }
    newest
}

fn cargo_lib(sdk: &Sdk, workspace: &Path, abi: &Abi, release: bool) -> Result<PathBuf, RunError> {
    let toolchain = ndk_toolchain(sdk)?;
    let bin = toolchain.join("bin");
    let clang = bin.join(format!("{}{MIN_SDK}-clang", abi.clang));
    let env_triple = abi.triple.replace('-', "_");
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command
        .args([
            "build",
            "-p",
            "nlayer-platform-android",
            "--lib",
            "--target",
            abi.triple,
            "--target-dir",
        ])
        .arg(workspace.join("target/nlayer-android"))
        .current_dir(workspace)
        .env(format!("CC_{env_triple}"), &clang)
        .env(format!("AR_{env_triple}"), bin.join("llvm-ar"))
        .env(
            format!("CARGO_TARGET_{}_LINKER", env_triple.to_uppercase()),
            &clang,
        )
        .env(
            format!("BINDGEN_EXTRA_CLANG_ARGS_{env_triple}"),
            format!("--sysroot=\"{}\"", toolchain.join("sysroot").display()),
        );
    if release {
        command.arg("--release");
    }
    let status = command
        .status()
        .map_err(|error| RunError::message(format!("cargo: {error}")))?;
    if !status.success() {
        return Err(RunError::message("the Android runtime failed to build"));
    }
    let profile = if release { "release" } else { "debug" };
    Ok(workspace
        .join("target/nlayer-android")
        .join(abi.triple)
        .join(profile)
        .join("libnlayer.so"))
}

fn ndk_toolchain(sdk: &Sdk) -> Result<PathBuf, RunError> {
    let ndk = match env::var_os("ANDROID_NDK_HOME") {
        Some(path) => PathBuf::from(path),
        None => newest(&sdk.root.join("ndk"))?,
    };
    let host = if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else if cfg!(windows) {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    };
    let toolchain = ndk.join("toolchains/llvm/prebuilt").join(host);
    if toolchain.exists() {
        Ok(toolchain)
    } else {
        Err(RunError::message(format!(
            "NDK toolchain missing: {}",
            toolchain.display()
        )))
    }
}

fn dex_host(sdk: &Sdk, workspace: &Path) -> Result<PathBuf, RunError> {
    let java = workspace.join("crates/nlayer-platform-android/java");
    let work = workspace.join("target/nlayer-android/java");
    let classes = work.join("classes");
    let _ = fs::remove_dir_all(&classes);
    fs::create_dir_all(&classes)?;
    let sources = java_sources(&java)?;
    let javac = Command::new("javac")
        .arg("-source")
        .arg("8")
        .arg("-target")
        .arg("8")
        .arg("-classpath")
        .arg(sdk.android_jar()?)
        .arg("-d")
        .arg(&classes)
        .args(&sources)
        .status()
        .map_err(|error| RunError::message(format!("javac: {error}")))?;
    if !javac.success() {
        return Err(RunError::message("the Android host failed to compile"));
    }
    let class_files = class_files(&classes)?;
    let dex = work.join("classes.dex");
    let d8 = Command::new(sdk.tool("d8"))
        .arg("--min-api")
        .arg(MIN_SDK.to_string())
        .arg("--lib")
        .arg(sdk.android_jar()?)
        .arg("--output")
        .arg(&work)
        .args(&class_files)
        .status()
        .map_err(|error| RunError::message(format!("d8: {error}")))?;
    if !d8.success() {
        return Err(RunError::message("d8 failed to dex the Android host"));
    }
    if dex.exists() {
        Ok(dex)
    } else {
        Err(RunError::message("d8 did not write classes.dex"))
    }
}

fn java_sources(dir: &Path) -> Result<Vec<PathBuf>, RunError> {
    let mut out = Vec::new();
    collect_java(dir, &mut out)?;
    if out.is_empty() {
        return Err(RunError::message(format!(
            "no Java sources in {}",
            dir.display()
        )));
    }
    Ok(out)
}

fn collect_java(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), RunError> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_java(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "java") {
            out.push(path);
        }
    }
    Ok(())
}

fn class_files(dir: &Path) -> Result<Vec<PathBuf>, RunError> {
    let mut out = Vec::new();
    collect_class(dir, &mut out)?;
    Ok(out)
}

fn collect_class(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), RunError> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_class(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "class") {
            out.push(path);
        }
    }
    Ok(())
}

fn package(sdk: &Sdk, library: &Path, dex: &Path, abi: &Abi, out: &Path) -> Result<(), RunError> {
    let work = out.with_extension("work");
    if work.exists() {
        fs::remove_dir_all(&work)?;
    }
    fs::create_dir_all(&work)?;
    let manifest = work.join("AndroidManifest.xml");
    fs::write(&manifest, manifest_xml())?;
    let base = work.join("base.apk");
    run(Command::new(sdk.tool("aapt2"))
        .arg("link")
        .arg("-o")
        .arg(&base)
        .arg("-I")
        .arg(sdk.android_jar()?)
        .arg("--manifest")
        .arg(&manifest)
        .args(["--min-sdk-version", &MIN_SDK.to_string()])
        .args(["--target-sdk-version", &TARGET_SDK.to_string()])
        .args(["--debug-mode"]))?;
    let unaligned = work.join("unaligned.apk");
    add_payload(&base, &unaligned, library, dex, abi)?;
    let aligned = work.join("aligned.apk");
    run(Command::new(sdk.tool("zipalign"))
        .args(["-P", "16", "-f", "4"])
        .arg(&unaligned)
        .arg(&aligned))?;
    sign(sdk, &aligned)?;
    fs::rename(&aligned, out)?;
    fs::remove_dir_all(&work)?;
    Ok(())
}

fn manifest_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="{PACKAGE}">
    <uses-permission android:name="android.permission.INTERNET" />
    <application android:label="nlayer" android:usesCleartextTraffic="true"
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
"#
    )
}

fn add_payload(
    base: &Path,
    out: &Path,
    library: &Path,
    dex: &Path,
    abi: &Abi,
) -> Result<(), RunError> {
    let mut input = ZipArchive::new(fs::File::open(base)?).map_err(zip_err)?;
    let mut zip = ZipWriter::new(fs::File::create(out)?);
    for index in 0..input.len() {
        zip.raw_copy_file(input.by_index_raw(index).map_err(zip_err)?)
            .map_err(zip_err)?;
    }
    let deflate = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("classes.dex", deflate).map_err(zip_err)?;
    zip.write_all(&fs::read(dex)?)?;
    zip.start_file(format!("lib/{}/libnlayer.so", abi.name), stored)
        .map_err(zip_err)?;
    zip.write_all(&fs::read(library)?)?;
    zip.finish().map_err(zip_err)?;
    Ok(())
}

fn sign(sdk: &Sdk, apk: &Path) -> Result<(), RunError> {
    let keystore = debug_keystore()?;
    run(Command::new(sdk.tool("apksigner"))
        .args(["sign", "--ks"])
        .arg(&keystore)
        .args(["--ks-key-alias", "androiddebugkey"])
        .args(["--ks-pass", "pass:android"])
        .args(["--key-pass", "pass:android"])
        .arg(apk))
}

fn debug_keystore() -> Result<PathBuf, RunError> {
    let home = env::var_os("HOME").ok_or_else(|| RunError::message("HOME is not set"))?;
    let dir = PathBuf::from(home).join(".android");
    let path = dir.join("debug.keystore");
    if path.exists() {
        return Ok(path);
    }
    fs::create_dir_all(&dir)?;
    run(Command::new("keytool")
        .args(["-genkeypair", "-keystore"])
        .arg(&path)
        .args([
            "-storepass",
            "android",
            "-alias",
            "androiddebugkey",
            "-keypass",
            "android",
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "10000",
            "-dname",
            "CN=Android Debug,O=Android,C=US",
        ]))
    .map_err(|error| RunError::message(format!("{error} (is a JDK installed?)")))?;
    Ok(path)
}

pub(crate) fn install_if_needed(sdk: &Sdk, serial: &str, apk: &Path) -> Result<(), RunError> {
    let stamp = apk.with_extension(format!("installed-{serial}"));
    let apk_time = apk.metadata().and_then(|meta| meta.modified()).ok();
    let installed = stamp.metadata().and_then(|meta| meta.modified()).ok();
    if installed.is_some_and(|time| Some(time) >= apk_time) {
        return Ok(());
    }
    eprintln!("installing {PACKAGE}");
    adb(sdk, serial, &["install", "-r", &apk.to_string_lossy()])?;
    fs::write(&stamp, b"")?;
    Ok(())
}

pub(crate) fn publish(
    sdk: &Sdk,
    serial: &str,
    app_dir: &Path,
    bundled: &Bundle,
) -> Result<(), RunError> {
    let _ = adb(
        sdk,
        serial,
        &[
            "shell",
            "run-as",
            PACKAGE,
            "rm",
            "-f",
            "files/nlayer-test.json",
        ],
    );
    push_bytes(sdk, serial, "files/app.js", bundled.code.as_bytes())?;
    for rel in &bundled.assets {
        let Some(rel) = asset_rel(rel) else {
            return Err(RunError::message(format!(
                "asset path escapes the app directory: {}",
                rel.display()
            )));
        };
        let bytes = fs::read(app_dir.join(rel))?;
        let remote = Path::new("files/assets").join(rel);
        push_bytes(
            sdk,
            serial,
            &remote.to_string_lossy().replace('\\', "/"),
            &bytes,
        )?;
    }
    Ok(())
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

/// Copies `bytes` into the app's private files via `run-as`.
pub(crate) fn push_bytes(
    sdk: &Sdk,
    serial: &str,
    remote: &str,
    bytes: &[u8],
) -> Result<(), RunError> {
    if let Some(parent) = Path::new(remote).parent()
        && parent != Path::new("")
    {
        adb(
            sdk,
            serial,
            &[
                "shell",
                "run-as",
                PACKAGE,
                "mkdir",
                "-p",
                &parent.to_string_lossy(),
            ],
        )?;
    }
    let tag = remote.replace('/', "_");
    let local = std::env::temp_dir().join(format!("nlayer-push-{}-{tag}", std::process::id()));
    fs::write(&local, bytes)?;
    let tmp = format!("/data/local/tmp/nlayer-{}-{tag}", std::process::id());
    let pushed = Command::new(sdk.adb())
        .args(["-s", serial, "push", &local.to_string_lossy(), &tmp])
        .output()
        .map_err(|error| RunError::message(format!("adb push: {error}")))?;
    let _ = fs::remove_file(&local);
    if !pushed.status.success() {
        return Err(RunError::message(format!(
            "adb push failed: {}",
            String::from_utf8_lossy(&pushed.stderr)
        )));
    }
    // `run-as` cannot read `/data/local/tmp` on every image, so the shell
    // cats it across the namespace boundary.
    adb(
        sdk,
        serial,
        &[
            "shell",
            &format!("cat {tmp} | run-as {PACKAGE} sh -c 'cat > {remote}'"),
        ],
    )?;
    let _ = adb(sdk, serial, &["shell", "rm", "-f", &tmp]);
    Ok(())
}

pub(crate) fn relaunch(sdk: &Sdk, serial: &str) -> Result<(), RunError> {
    let _ = adb(sdk, serial, &["shell", "am", "force-stop", PACKAGE]);
    adb(
        sdk,
        serial,
        &[
            "shell",
            "am",
            "start",
            "-n",
            &format!("{PACKAGE}/{ACTIVITY}"),
        ],
    )?;
    Ok(())
}

pub(crate) fn adb(sdk: &Sdk, serial: &str, args: &[&str]) -> Result<String, RunError> {
    let output = Command::new(sdk.adb())
        .arg("-s")
        .arg(serial)
        .args(args)
        .output()
        .map_err(|error| RunError::message(format!("adb: {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(RunError::message(format!(
            "adb {} failed: {stderr}{stdout}",
            args.first().unwrap_or(&"")
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn zip_err(error: zip::result::ZipError) -> RunError {
    RunError::message(error.to_string())
}

fn run(command: &mut Command) -> Result<(), RunError> {
    let status = command
        .status()
        .map_err(|error| RunError::message(format!("{error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(RunError::message(format!(
            "{} failed",
            command.get_program().to_string_lossy()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::Abi;

    #[test]
    fn device_abis_map_to_rust_triples() {
        assert_eq!(
            Abi::parse("arm64-v8a").unwrap().triple,
            "aarch64-linux-android"
        );
        assert_eq!(
            Abi::parse("armeabi-v7a").unwrap().triple,
            "armv7-linux-androideabi"
        );
        assert_eq!(Abi::parse("x86_64").unwrap().clang, "x86_64-linux-android");
        assert!(Abi::parse("x86").is_none());
    }
}
