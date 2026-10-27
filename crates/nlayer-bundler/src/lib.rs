//! The app's JS compiler: TypeScript removed and JSX lowered to the
//! runtime's calls, written in Rust end to end (no Node.js).
//!
//! Each `.tsx`/`.jsx` module is compiled against the native modules
//! (`@nlayer`, `nlayer:rt`), which stay imports. Apps are single modules
//! for now: their own imports are the next slice (a module graph).

mod compile;
mod jsx;

/// Compiles one module (TSX, JSX, TS or JS, as `name`'s extension says) to
/// the ES module the runtime runs: TypeScript removed, JSX compiled, imports
/// kept. Errors come back as text with a code frame.
pub fn compile_module(name: &str, source: &str) -> Result<String, String> {
    let path = std::path::Path::new(name);
    let source_type = oxc::span::SourceType::from_path(path).map_err(|e| e.to_string())?;
    let modules = compile::NativeModules::new();
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
            "import { mount, signal, View } from \"@nlayer\";\n\
             const n: number = 1;\n\
             function App() { const c = signal(n); return <View onPress={() => c.set(2)} />; }\n\
             mount(App);\n",
        )
        .unwrap();
        assert!(code.contains("from \"nlayer:rt\""), "{code}");
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
