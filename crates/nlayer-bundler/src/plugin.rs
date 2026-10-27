//! Rolldown plugin: compiles the app's modules with nlayer's JSX compiler
//! against the native modules (kept as imports).

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use oxc::span::SourceType;
use rolldown::{
    ModuleType,
    plugin::HookLoadOutput,
    plugin::{
        HookLoadArgs, HookLoadReturn, HookResolveIdArgs, HookResolveIdOutput, HookResolveIdReturn,
        HookTransformArgs, HookTransformOutput, HookTransformReturn, HookUsage, Plugin,
        PluginContext, SharedTransformPluginContext,
    },
};

use crate::compile::{NativeModules, compile_native};

/// File types imported as image assets.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// File types v1 does not draw (SVG and GIF are later).
const LATER_EXTENSIONS: &[&str] = &["svg", "gif"];

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// The density the file name says (`logo@2x.png`): 2, or 1.
fn density(path: &Path) -> f64 {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    match stem.rsplit_once('@').and_then(|(_, d)| d.strip_suffix('x')) {
        Some(d) => d.parse().unwrap_or(1.0),
        None => 1.0,
    }
}

/// Whether the file name says its density (`logo@2x.png`).
fn has_density(path: &Path) -> bool {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    stem.rsplit_once('@')
        .and_then(|(_, d)| d.strip_suffix('x'))
        .is_some_and(|d| d.parse::<f64>().is_ok())
}

/// The pixel-density variants of image `path` (a file without a density
/// suffix): its siblings `<stem>@<scale>x.<ext>` for the scales above 1
/// that exist. Stat calls only: no directory listing.
fn variants(path: &Path) -> Vec<(f32, PathBuf)> {
    if has_density(path) {
        return Vec::new();
    }
    let (Some(stem), Some(ext)) = (
        path.file_stem().and_then(|s| s.to_str()),
        path.extension().and_then(|e| e.to_str()),
    ) else {
        return Vec::new();
    };
    nlayer_core::IMAGE_SCALES
        .into_iter()
        .filter(|s| *s != 1.0)
        .map(|s| (s, path.with_file_name(format!("{stem}@{s}x.{ext}"))))
        .filter(|(_, p)| p.is_file())
        .collect()
}

/// The module of an image import: its asset URL and its size in points,
/// and the scales it ships in when it has variants.
fn image_module(url: &str, (w, h): (f64, f64), scales: &[f32]) -> String {
    let scales = if scales.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = std::iter::once(1.0)
            .chain(scales.iter().copied())
            .map(|s| s.to_string())
            .collect();
        format!(", scales: [{}]", list.join(", "))
    };
    format!("export default {{ url: {url:?}, size: {{ width: {w}, height: {h} }}{scales} }};")
}

/// Compiles the app's modules and keeps the native modules external.
#[derive(Debug)]
pub(crate) struct NlayerPlugin {
    /// Paths in error messages are shown relative to this directory.
    pub(crate) root: PathBuf,
    /// The first compile error, already formatted. Rolldown wraps plugin
    /// errors in its own report; the bundler shows this one instead.
    pub(crate) error: Arc<Mutex<Option<String>>>,
    /// The native modules there are.
    pub(crate) modules: Arc<NativeModules>,
    /// Asset files imported by the app (absolute paths).
    pub(crate) assets: Arc<Mutex<Vec<PathBuf>>>,
}

impl NlayerPlugin {
    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn fail(&self, path: &Path, message: &str) -> anyhow::Error {
        let message = format!("{}: {message}", self.rel(path));
        *self.error.lock().expect("error lock") = Some(message.clone());
        anyhow::anyhow!(message)
    }

    fn refuse(&self, message: String) -> anyhow::Error {
        *self.error.lock().expect("error lock") = Some(message.clone());
        anyhow::anyhow!(message)
    }
}

impl Plugin for NlayerPlugin {
    fn name(&self) -> Cow<'static, str> {
        "nlayer".into()
    }

    fn register_hook_usage(&self) -> HookUsage {
        HookUsage::ResolveId | HookUsage::Load | HookUsage::Transform
    }

    async fn resolve_id(
        &self,
        _cx: &PluginContext,
        args: &HookResolveIdArgs<'_>,
    ) -> HookResolveIdReturn {
        let spec = args.specifier;
        if self.modules.contains(spec) {
            // Imports of the native modules stay.
            return Ok(Some(HookResolveIdOutput {
                external: Some(true.into()),
                ..HookResolveIdOutput::from_id(spec)
            }));
        }
        let from = args
            .importer
            .map(|i| format!("{}: ", self.rel(Path::new(i))))
            .unwrap_or_default();
        if spec == "@nlayer" {
            return Err(self.refuse(format!(
                "{from}`@nlayer` is not a module: import signal, computed, effect, batch, untracked, cleanup, context, mount and ErrorBoundary from \"@nlayer/runtime\", and View, Text, Image, Scroll and Input from \"@nlayer/ui\""
            )));
        }
        if spec == "@nlayer/jsx-runtime" {
            return Err(self.refuse(format!(
                "{from}`@nlayer/jsx-runtime` is types only (`jsxImportSource` is \"@nlayer\"); compiled JSX imports \"nlayer:rt\", which apps do not write"
            )));
        }
        if spec == "solid-js" || spec.starts_with("solid-js/") {
            return Err(self.refuse(format!(
                "{from}`{spec}` is not part of nlayer apps: import signal, computed, effect, batch, untracked, cleanup, context, mount and ErrorBoundary from \"@nlayer/runtime\""
            )));
        }
        // Node's modules are not there: the runtime has the web APIs only.
        if spec.starts_with("node:")
            || [
                "events",
                "assert",
                "util",
                "string_decoder",
                "fs",
                "path",
                "os",
            ]
            .contains(&spec)
        {
            return Err(self.refuse(format!("{from}nlayer has no module `{spec}`")));
        }
        Ok(None)
    }

    async fn load(
        &self,
        _cx: rolldown::plugin::SharedLoadPluginContext,
        args: &HookLoadArgs<'_>,
    ) -> HookLoadReturn {
        let path = Path::new(args.id);
        if has_extension(path, LATER_EXTENSIONS) {
            return Err(self.fail(
                path,
                "SVG and GIF images are not part of v1 (PNG, JPEG and WebP are)",
            ));
        }
        if !has_extension(path, IMAGE_EXTENSIONS) {
            return Ok(None);
        }
        let rel = self.rel(path);
        // Its natural size in points: pixels over the density its file name
        // says.
        let dim = imagesize::size(path)
            .map_err(|e| self.fail(path, &format!("cannot read image: {e}")))?;
        let scale = density(path);
        let (w, h) = (dim.width as f64 / scale, dim.height as f64 / scale);
        let variants = variants(path);
        for (scale, variant) in &variants {
            let d = imagesize::size(variant)
                .map_err(|e| self.fail(variant, &format!("cannot read image: {e}")))?;
            let expected = (w * f64::from(*scale), h * f64::from(*scale));
            if (expected.0 - d.width as f64).abs() >= 1.0
                || (expected.1 - d.height as f64).abs() >= 1.0
            {
                log::warn!(
                    "{}: {}x{} pixels, expected {}x{} ({}x{} points at @{}x)",
                    self.rel(variant),
                    d.width,
                    d.height,
                    expected.0,
                    expected.1,
                    w,
                    h,
                    scale
                );
            }
        }
        {
            let mut assets = self.assets.lock().expect("assets lock");
            assets.push(path.to_path_buf());
            assets.extend(variants.iter().map(|(_, p)| p.clone()));
        }
        let scales: Vec<f32> = variants.iter().map(|(s, _)| *s).collect();
        let code = image_module(&format!("asset:/{rel}"), (w, h), &scales);
        Ok(Some(HookLoadOutput {
            code: code.into(),
            module_type: Some(ModuleType::Js),
            ..Default::default()
        }))
    }

    async fn transform(
        &self,
        _cx: SharedTransformPluginContext,
        args: &HookTransformArgs<'_>,
    ) -> HookTransformReturn {
        // Rolldown's own helpers.
        if args.id.starts_with('\0') {
            return Ok(None);
        }
        if !matches!(
            args.module_type,
            ModuleType::Tsx | ModuleType::Jsx | ModuleType::Ts | ModuleType::Js
        ) {
            return Ok(None);
        }
        let path = Path::new(args.id);
        let source_type = match args.module_type {
            ModuleType::Tsx => SourceType::tsx(),
            ModuleType::Jsx => SourceType::jsx(),
            ModuleType::Ts => SourceType::ts(),
            _ => SourceType::mjs(),
        };
        let compiled = compile_native(path, source_type, args.code, &self.modules)
            .map_err(|detail| self.fail(path, &detail))?;
        Ok(Some(HookTransformOutput {
            code: Some(compiled.code),
            module_type: Some(ModuleType::Js),
            ..Default::default()
        }))
    }
}
