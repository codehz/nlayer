//! `nlayer init` creates a TSX app. `nlayer build` writes its bundle, source
//! map, imported images and TypeScript declarations. `nlayer test` runs its
//! test files headless.

mod init;

use std::{
    fs,
    path::{Path, PathBuf},
};

pub use init::{InitError, InitOptions, InitReport, init};
use nlayer_bundler::{BundleError, BundleOptions, bundle};

/// What to build and where to write it.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// An entry module, or a directory whose `App.tsx` is the entry.
    pub entry: PathBuf,
    /// Output directory (`bundle.js`, `bundle.js.map`, assets, `types`).
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
    let types = options.out.join("types");
    for (path, text) in nlayer_bundler::type_files() {
        let to = types.join(path);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(to, text)?;
    }
    Ok(())
}

fn resolve_entry(path: &Path) -> Result<PathBuf, BuildError> {
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
    /// A test file, or a directory of them.
    pub path: PathBuf,
    /// Run only tests whose name contains this.
    pub filter: Option<String>,
}

/// Runs the tests [`TestOptions`] names.
pub fn test(options: &TestOptions) -> Result<nlayer_testing::Report, nlayer_testing::RunError> {
    nlayer_testing::run(&options.path, options.filter.as_deref())
}
