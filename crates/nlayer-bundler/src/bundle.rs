//! Bundling an app's modules (rolldown). [`bundle`] emits one ES module,
//! with the native modules left as imports and a source map back to the
//! app's modules. [`bundle_hot`] keeps one CommonJS chunk per module so a
//! development update can replace just the ones that changed.

use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use rolldown::{
    Bundler, BundlerOptions, InputItem, OutputFormat, Platform, RawMinifyOptions, SourceMapType,
};
use rolldown_common::{CodeSplittingMode, Output};
use rolldown_sourcemap::{SourceJoiner, SourceMapSource};

use crate::{compile::NativeModules, compile::strip_ansi, plugin};

/// What to bundle and how.
#[derive(Debug, Clone)]
pub struct BundleOptions {
    /// Entry module (usually `App.tsx`).
    pub entry: PathBuf,
    /// Minify the output (release builds).
    pub minify: bool,
    /// Inline `@nlayer/test`. An app bundle leaves this false and refuses
    /// the import.
    pub test: bool,
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
    /// Source map (JSON) from [`Self::code`] back to the app's modules.
    /// Kept out of the code: no `sourceMappingURL` comment.
    pub source_map: String,
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
        // Kept out of the code; `build` writes the JSON beside the bundle.
        sourcemap: Some(SourceMapType::Hidden),
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
                modules: Arc::new(NativeModules::new(options.test)),
                assets: assets.clone(),
                test: options.test,
                hot: false,
                natives: Arc::new(Mutex::new(BTreeSet::new())),
                preloads: None,
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
    let mut entry = None;
    for asset in output.assets {
        let Output::Chunk(chunk) = asset else {
            continue;
        };
        if chunk.is_entry {
            let map = chunk
                .map
                .as_ref()
                .map(|m| m.to_json_string())
                .ok_or_else(|| BundleError::other("bundle produced no source map"))?;
            entry = Some((chunk.code.clone(), map));
        }
    }
    let (code, source_map) =
        entry.ok_or_else(|| BundleError::other("bundle produced no entry chunk"))?;
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
        source_map,
        assets,
        watch_files,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}

/// A development bundle with hot module replacement: the app's modules,
/// each kept separate so an update can replace just the ones that changed.
#[derive(Debug, Clone)]
pub struct HotBundle {
    modules: Vec<HotModule>,
    /// Module id of the entry.
    entry: String,
    /// All native modules the app imports.
    natives: Vec<String>,
    /// Native namespaces the factories require statically.
    preloads: Vec<String>,
    /// Asset files the app imports (as in [`Bundle::assets`]).
    pub assets: Vec<PathBuf>,
    /// Source and configuration files read by the build, as absolute paths.
    pub watch_files: Vec<PathBuf>,
    /// Wall time spent bundling, in milliseconds.
    pub elapsed_ms: f64,
}

#[derive(Debug, Clone)]
struct HotModule {
    /// Chunk file name, e.g. `App.js` (what `require` resolves to).
    id: String,
    code: String,
    map: Option<rolldown_sourcemap::SourceMap>,
    /// Rolldown's runtime helpers: never replaced in place.
    embedded: bool,
}

/// What a rebuild means for a running development bundle.
#[derive(Debug, Clone)]
pub enum HotPatch {
    /// No module changed.
    Unchanged,
    /// A classic script that replaces the changed modules in place. It
    /// evaluates to `false` when the update cannot be applied (the host
    /// reloads).
    Update {
        /// The script.
        code: String,
        /// Its source map (JSON).
        source_map: Option<String>,
    },
    /// The change needs a full reload.
    Reload,
}

/// Name of the hot runtime's global.
const HOT: &str = "__nlayer_hot";

impl HotBundle {
    /// Native modules the app imports.
    pub fn natives(&self) -> &[String] {
        &self.natives
    }

    /// The whole app as one script, an ES module: the native hot runtime
    /// and the app's static native imports, every module defined, and the
    /// start of the entry.
    pub fn script(&self) -> (String, Option<String>) {
        let mut joiner = SourceJoiner::default();
        joiner.enable_sourcemap = true;
        let mut imports = String::from("import \"nlayer:hot\";");
        let mut map = String::new();
        for (i, spec) in self.preloads.iter().enumerate() {
            imports.push_str(&format!("import * as __nlayer_n{i} from {spec:?};"));
            // CommonJS interop must select the real ESM default object, including its getters.
            map.push_str(&format!(
                "{spec:?}: {{...__nlayer_n{i}, __esModule: true}},"
            ));
        }
        joiner.append_source(imports);
        let start = format!("{HOT}.start({:?}, {{{map}}});", self.entry);
        for m in &self.modules {
            let open = format!(
                "{HOT}.define({:?}, function (module, exports, require) {{",
                m.id
            );
            let close = if m.embedded { "}, true);" } else { "});" };
            append_module(&mut joiner, m, &open, close);
        }
        joiner.append_source(start);
        let (code, map) = joiner.join();
        (code, map.map(|m| m.to_json_string()))
    }

    /// The update from `previous` (what hosts run) to this bundle.
    ///
    /// A different entry, a native or preload the running script did not
    /// import, or a change to an embedded runtime module reloads. Anything
    /// else is an in-place [`HotPatch::Update`].
    pub fn patch(&self, previous: &HotBundle) -> HotPatch {
        if self.entry != previous.entry {
            return HotPatch::Reload;
        }
        // A native module the running script did not import.
        if !self.natives.iter().all(|s| previous.natives.contains(s))
            || !self.preloads.iter().all(|s| previous.preloads.contains(s))
        {
            return HotPatch::Reload;
        }
        let old: HashMap<&str, &HotModule> = previous
            .modules
            .iter()
            .map(|m| (m.id.as_str(), m))
            .collect();
        let changed: Vec<&HotModule> = self
            .modules
            .iter()
            .filter(|m| old.get(m.id.as_str()).is_none_or(|o| o.code != m.code))
            .collect();
        if changed.is_empty() {
            return HotPatch::Unchanged;
        }
        if changed.iter().any(|m| m.embedded) {
            return HotPatch::Reload;
        }
        let mut joiner = SourceJoiner::default();
        joiner.enable_sourcemap = true;
        joiner.append_source(format!("{HOT}.update({{"));
        for m in changed {
            append_module(
                &mut joiner,
                m,
                &format!("{:?}: function (module, exports, require) {{", m.id),
                "},",
            );
        }
        joiner.append_source("});");
        let (code, map) = joiner.join();
        HotPatch::Update {
            code,
            source_map: map.map(|m| m.to_json_string()),
        }
    }
}

/// Appends a module's code between `open` and `close` (each on its own
/// line, so the module's own lines keep their source map).
fn append_module(joiner: &mut SourceJoiner<'_>, m: &HotModule, open: &str, close: &str) {
    joiner.append_source(open.to_owned());
    match &m.map {
        Some(map) => joiner.append_source(SourceMapSource::new(m.code.clone(), map.clone())),
        None => joiner.append_source(m.code.clone()),
    }
    joiner.append_source(close.to_owned());
}

/// Bundles an app for development with hot module replacement, blocking
/// the current thread. [`bundle`] is unchanged: one ES module, no hot runtime.
/// `test` inlines `@nlayer/test`, as in [`BundleOptions::test`].
pub fn bundle_hot(entry: &Path, test: bool) -> Result<HotBundle, BundleError> {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(BundleError::other)?
        .block_on(bundle_hot_async(entry, test))
}

/// Bundles an app for development with hot module replacement.
/// `test` inlines `@nlayer/test`, as in [`BundleOptions::test`].
pub async fn bundle_hot_async(entry: &Path, test: bool) -> Result<HotBundle, BundleError> {
    let start = Instant::now();
    let entry = dunce::canonicalize(entry)
        .map_err(|e| BundleError::other(format!("entry not found: {}: {e}", entry.display())))?;
    let cwd = entry
        .parent()
        .ok_or_else(|| BundleError::other("entry has no parent directory"))?
        .to_path_buf();
    let natives = Arc::new(Mutex::new(BTreeSet::new()));
    let preloads = Arc::new(Mutex::new(BTreeSet::new()));
    let plugin_error = Arc::new(Mutex::new(None));
    let assets = Arc::new(Mutex::new(Vec::new()));
    let bundler_options = BundlerOptions {
        input: Some(vec![InputItem {
            import: entry.to_string_lossy().into_owned(),
            ..Default::default()
        }]),
        cwd: Some(cwd.clone()),
        // Nothing is written; this only makes source map paths relative
        // to the entry directory.
        dir: Some(".".into()),
        // One CommonJS chunk per module: modules can be replaced one by
        // one, and imports are read at use, so a module run again in place
        // is seen by its importers.
        format: Some(OutputFormat::Cjs),
        preserve_modules: Some(true),
        // Every statement stays. Rolldown still leaves out the exports no
        // module imports: an edit that starts using one changes the
        // exporting module too, which then runs again.
        treeshake: rolldown_common::TreeshakeOptions::Boolean(false),
        sourcemap: Some(SourceMapType::Hidden),
        ..Default::default()
    };
    let mut bundler = Bundler::with_plugins(
        bundler_options,
        vec![rolldown::plugin::Pluginable::new_shared(
            plugin::NlayerPlugin {
                root: cwd.clone(),
                error: plugin_error.clone(),
                modules: Arc::new(NativeModules::new(test)),
                assets: assets.clone(),
                test,
                hot: true,
                natives: natives.clone(),
                preloads: Some(preloads.clone()),
            },
        )],
    )
    .map_err(|e| BundleError::new(e.into_vec()))?;
    let output = bundler.generate().await.map_err(|e| {
        match plugin_error.lock().expect("error lock").take() {
            Some(message) => BundleError::other(message),
            None => BundleError::new(e.into_vec()),
        }
    })?;
    let mut modules = Vec::new();
    let mut entry_id = None;
    for asset in output.assets {
        let Output::Chunk(chunk) = asset else {
            continue;
        };
        let id = chunk.filename.to_string();
        let embedded = chunk
            .module_ids
            .iter()
            .any(|m| m.as_str().starts_with('\0'));
        if chunk.is_entry {
            entry_id = Some(id.clone());
        }
        modules.push(HotModule {
            id,
            code: chunk.code.clone(),
            map: chunk.map.clone(),
            embedded,
        });
    }
    let mut assets: Vec<PathBuf> = std::mem::take(&mut *assets.lock().expect("assets lock"))
        .into_iter()
        .filter_map(|p| p.strip_prefix(&cwd).ok().map(PathBuf::from))
        .collect();
    assets.sort();
    assets.dedup();
    Ok(HotBundle {
        modules,
        natives: std::mem::take(&mut *natives.lock().expect("natives lock"))
            .into_iter()
            .collect(),
        preloads: std::mem::take(&mut *preloads.lock().expect("preloads lock"))
            .into_iter()
            .collect(),
        entry: entry_id.ok_or_else(|| BundleError::other("bundle produced no entry chunk"))?,
        assets,
        watch_files: watch_files(&bundler),
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}

fn watch_files(bundler: &Bundler) -> Vec<PathBuf> {
    let mut watch_files: Vec<PathBuf> = bundler
        .watch_files()
        .iter()
        .map(|path| PathBuf::from(path.key().as_str()))
        .filter(|path| path.is_file())
        .collect();
    watch_files.sort();
    watch_files.dedup();
    watch_files
}
