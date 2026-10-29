//! One module to the JS the runtime runs: TypeScript removed and the JSX
//! compiled by nlayer's compiler ([`crate::jsx`]) in one oxc pass, with a
//! source map back to that module.

use std::path::Path;

use oxc::{
    allocator::Allocator,
    ast::ast::{
        Argument, CallExpression, ExportAllDeclaration, ExportFromDeclaration, Expression,
        ImportDeclaration, ImportExpression, NewExpression, Program,
    },
    ast_visit::{Visit, walk},
    codegen::{Codegen, CodegenOptions},
    diagnostics::OxcDiagnostic,
    parser::Parser,
    semantic::SemanticBuilder,
    span::{SourceType, Span},
    transformer::{JsxOptions, TransformOptions, Transformer},
};
use oxc_sourcemap::SourceMap;

/// What the compiler knows of the modules it checks named imports against:
/// `@nlayer/runtime`, `@nlayer/ui` and the renderer's [`RT`]. With `test`,
/// `@nlayer/test` too (the bundler inlines that module; the app host does
/// not serve it). Handwritten [`RT`] is refused before the compiler inserts
/// its own import.
#[derive(Debug, Default)]
pub(crate) struct NativeModules {
    specifiers: std::collections::BTreeSet<String>,
    /// The names each module exports, to check named imports against.
    exports: std::collections::BTreeMap<String, Vec<&'static str>>,
}

impl NativeModules {
    pub(crate) fn new(test: bool) -> Self {
        let mut out = Self::default();
        for spec in ["@nlayer/runtime", "@nlayer/ui", RT] {
            out.specifiers.insert(spec.to_owned());
            if let Some(names) = nlayer_js::native_module_exports(spec) {
                out.exports.insert(spec.to_owned(), names);
            }
        }
        if test {
            out.exports.insert(
                crate::test_api::SPECIFIER.to_owned(),
                crate::test_api::EXPORTS.to_vec(),
            );
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
    // Before the compiler inserts its own `nlayer:rt` import. After that,
    // rolldown cannot tell a handwritten specifier from the generated one.
    if let Some(err) = handwritten_rt(&program) {
        return Err(err.render_with_source_code(source.to_owned()));
    }
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
        elide_unused_imports(&mut program, &scoping, modules);
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

/// The renderer's module. Compiled JSX imports it; app source does not write it.
pub(crate) const RT: &str = "nlayer:rt";

/// The first handwritten [`RT`] specifier: a static import or re-export,
/// `import()`, `require()`, or `new URL()`.
fn handwritten_rt(program: &Program<'_>) -> Option<OxcDiagnostic> {
    let mut finder = HandwrittenRt { hit: None };
    finder.visit_program(program);
    finder.hit
}

struct HandwrittenRt {
    hit: Option<OxcDiagnostic>,
}

impl HandwrittenRt {
    fn note(&mut self, source: &str, span: Span) {
        if self.hit.is_some() || source != RT {
            return;
        }
        self.hit = Some(
            OxcDiagnostic::error(
                "`nlayer:rt` is the renderer's module; compiled JSX imports it, apps do not write it",
            )
            .with_label(span),
        );
    }

    fn note_expr(&mut self, expr: &Expression<'_>) {
        if let Some((source, span)) = static_specifier(expr) {
            self.note(source, span);
        }
    }

    fn note_arg(&mut self, arg: &Argument<'_>) {
        let Some((source, span)) = static_argument(arg) else {
            return;
        };
        self.note(source, span);
    }
}

impl<'a> Visit<'a> for HandwrittenRt {
    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        self.note(it.source.value.as_str(), it.source.span);
    }

    fn visit_export_all_declaration(&mut self, it: &ExportAllDeclaration<'a>) {
        self.note(it.source.value.as_str(), it.source.span);
    }

    fn visit_export_from_declaration(&mut self, it: &ExportFromDeclaration<'a>) {
        self.note(it.source.value.as_str(), it.source.span);
    }

    fn visit_import_expression(&mut self, it: &ImportExpression<'a>) {
        self.note_expr(&it.source);
        walk::walk_import_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if ident_name(&it.callee) == Some("require")
            && let Some(arg) = it.arguments.first()
        {
            self.note_arg(arg);
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_new_expression(&mut self, it: &NewExpression<'a>) {
        if ident_name(&it.callee) == Some("URL")
            && let Some(arg) = it.arguments.first()
        {
            self.note_arg(arg);
        }
        walk::walk_new_expression(self, it);
    }
}

/// A specifier rolldown can resolve: a string, a static template, or either
/// wrapped in parentheses.
fn static_specifier<'a>(expr: &'a Expression<'a>) -> Option<(&'a str, Span)> {
    match expr {
        Expression::StringLiteral(lit) => Some((lit.value.as_str(), lit.span)),
        Expression::TemplateLiteral(lit) if lit.expressions.is_empty() => {
            let cooked = lit.quasis.first()?.value.cooked.as_ref()?;
            Some((cooked.as_str(), lit.span))
        }
        Expression::ParenthesizedExpression(lit) => static_specifier(&lit.expression),
        _ => None,
    }
}

fn static_argument<'a>(arg: &'a Argument<'a>) -> Option<(&'a str, Span)> {
    match arg {
        Argument::StringLiteral(lit) => Some((lit.value.as_str(), lit.span)),
        Argument::TemplateLiteral(lit) if lit.expressions.is_empty() => {
            let cooked = lit.quasis.first()?.value.cooked.as_ref()?;
            Some((cooked.as_str(), lit.span))
        }
        Argument::ParenthesizedExpression(lit) => static_specifier(&lit.expression),
        _ => None,
    }
}

fn ident_name<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::Identifier(id) => Some(id.name.as_str()),
        Expression::ParenthesizedExpression(e) => ident_name(&e.expression),
        _ => None,
    }
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
/// `scoping` is the program's. An unknown `@nlayer` import stays, so the
/// bundler resolves it and refuses it instead of dropping it first.
fn elide_unused_imports(
    program: &mut oxc::ast::ast::Program<'_>,
    scoping: &oxc::semantic::Scoping,
    modules: &NativeModules,
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
        let source = import.source.value.as_str();
        if !modules.contains(source) && (source == "@nlayer" || source.starts_with("@nlayer/")) {
            return true;
        }
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
