//! `nlayer init`: a TSX app, its test, and project configuration.
//! Existing files are kept. Editor types are npm packages; init does not install them.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Where to create the app.
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// The app directory (created if needed).
    pub dir: PathBuf,
}

/// What [`init`] wrote and what it left in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    /// Paths written, relative to the app directory.
    pub wrote: Vec<String>,
    /// Paths that already existed and were not changed.
    pub kept: Vec<String>,
}

impl std::fmt::Display for InitReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for path in &self.wrote {
            writeln!(f, "wrote {path}")?;
        }
        for path in &self.kept {
            writeln!(f, "kept {path}")?;
        }
        if self.wrote.iter().any(|path| path == "package.json") {
            writeln!(f, "install editor types with: bun install")?;
        }
        Ok(())
    }
}

/// A failed init.
#[derive(Debug)]
pub struct InitError {
    message: String,
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InitError {}

impl From<std::io::Error> for InitError {
    fn from(error: std::io::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

const APP: &str = r#"import { mount, signal } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
  const count = signal(0);
  return (
    <View style={{ padding: 16 }} onPress={() => count.set(count() + 1)}>
      <Text>Count: {count()}</Text>
    </View>
  );
}

mount(App);
"#;

const APP_TEST: &str = r#"import { expect, open, test } from "@nlayer/test";

test("a press increases the count", () => {
  const app = open("./App.tsx");
  expect(app.texts()).toEqual(["Count: 0"]);
  app.press("Count: 0");
  expect(app.texts()).toEqual(["Count: 1"]);
});
"#;

const GITIGNORE: &str = ".nlayer/\nnode_modules/\n";

const NLAYER_JSON: &str = r#"{
  "name": "Example",
  "id": "com.example.app",
  "version": "1.0.0",
  "build": 1,
  "icon": null
}
"#;

/// `jsxImportSource` is `@nlayer` (types only). `types: ["globals"]` loads
/// `@nlayer/globals` from `typeRoots`. Init does not write `.nlayer/types`.
const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "jsxImportSource": "@nlayer",
    "strict": true,
    "noEmit": true,
    "lib": ["ES2022"],
    "typeRoots": ["./node_modules/@nlayer", "./node_modules/@types"],
    "types": ["globals"]
  },
  "include": ["**/*.ts", "**/*.tsx"]
}
"#;

/// Creates an app in `options.dir`: `App.tsx`, `App.test.tsx`,
/// `tsconfig.json`, `package.json`, `nlayer.json` and `.gitignore`. A file
/// that already exists is kept.
pub fn init(options: &InitOptions) -> Result<InitReport, InitError> {
    if options.dir.is_file() {
        return Err(InitError {
            message: format!("{} is a file", options.dir.display()),
        });
    }
    fs::create_dir_all(&options.dir)?;
    let mut report = InitReport {
        wrote: Vec::new(),
        kept: Vec::new(),
    };
    write_new(&options.dir, "App.tsx", APP, &mut report)?;
    write_new(&options.dir, "App.test.tsx", APP_TEST, &mut report)?;
    write_new(&options.dir, "tsconfig.json", TSCONFIG, &mut report)?;
    write_new(&options.dir, ".gitignore", GITIGNORE, &mut report)?;
    write_new(&options.dir, "nlayer.json", NLAYER_JSON, &mut report)?;
    write_new(
        &options.dir,
        "package.json",
        &package_json(&package_name(&options.dir)),
        &mut report,
    )?;
    Ok(report)
}

fn write_new(dir: &Path, rel: &str, text: &str, report: &mut InitReport) -> Result<(), InitError> {
    let path = dir.join(rel);
    if path.exists() {
        report.kept.push(rel.to_owned());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    report.wrote.push(rel.to_owned());
    Ok(())
}

fn package_json(name: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        r#"{{
  "name": "{name}",
  "private": true,
  "dependencies": {{
    "@nlayer/globals": "{version}",
    "@nlayer/jsx-runtime": "{version}",
    "@nlayer/runtime": "{version}",
    "@nlayer/ui": "{version}"
  }},
  "devDependencies": {{
    "@nlayer/test": "{version}"
  }}
}}
"#
    )
}

/// npm name from the directory: lowercase, with characters outside
/// `a-z`, `0-9`, `.`, `_` and `-` replaced. A missing or empty result is `app`.
fn package_name(dir: &Path) -> String {
    let Some(name) = dir.file_name() else {
        return "app".to_owned();
    };
    let raw = name.to_string_lossy();
    if raw == "." || raw == ".." {
        return "app".to_owned();
    }
    let mut out = String::new();
    for c in raw.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_start_matches(['.', '-']);
    if trimmed.is_empty() {
        "app".to_owned()
    } else {
        trimmed.to_owned()
    }
}
