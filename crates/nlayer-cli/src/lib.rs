//! `nlayer init` creates a TSX app. `nlayer build` writes its bundle, source
//! map and imported images. `nlayer test` runs its
//! test files, or, given an app entry, opens it and runs the steps from
//! standard input. The default is headless; `--target web` runs the opened app
//! in a headless browser, `--target ios` in the simulator, and `--target
//! android` on an emulator or a connected device. `nlayer run --target web`
//! serves the app to a browser and reloads it after a rebuild. `nlayer run
//! --target ios` runs it in the simulator and restarts it after a rebuild,
//! without reinstalling the host. `nlayer run --target android` does the same
//! on an emulator or a connected device.

mod android;
mod android_test;
mod init;
mod ios;
mod ios_package;
mod ios_test;
mod run;
mod types_version;
mod web_package;
mod web_test;

use std::{
    fs,
    path::{Path, PathBuf},
};

pub use android::{
    AndroidJavaTests, AndroidOptions, AndroidSession, android_java_tests, run_android,
};
pub use init::{InitError, InitOptions, InitReport, init};
pub use ios::{IosHostTests, IosOptions, IosSession, ios_host_tests, run_ios};
pub use ios_package::package_ios;
use nlayer_bundler::{BundleError, BundleOptions, bundle};
pub use run::{BuildOutcome, DevServer, RunError, RunOptions, serve, web_module};
pub use types_version::type_package_warning;
pub use web_package::package_web;

/// What to build and where to write it.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// An entry module, or a directory whose `App.tsx` is the entry.
    pub entry: PathBuf,
    /// Output directory (`bundle.js`, `bundle.js.map`, assets).
    pub out: PathBuf,
    /// Minify the bundle.
    pub release: bool,
}

/// A failed build.
#[derive(Debug)]
pub enum BuildError {
    /// The bundler rejected the app.
    Bundle(BundleError),
    /// A file could not be written.
    Io(std::io::Error),
    /// The entry path is not an app.
    Message(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Bundle(err) => f.write_str(&err.colored),
            BuildError::Io(err) => write!(f, "{err}"),
            BuildError::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<BundleError> for BuildError {
    fn from(err: BundleError) -> Self {
        BuildError::Bundle(err)
    }
}

impl From<std::io::Error> for BuildError {
    fn from(err: std::io::Error) -> Self {
        BuildError::Io(err)
    }
}

/// Bundles `options.entry` and writes the output.
pub fn build(options: &BuildOptions) -> Result<(), BuildError> {
    let entry = resolve_entry(&options.entry)?;
    let bundled = bundle(&BundleOptions {
        entry: entry.clone(),
        minify: options.release,
        test: false,
    })?;
    let root = entry
        .parent()
        .ok_or_else(|| BuildError::Message("entry has no parent directory".to_owned()))?;
    fs::create_dir_all(&options.out)?;
    fs::write(options.out.join("bundle.js"), &bundled.code)?;
    fs::write(options.out.join("bundle.js.map"), &bundled.source_map)?;
    for rel in &bundled.assets {
        let to = options.out.join(rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(root.join(rel), &to)?;
    }
    Ok(())
}

pub(crate) fn resolve_entry(path: &Path) -> Result<PathBuf, BuildError> {
    let path = if path.is_dir() {
        path.join("App.tsx")
    } else {
        path.to_path_buf()
    };
    if path.is_file() {
        Ok(path)
    } else {
        Err(BuildError::Message(format!(
            "entry not found: {}",
            path.display()
        )))
    }
}

/// What `nlayer test` runs.
#[derive(Debug, Clone)]
pub struct TestOptions {
    /// A test file, a directory of them, or an app entry.
    pub path: PathBuf,
    /// Run only tests whose name contains this.
    pub filter: Option<String>,
    /// Host that runs the app `open` starts. `None` is the headless engine.
    pub target: Option<TestTarget>,
}

/// A host `nlayer test --target` can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestTarget {
    /// A headless browser. The page executes the app.
    Web,
    /// The iOS simulator. The steps go through UIKit.
    Ios,
    /// An Android emulator or a connected device. The steps go through
    /// the view system.
    Android,
}

/// Runs the tests [`TestOptions`] names.
pub fn test(options: &TestOptions) -> Result<nlayer_testing::Report, nlayer_testing::RunError> {
    match options.target {
        None => nlayer_testing::run(&options.path, options.filter.as_deref()),
        Some(TestTarget::Web) => {
            let wasm = crate::web_module()
                .map_err(|error| nlayer_testing::RunError::message(error.to_string()))?;
            let mut host = web_test::WebHost::new(wasm);
            nlayer_testing::run_with(&options.path, options.filter.as_deref(), &mut host)
        }
        Some(TestTarget::Ios) => {
            let mut host = ios_test::IosHost::new();
            nlayer_testing::run_with(&options.path, options.filter.as_deref(), &mut host)
        }
        Some(TestTarget::Android) => {
            let mut host = android_test::AndroidHost::new();
            nlayer_testing::run_with(&options.path, options.filter.as_deref(), &mut host)
        }
    }
}
