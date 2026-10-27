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
    plugin::{
        HookLoadArgs, HookLoadReturn, HookResolveIdArgs, HookResolveIdOutput, HookResolveIdReturn,
        HookTransformArgs, HookTransformOutput, HookTransformReturn, HookUsage, Plugin,
        PluginContext, SharedTransformPluginContext,
    },
};

use crate::compile::{NativeModules, compile_native};

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
        if spec == "solid-js" || spec.starts_with("solid-js/") {
            return Err(self.refuse(format!(
                "{from}`{spec}` is not part of nlayer apps: import signal, computed, effect, batch, untracked, cleanup, context and mount from \"@nlayer\""
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
        _args: &HookLoadArgs<'_>,
    ) -> HookLoadReturn {
        Ok(None)
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
