//! Installed `@nlayer/*` versions versus this CLI.

use std::{fs, path::Path};

const PACKAGES: [&str; 5] = ["runtime", "ui", "jsx-runtime", "test", "globals"];

/// `None` when no installed `@nlayer/*` package.json was found, or every found version equals this CLI.
/// Does not fail the command. Unreadable package.json entries are skipped.
pub fn type_package_warning(start: &Path) -> Option<String> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    let mut installed = None;
    for _ in 0..4 {
        let candidate = dir.join("node_modules/@nlayer");
        if candidate.is_dir() {
            installed = Some(candidate);
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    let installed = installed?;
    let mut mismatches = Vec::new();
    for name in PACKAGES {
        let path = installed.join(name).join("package.json");
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(version) = value.get("version").and_then(|v| v.as_str()) else {
            continue;
        };
        if version != env!("CARGO_PKG_VERSION") {
            mismatches.push(format!("@nlayer/{name} {version}"));
        }
    }
    if mismatches.is_empty() {
        None
    } else {
        Some(format!(
            "nlayer {} and installed types differ ({}); editor types may not match this binary",
            env!("CARGO_PKG_VERSION"),
            mismatches.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_when_an_installed_version_differs() {
        let dir = std::env::temp_dir().join(format!("nlayer-types-warn-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let pkg = dir.join("node_modules/@nlayer/runtime");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("package.json"), r#"{"version":"9.9.9"}"#).unwrap();
        let warning = type_package_warning(&dir).expect("a mismatch warns");
        assert!(warning.contains("nlayer "), "{warning}");
        assert!(warning.contains("@nlayer/runtime 9.9.9"), "{warning}");
        assert!(warning.contains("editor types may not match"), "{warning}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn silent_without_node_modules() {
        let dir = std::env::temp_dir().join(format!("nlayer-types-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(type_package_warning(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn silent_when_the_installed_version_matches() {
        let dir = std::env::temp_dir().join(format!("nlayer-types-match-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let pkg = dir.join("node_modules/@nlayer/ui");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(
            pkg.join("package.json"),
            format!(r#"{{"version":"{}"}}"#, env!("CARGO_PKG_VERSION")),
        )
        .unwrap();
        assert_eq!(type_package_warning(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
