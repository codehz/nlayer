//! The app's JS compiler: TypeScript removed and JSX lowered to the
//! runtime's calls, written in Rust end to end (no Node.js).
//!
//! Each `.tsx`/`.jsx` module is compiled against the native modules
//! (`@nlayer/runtime`, `@nlayer/ui`, `nlayer:rt`), which stay imports; rolldown bundles the app's
//! own modules into one ES module of the app's code.

mod bundle;
mod compile;
mod jsx;
mod plugin;
mod test_api;
mod types;

pub use bundle::{Bundle, BundleError, BundleOptions, bundle, bundle_async};
pub use types::type_files;

/// Compiles one module (TSX, JSX, TS or JS, as `name`'s extension says) to
/// the ES module the runtime runs: TypeScript removed, JSX compiled, imports
/// kept. Errors come back as text with a code frame.
pub fn compile_module(name: &str, source: &str) -> Result<String, String> {
    let path = std::path::Path::new(name);
    let source_type = oxc::span::SourceType::from_path(path).map_err(|e| e.to_string())?;
    let modules = compile::NativeModules::new(false);
    let compiled = compile::compile_native(path, source_type, source, &modules)
        .map_err(|e| compile::strip_ansi(&e))?;
    Ok(compiled.code)
}

#[cfg(test)]
mod tests {
    #[test]
    fn compile_module_makes_a_runnable_module() {
        let code = super::compile_module(
            "App.tsx",
            "import { mount, signal } from \"@nlayer/runtime\";\n\
             import { View } from \"@nlayer/ui\";\n\
             const n: number = 1;\n\
             function App() { const c = signal(n); return <View onPress={() => c.set(2)} />; }\n\
             mount(App);\n",
        )
        .unwrap();
        assert!(code.contains("from \"nlayer:rt\""), "{code}");
        assert!(code.contains("from \"@nlayer/runtime\""), "{code}");
        assert!(!code.contains(": number"), "types are removed: {code}");
        assert!(!code.contains("<View"), "JSX is compiled: {code}");
    }

    #[test]
    fn compile_module_errors_have_a_code_frame_without_colors() {
        let err = super::compile_module("App.tsx", "const = ;").unwrap_err();
        assert!(err.contains("const = ;"), "{err}");
        assert!(!err.contains('\u{1b}'), "{err}");
    }
}
