//! Build-time checks of `class` and `classList`.
//!
//! Adapted from `nsh-bundler`'s checker. Each static class string is parsed
//! with `nlayer_core::parse` and `Theme::default`. Unknown and unsupported
//! names are warned. The warning does not fail the build.

use nlayer_core::{Theme, parse};
use oxc::{
    allocator::Allocator,
    ast::ast::{
        Expression, JSXAttribute, JSXAttributeName, JSXAttributeValue, ObjectPropertyKind,
        PropertyKey,
    },
    ast_visit::{Visit, walk},
    parser::Parser,
    span::SourceType,
};

/// One static class the parser rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassWarning {
    /// 1-based line and column.
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) class: String,
    /// The effect gate refused this name (`unsupported classes`), rather
    /// than it being unknown.
    pub(crate) unsupported: bool,
}

/// Unknown and unsupported classes in `source`'s JSX. The build does not fail.
pub(crate) fn check(source: &str, source_type: SourceType) -> Vec<ClassWarning> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let theme = Theme::default();
    let mut checker = Checker {
        source,
        theme: &theme,
        found: Vec::new(),
    };
    checker.visit_program(&parsed.program);
    checker.found
}

struct Checker<'s> {
    source: &'s str,
    theme: &'s Theme,
    found: Vec<ClassWarning>,
}

impl Checker<'_> {
    /// Checks the classes of the string at byte `offset` of the source.
    fn classes(&mut self, classes: &str, offset: u32) {
        let list = parse(self.theme, classes);
        for name in list.unknown() {
            self.report(classes, offset, name, false);
        }
        for name in list.unsupported() {
            self.report(classes, offset, name, true);
        }
    }

    /// The name's own position within the string, if it is found.
    fn report(&mut self, classes: &str, offset: u32, name: &str, unsupported: bool) {
        let at = classes.find(name).unwrap_or(0) as u32;
        let (line, column) = position(self.source, offset + at);
        self.found.push(ClassWarning {
            line,
            column,
            class: name.to_owned(),
            unsupported,
        });
    }

    /// Strings an expression can evaluate to statically: literals, both
    /// sides of conditionals, the right of `&&` / `||`, the parts of templates.
    fn expression(&mut self, e: &Expression<'_>) {
        match e {
            Expression::StringLiteral(s) => self.classes(&s.value, s.span.start + 1),
            Expression::TemplateLiteral(t) => {
                let last = t.quasis.len().saturating_sub(1);
                for (i, q) in t.quasis.iter().enumerate() {
                    // A class touching an interpolation is only part of a
                    // name (`bg-${color}-500`): skip it.
                    let raw: &str = &q.value.raw;
                    let mut start = 0;
                    let mut end = raw.len();
                    if i > 0 && !raw.starts_with(char::is_whitespace) {
                        start = raw.find(char::is_whitespace).unwrap_or(raw.len());
                    }
                    if i < last && !raw.ends_with(char::is_whitespace) {
                        end = raw.rfind(char::is_whitespace).map_or(0, |e| e + 1);
                    }
                    if start < end {
                        self.classes(&raw[start..end], q.span.start + start as u32);
                    }
                }
                for e in &t.expressions {
                    self.expression(e);
                }
            }
            Expression::ConditionalExpression(c) => {
                self.expression(&c.consequent);
                self.expression(&c.alternate);
            }
            Expression::LogicalExpression(l) => self.expression(&l.right),
            Expression::ParenthesizedExpression(p) => self.expression(&p.expression),
            _ => {}
        }
    }
}

impl<'a> Visit<'a> for Checker<'_> {
    fn visit_jsx_attribute(&mut self, attr: &JSXAttribute<'a>) {
        let JSXAttributeName::Identifier(name) = &attr.name else {
            return walk::walk_jsx_attribute(self, attr);
        };
        match (name.name.as_str(), &attr.value) {
            ("class", Some(JSXAttributeValue::StringLiteral(s))) => {
                self.classes(&s.value, s.span.start + 1);
            }
            ("class", Some(JSXAttributeValue::ExpressionContainer(c))) => {
                if let Some(e) = c.expression.as_expression() {
                    self.expression(e);
                }
            }
            ("classList", Some(JSXAttributeValue::ExpressionContainer(c))) => {
                if let Some(Expression::ObjectExpression(o)) = c.expression.as_expression() {
                    for p in &o.properties {
                        if let ObjectPropertyKind::ObjectProperty(p) = p {
                            match &p.key {
                                PropertyKey::StringLiteral(s) => {
                                    self.classes(&s.value, s.span.start + 1);
                                }
                                PropertyKey::StaticIdentifier(i) => {
                                    self.classes(&i.name, i.span.start);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        walk::walk_jsx_attribute(self, attr);
    }
}

/// 1-based line and column (in characters) of byte `offset` of `source`.
fn position(source: &str, offset: u32) -> (usize, usize) {
    let before = &source[..(offset as usize).min(source.len())];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_unknown_static_classes() {
        let source = r#"
import { Text, View } from "@nlayer/ui";
export function A(p: { on: boolean }) {
  return (
    <View class="p-4 bg-blu-500 blur-sm antialiased">
      <Text class={p.on ? "text-white" : "txt-black"} classList={{ "opacity-50": p.on, nope: p.on }} />
      <View class={`flex ${p.on ? "gap-2" : ""} wat bg-${p.on}-500`} />
    </View>
  );
}
"#;
        let found = check(source, SourceType::tsx());
        let unknown: Vec<&str> = found
            .iter()
            .filter(|w| !w.unsupported)
            .map(|w| w.class.as_str())
            .collect();
        let unsupported: Vec<&str> = found
            .iter()
            .filter(|w| w.unsupported)
            .map(|w| w.class.as_str())
            .collect();
        assert_eq!(unknown, ["bg-blu-500", "txt-black", "nope", "wat"]);
        assert_eq!(unsupported, ["blur-sm"]);
        assert!(
            found
                .iter()
                .all(|w| { !w.class.contains('$') && w.class != "bg-" && w.class != "-500" }),
            "a dynamic bg-${{...}} fragment was reported: {found:?}"
        );
        assert_eq!(
            (found[0].line, found[0].column, found[0].class.as_str()),
            (5, 22, "bg-blu-500")
        );
        let shadow = found.iter().find(|w| w.class == "blur-sm").unwrap();
        assert_eq!((shadow.line, shadow.column), (5, 33));
    }
}
