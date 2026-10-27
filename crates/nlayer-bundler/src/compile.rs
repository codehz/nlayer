//! One module to the JS the runtime runs: TypeScript removed and the JSX
//! compiled by nlayer's compiler ([`crate::jsx`]) in one oxc pass, with a
//! source map back to that module.

use std::path::Path;

use oxc::{
    allocator::Allocator,
    codegen::{Codegen, CodegenOptions},
    parser::Parser,
    semantic::SemanticBuilder,
    span::SourceType,
    transformer::{JsxOptions, TransformOptions, Transformer},
};
use oxc_sourcemap::SourceMap;

/// What the compiler knows of the native modules a host serves:
/// `@nlayer/runtime`, `@nlayer/ui` and the renderer's `nlayer:rt`.
#[derive(Debug, Default)]
pub(crate) struct NativeModules {
    specifiers: std::collections::BTreeSet<String>,
    /// The names each module exports, to check named imports against.
    exports: std::collections::BTreeMap<String, Vec<&'static str>>,
}

impl NativeModules {
    pub(crate) fn new() -> Self {
        let mut out = Self::default();
        for spec in ["@nlayer/runtime", "@nlayer/ui", "nlayer:rt"] {
            out.specifiers.insert(spec.to_owned());
            if let Some(names) = nlayer_js::native_module_exports(spec) {
                out.exports.insert(spec.to_owned(), names);
            }
        }
        out
    }

    /// Whether `spec` is a native module.
    pub(crate) fn contains(&self, spec: &str) -> bool {
        self.specifiers.contains(spec)
    }
}

/// A module compiled by [`compile_native`].
pub(crate) struct Compiled {
    pub(crate) code: String,
    /// Mappings from [`Self::code`] back to the module's source.
    pub(crate) map: SourceMap<'static>,
}

/// TSX -> JS with nlayer's JSX compiler ([`crate::jsx`]), TypeScript
/// removed in the same pass. Errors are rendered with a code frame.
pub(crate) fn compile_native(
    path: &Path,
    source_type: SourceType,
    source: &str,
    modules: &NativeModules,
) -> Result<Compiled, String> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if let Some(err) = parsed.diagnostics.into_iter().next() {
        return Err(err.render_with_source_code(source.to_owned()));
    }
    let mut program = parsed.program;
    let scoping = SemanticBuilder::new()
        .build(&program)
        .semantic
        .into_scoping();
    let options = TransformOptions {
        jsx: JsxOptions::disable(),
        // Imports are elided once the JSX is compiled
        // (`elide_unused_imports`): only `import type` goes here.
        typescript: oxc::transformer::TypeScriptOptions {
            only_remove_type_imports: true,
            ..Default::default()
        },
        ..TransformOptions::default()
    };
    let ret =
        Transformer::new(&allocator, path, &options).build_with_scoping(scoping, &mut program);
    if let Some(err) = ret.diagnostics.into_iter().next() {
        return Err(err.render_with_source_code(source.to_owned()));
    }
    crate::jsx::transform(&allocator, &mut program, &ret.scoping, source, modules)?;
    // The scopes once TypeScript and JSX are compiled, for checking imports.
    let scoping = SemanticBuilder::new()
        .build(&program)
        .semantic
        .into_scoping();
    if let Some(err) = check_named_imports(&program, &scoping, modules) {
        return Err(err.render_with_source_code(source.to_owned()));
    }
    // Plain JS keeps imports that may be present for their side effects.
    if source_type.is_typescript() || source_type.is_jsx() {
        elide_unused_imports(&mut program, &scoping);
    }
    let out = Codegen::new()
        .with_options(CodegenOptions {
            source_map_path: Some(path.to_path_buf()),
            ..CodegenOptions::default()
        })
        .build(&program);
    Ok(Compiled {
        code: out.code,
        map: out.map.expect("source map requested").into_owned(),
    })
}

/// The first named import of a native module that it does not export:
/// `import { createSignal } from "@nlayer/runtime"` bundles fine and only fails
/// when the app runs, so it is reported with its place in the source.
/// Only names the code uses as values count (`scoping` is that of the
/// program once TypeScript and the JSX are compiled): the declarations also
/// name types, which need no `type`.
fn check_named_imports(
    program: &oxc::ast::ast::Program<'_>,
    scoping: &oxc::semantic::Scoping,
    modules: &NativeModules,
) -> Option<oxc::diagnostics::OxcDiagnostic> {
    use oxc::{
        ast::ast::{ImportDeclarationSpecifier, Statement},
        diagnostics::OxcDiagnostic,
    };
    if modules.exports.is_empty() {
        return None;
    }
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else {
            continue;
        };
        let module = import.source.value.as_str();
        let (true, Some(exports), Some(specifiers)) = (
            !import.import_kind.is_type(),
            modules.exports.get(module),
            &import.specifiers,
        ) else {
            continue;
        };
        for specifier in specifiers {
            let ImportDeclarationSpecifier::ImportSpecifier(s) = specifier else {
                continue;
            };
            let name = s.imported.name();
            let used = s
                .local
                .symbol_id
                .get()
                .is_none_or(|id| !scoping.get_resolved_reference_ids(id).is_empty());
            if s.import_kind.is_type() || !used || exports.iter().any(|e| *e == name.as_str()) {
                continue;
            }
            let mut error = OxcDiagnostic::error(format!("`{module}` has no export `{name}`"))
                .with_label(oxc::span::GetSpan::span(&s.imported));
            if let Some(near) = closest_name(name.as_str(), exports) {
                error = error.with_help(format!("did you mean `{near}`?"));
            }
            return Some(error);
        }
    }
    None
}

/// The name in `names` closest to `name` (a typo or a rename), if any is
/// close enough to suggest.
fn closest_name<'a>(name: &str, names: &'a [&'static str]) -> Option<&'a str> {
    let name = name.to_lowercase();
    let limit = (name.chars().count() / 3).clamp(1, 3);
    names
        .iter()
        .map(|n| (edit_distance(&name, &n.to_lowercase()), *n))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| n)
}

/// Levenshtein distance.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != *cb))
                .min(above + 1)
                .min(row[j] + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

/// TypeScript's import elision, once the JSX is compiled (a primitive's
/// tag made in place no longer references its import): import specifiers
/// nothing references go (an import of types only, without `type`, or of a
/// primitive only its tags use), and declarations left without any.
/// `scoping` is the program's.
fn elide_unused_imports(
    program: &mut oxc::ast::ast::Program<'_>,
    scoping: &oxc::semantic::Scoping,
) {
    use oxc::ast::ast::{ImportDeclarationSpecifier as S, Statement};
    let used = |local: &oxc::ast::ast::BindingIdentifier<'_>| {
        local
            .symbol_id
            .get()
            .is_none_or(|id| !scoping.get_resolved_reference_ids(id).is_empty())
    };
    program.body.retain_mut(|statement| {
        let Statement::ImportDeclaration(import) = statement else {
            return true;
        };
        let Some(specifiers) = &mut import.specifiers else {
            // `import "x"`: for its effects.
            return true;
        };
        if specifiers.is_empty() {
            return true;
        }
        specifiers.retain(|s| match s {
            S::ImportSpecifier(s) => used(&s.local),
            S::ImportDefaultSpecifier(s) => used(&s.local),
            S::ImportNamespaceSpecifier(s) => used(&s.local),
        });
        !specifiers.is_empty()
    });
}

/// Removes ANSI escape sequences (`ESC [ ... letter`).
pub(crate) fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn closest_names_are_only_suggested_when_near() {
        let names = ["computed", "untracked"];
        assert_eq!(super::closest_name("computd", &names), Some("computed"));
        assert_eq!(super::closest_name("createSignal", &names), None);
    }
}
