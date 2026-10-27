//! Bundling an app's modules (rolldown): one ES module holding the app's
//! code, with the native modules left as imports.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use rolldown::{Bundler, BundlerOptions, InputItem, OutputFormat, Platform, RawMinifyOptions};
use rolldown_common::{CodeSplittingMode, Output};

use crate::{compile::NativeModules, compile::strip_ansi, plugin};

/// What to bundle and how.
#[derive(Debug, Clone)]
pub struct BundleOptions {
    /// Entry module (usually `App.tsx`).
    pub entry: PathBuf,
    /// Minify the output (release builds).
    pub minify: bool,
}

/// A failed build, formatted for people.
#[derive(Debug, Clone)]
pub struct BundleError {
    /// With ANSI colors, for terminals.
    pub colored: String,
    /// Plain text, for on-device error screens.
    pub plain: String,
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.plain)
    }
}

impl std::error::Error for BundleError {}

impl BundleError {
    fn new(diagnostics: Vec<rolldown_error::BuildDiagnostic>) -> Self {
        let render = |color| {
            diagnostics
                .iter()
                .map(|d| d.to_diagnostic().convert_to_string(color))
                .collect::<Vec<_>>()
                .join("\n")
        };
        Self {
            colored: render(true),
            // Plugin messages may carry their own escape codes.
            plain: strip_ansi(&render(false)),
        }
    }

    fn other(err: impl std::fmt::Display) -> Self {
        let message = err.to_string();
        Self {
            colored: message.clone(),
            plain: message,
        }
    }
}

/// A bundled app.
#[derive(Debug, Clone)]
pub struct Bundle {
    /// The bundle: an ES module importing the native modules.
    pub code: String,
    /// Asset files the app imports, relative to the entry directory
    /// (`asset:/<path>` URIs refer to them).
    pub assets: Vec<PathBuf>,
    /// Source and configuration files read by the build, as absolute paths.
    pub watch_files: Vec<PathBuf>,
    /// Wall time spent bundling, in milliseconds.
    pub elapsed_ms: f64,
}

/// Bundles an app, blocking the current thread.
pub fn bundle(options: &BundleOptions) -> Result<Bundle, BundleError> {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(BundleError::other)?
        .block_on(bundle_async(options))
}

/// Bundles an app.
pub async fn bundle_async(options: &BundleOptions) -> Result<Bundle, BundleError> {
    let start = Instant::now();
    let entry = dunce::canonicalize(&options.entry).map_err(|e| {
        BundleError::other(format!("entry not found: {}: {e}", options.entry.display()))
    })?;
    let cwd = entry
        .parent()
        .ok_or_else(|| BundleError::other("entry has no parent directory"))?
        .to_path_buf();
    let plugin_error = Arc::new(Mutex::new(None));
    let assets = Arc::new(Mutex::new(Vec::new()));
    let cwd_root = entry
        .parent()
        .ok_or_else(|| BundleError::other("entry has no parent directory"))?
        .to_path_buf();
    let bundler_options = BundlerOptions {
        input: Some(vec![InputItem {
            import: entry.to_string_lossy().into_owned(),
            ..Default::default()
        }]),
        cwd: Some(cwd),
        // Nothing is written; this only makes source map paths relative
        // to the entry directory.
        dir: Some(".".into()),
        // Imports of the native modules stay.
        format: Some(OutputFormat::Esm),
        minify: Some(RawMinifyOptions::Bool(options.minify)),
        // The runtime has the web APIs, not Node's.
        platform: Some(Platform::Browser),
        // Dynamic imports are inlined: one module.
        code_splitting: Some(CodeSplittingMode::Bool(false)),
        ..Default::default()
    };
    let mut bundler = Bundler::with_plugins(
        bundler_options,
        vec![rolldown::plugin::Pluginable::new_shared(
            plugin::NlayerPlugin {
                root: cwd_root.clone(),
                error: plugin_error.clone(),
                modules: Arc::new(NativeModules::new()),
                assets: assets.clone(),
            },
        )],
    )
    .map_err(|e| BundleError::new(e.into_vec()))?;
    let output = bundler.generate().await.map_err(|e| {
        // Prefer our own compile error (clean code frame) over rolldown's
        // wrapped report of it.
        match plugin_error.lock().expect("error lock").take() {
            Some(message) => BundleError::other(message),
            None => BundleError::new(e.into_vec()),
        }
    })?;
    let mut code = None;
    for asset in output.assets {
        let Output::Chunk(chunk) = asset else {
            continue;
        };
        if chunk.is_entry {
            code = Some(chunk.code.clone());
        }
    }
    let code = code.ok_or_else(|| BundleError::other("bundle produced no entry chunk"))?;
    let mut assets: Vec<PathBuf> = std::mem::take(&mut *assets.lock().expect("assets lock"))
        .into_iter()
        .filter_map(|p| p.strip_prefix(&cwd_root).ok().map(PathBuf::from))
        .collect();
    assets.sort();
    assets.dedup();
    let mut watch_files: Vec<PathBuf> = bundler
        .watch_files()
        .iter()
        .map(|path| PathBuf::from(path.key().as_str()))
        .filter(|path| path.is_file())
        .collect();
    watch_files.sort();
    watch_files.dedup();
    Ok(Bundle {
        code,
        assets,
        watch_files,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}
