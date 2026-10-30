//! The prebuilt runtime: native executables and libraries that apps are
//! packaged with, so building an app never needs Rust.
//!
//! Distribution layout, one directory per platform:
//!
//! ```text
//! runtime/android/classes.dex                   the Java host
//! runtime/android/lib/arm64-v8a/libnlayer.so    the engine, per ABI
//! runtime/android/lib/armeabi-v7a/libnlayer.so
//! runtime/android/lib/x86_64/libnlayer.so
//! runtime/ios/iphonesimulator/nlayer-ios-app    the simulator host
//! runtime/ios/iphoneos/nlayer-ios-app           the device runner
//! ```
//!
//! `npm/cli/bin/nlayer.js` sets `NLAYER_RUNTIME_ANDROID` and
//! `NLAYER_RUNTIME_IOS` to the `runtime/android` and `runtime/ios`
//! directories of `@nlayer/runtime-android` and `@nlayer/runtime-ios`, which
//! it resolves through `node_modules`. Without those variables the same
//! directories are read next to the binary (`../runtime/<platform>`), the
//! layout of a distribution tree.
//!
//! A cargo checkout has neither: the callers build the runtime from the
//! workspace instead, so a run never uses a missing or stale host.

use std::{env, path::PathBuf};

use crate::RunError;

/// The Android runtime: the Java host and the engine every packaged ABI uses.
#[derive(Debug)]
pub(crate) struct AndroidRuntime {
    /// `classes.dex`, the Java host the APK's activity comes from.
    pub(crate) dex: PathBuf,
    /// The directory `lib/<abi>/libnlayer.so` files live under.
    pub(crate) lib: PathBuf,
}

impl AndroidRuntime {
    /// `libnlayer.so` for `abi` (`arm64-v8a`, `armeabi-v7a`, `x86_64`).
    pub(crate) fn library(&self, abi: &str) -> Result<PathBuf, RunError> {
        let path = self.lib.join(abi).join("libnlayer.so");
        if path.is_file() {
            Ok(path)
        } else {
            Err(RunError::message(format!(
                "the Android runtime has no {abi} library: {}",
                path.display()
            )))
        }
    }
}

/// The iOS hosts, both named `nlayer-ios-app` and built per target: the
/// simulator one `nlayer run` installs, the device one `nlayer package`
/// copies into the `.app`.
#[derive(Debug)]
pub(crate) struct IosRuntime {
    /// The `aarch64-apple-ios-sim` host.
    pub(crate) simulator: Option<PathBuf>,
    /// The `aarch64-apple-ios` runner.
    pub(crate) device: Option<PathBuf>,
}

/// The installed Android runtime, if there is one.
pub(crate) fn android() -> Result<Option<AndroidRuntime>, RunError> {
    let Some(dir) = locate("NLAYER_RUNTIME_ANDROID", "android")? else {
        return Ok(None);
    };
    let runtime = AndroidRuntime {
        dex: dir.join("classes.dex"),
        lib: dir.join("lib"),
    };
    if runtime.dex.is_file() && runtime.lib.is_dir() {
        Ok(Some(runtime))
    } else {
        Err(RunError::message(format!(
            "{} is not an Android runtime: no classes.dex or lib/<abi>/libnlayer.so",
            dir.display()
        )))
    }
}

/// The installed iOS runtime, if there is one.
pub(crate) fn ios() -> Result<Option<IosRuntime>, RunError> {
    let Some(dir) = locate("NLAYER_RUNTIME_IOS", "ios")? else {
        return Ok(None);
    };
    let simulator = dir.join("iphonesimulator").join("nlayer-ios-app");
    let device = dir.join("iphoneos").join("nlayer-ios-app");
    let runtime = IosRuntime {
        simulator: simulator.is_file().then_some(simulator),
        device: device.is_file().then_some(device),
    };
    if runtime.simulator.is_some() || runtime.device.is_some() {
        Ok(Some(runtime))
    } else {
        Err(RunError::message(format!(
            "{} is not an iOS runtime: no iphonesimulator/nlayer-ios-app or iphoneos/nlayer-ios-app",
            dir.display()
        )))
    }
}

/// The directory `variable` names, else `../runtime/<platform>` next to the
/// binary when that is a directory; `None` when neither is there. A set
/// variable that names no directory is an error: it would otherwise run a
/// runtime the caller did not choose.
fn locate(variable: &str, platform: &str) -> Result<Option<PathBuf>, RunError> {
    if let Some(value) = env::var_os(variable) {
        let dir = PathBuf::from(value);
        if dir.is_dir() {
            return Ok(Some(dir));
        }
        return Err(RunError::message(format!(
            "{variable} {} is not a directory",
            dir.display()
        )));
    }
    let exe = env::current_exe()
        .map_err(|error| RunError::message(format!("cannot locate the nlayer binary: {error}")))?;
    let Some(bin) = exe.parent() else {
        return Ok(None);
    };
    let dir = bin.join("../runtime").join(platform);
    Ok(dir.is_dir().then_some(dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory under the target dir, emptied for the test that owns it.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/runtime-tests")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    #[test]
    fn installed_runtimes_are_read_from_the_named_directories() {
        let android_dir = scratch("android");
        std::fs::create_dir_all(android_dir.join("lib/arm64-v8a")).expect("lib");
        std::fs::write(android_dir.join("classes.dex"), b"dex").expect("dex");
        std::fs::write(android_dir.join("lib/arm64-v8a/libnlayer.so"), b"so").expect("library");

        let ios_dir = scratch("ios");
        std::fs::create_dir_all(ios_dir.join("iphonesimulator")).expect("simulator");
        std::fs::create_dir_all(ios_dir.join("iphoneos")).expect("device");
        std::fs::write(ios_dir.join("iphonesimulator/nlayer-ios-app"), b"host").expect("host");
        std::fs::write(ios_dir.join("iphoneos/nlayer-ios-app"), b"runner").expect("runner");

        // One test sets the variables, so no other test reads them while it runs.
        unsafe {
            env::set_var("NLAYER_RUNTIME_ANDROID", &android_dir);
            env::set_var("NLAYER_RUNTIME_IOS", &ios_dir);
        }

        let found = android().expect("android runtime").expect("installed");
        assert_eq!(found.dex, android_dir.join("classes.dex"));
        assert_eq!(
            found.library("arm64-v8a").expect("library"),
            android_dir.join("lib/arm64-v8a/libnlayer.so")
        );
        assert!(found.library("x86_64").is_err(), "x86_64 is not there");

        let found = ios().expect("ios runtime").expect("installed");
        assert_eq!(
            found.simulator,
            Some(ios_dir.join("iphonesimulator/nlayer-ios-app"))
        );
        assert_eq!(found.device, Some(ios_dir.join("iphoneos/nlayer-ios-app")));

        unsafe {
            env::set_var("NLAYER_RUNTIME_ANDROID", android_dir.join("missing"));
            env::set_var("NLAYER_RUNTIME_IOS", ios_dir.join("missing"));
        }
        assert!(
            android().is_err(),
            "a directory that is not there is an error"
        );
        assert!(ios().is_err(), "a directory that is not there is an error");

        unsafe {
            env::set_var("NLAYER_RUNTIME_ANDROID", scratch("empty"));
            env::set_var("NLAYER_RUNTIME_IOS", scratch("empty-ios"));
        }
        assert!(
            android().is_err(),
            "a directory without a runtime is an error"
        );
        assert!(ios().is_err(), "a directory without a runtime is an error");
    }
}
