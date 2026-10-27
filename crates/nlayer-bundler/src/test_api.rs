//! `@nlayer/test`, inlined into a test bundle. The app host does not serve
//! it: [`crate::plugin`] resolves the specifier only when bundling a test.

/// The specifier test files import.
pub(crate) const SPECIFIER: &str = "@nlayer/test";

/// The id the plugin loads the module under (a `\0` id is not a file).
pub(crate) const MODULE_ID: &str = "\0@nlayer/test";

/// Names a test file may import. `finish` is the generated entry's.
pub(crate) const EXPORTS: &[&str] = &["test", "expect", "open", "finish"];

/// The module source.
pub(crate) const SOURCE: &str = include_str!("test_api.js");
