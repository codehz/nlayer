//! The browser host files `nlayer run --target web` serves and
//! `nlayer package --target web` writes.
//!
//! One table, so a page module added to the platform crate cannot be
//! served by one path and missing from the other. Tests walk the source
//! directory to keep the table complete.

const JS: &str = "text/javascript; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";

/// `(path on the page, content type, body)`.
pub(crate) const FILES: &[(&str, &str, &str)] = &[
    (
        "boot.js",
        JS,
        include_str!("../../nlayer-platform-web/web/boot.js"),
    ),
    (
        "nlayer.js",
        JS,
        include_str!("../../nlayer-platform-web/web/nlayer.js"),
    ),
    (
        "host.js",
        JS,
        include_str!("../../nlayer-platform-web/web/host.js"),
    ),
    (
        "base.css",
        CSS,
        include_str!("../../nlayer-platform-web/web/base.css"),
    ),
    (
        "browser/runtime.js",
        JS,
        include_str!("../../nlayer-platform-web/web/browser/runtime.js"),
    ),
    (
        "browser/list.js",
        JS,
        include_str!("../../nlayer-platform-web/web/browser/list.js"),
    ),
    (
        "browser/values.js",
        JS,
        include_str!("../../nlayer-platform-web/web/browser/values.js"),
    ),
    (
        "browser/storage.js",
        JS,
        include_str!("../../nlayer-platform-web/web/browser/storage.js"),
    ),
    (
        "browser/animation.js",
        JS,
        include_str!("../../nlayer-platform-web/web/browser/animation.js"),
    ),
    (
        "browser/navigation.js",
        JS,
        include_str!("../../nlayer-platform-web/web/browser/navigation.js"),
    ),
];

pub(crate) fn find(path: &str) -> Option<(&'static str, &'static str)> {
    FILES
        .iter()
        .find(|(name, _, _)| *name == path)
        .map(|(_, mime, body)| (*mime, *body))
}

/// A path the page owns: an app asset may not take it, or the host file
/// would win in `run` and the asset would be lost.
pub(crate) fn reserved(path: &str) -> bool {
    matches!(
        path,
        "index.html" | "nlayer.wasm" | "bundle.js" | "bundle.js.map" | "__nlayer/generation"
    ) || path.starts_with("__nlayer/")
        || find(path).is_some()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, path::Path};

    use super::{FILES, find, reserved};

    /// Every production file in the platform crate's `web` directory: the
    /// page, modules and the base sheet. Tests and `index.html` are not
    /// served as files.
    fn production_files() -> BTreeSet<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../nlayer-platform-web/web");
        let mut files = BTreeSet::new();
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir).expect("the web directory") {
                let path = entry.expect("a web entry").path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if name.ends_with(".test.mjs") {
                    continue;
                }
                let relative = path
                    .strip_prefix(&root)
                    .expect("inside web")
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(relative);
            }
        }
        files
    }

    #[test]
    fn every_web_file_is_embedded() {
        let embedded: BTreeSet<&str> = FILES.iter().map(|(path, _, _)| *path).collect();
        for path in production_files() {
            if path == "index.html" {
                continue;
            }
            assert!(
                embedded.contains(path.as_str()),
                "`{path}` is in the platform web directory but not in `web_assets::FILES`"
            );
        }
        assert_eq!(embedded.len(), FILES.len(), "a duplicated asset path");
    }

    #[test]
    fn the_page_imports_embedded_modules() {
        let page = include_str!("../../nlayer-platform-web/web/index.html");
        assert!(page.contains("from \"./boot.js\""), "{page}");
        assert!(find("boot.js").is_some());
    }

    #[test]
    fn host_files_are_reserved() {
        for (path, _, _) in FILES {
            assert!(reserved(path), "`{path}` is not reserved");
        }
        assert!(reserved("index.html"));
        assert!(reserved("bundle.js"));
        assert!(reserved("__nlayer/generation"));
        assert!(!reserved("img/icon.png"));
    }
}
