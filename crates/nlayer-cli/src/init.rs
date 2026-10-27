//! `nlayer init`: a TSX app, its test, `tsconfig.json` and the declarations
//! the editor reads. Existing files are kept.

use std::{
    fs,
    path::{Path, PathBuf},
};

use nlayer_bundler::type_files;

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
  app.press("Count");
  expect(app.texts()).toEqual(["Count: 1"]);
});
"#;

const GITIGNORE: &str = ".nlayer/\n";

/// `jsxImportSource` is `@nlayer` (types only). The declarations init writes
/// live in `.nlayer/types`, which `.gitignore` excludes.
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
    "paths": {
      "@nlayer/runtime": [".nlayer/types/@nlayer/runtime.d.ts"],
      "@nlayer/ui": [".nlayer/types/@nlayer/ui.d.ts"],
      "@nlayer/test": [".nlayer/types/@nlayer/test.d.ts"],
      "@nlayer/jsx-runtime": [".nlayer/types/@nlayer/jsx-runtime.d.ts"]
    }
  },
  "include": ["**/*.ts", "**/*.tsx", ".nlayer/types/globals.d.ts"]
}
"#;

/// Creates an app in `options.dir`: `App.tsx`, `App.test.tsx`,
/// `tsconfig.json`, `.gitignore` and `.nlayer/types`. A file that already
/// exists is kept. Generated declarations are rewritten when they changed.
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
    for (path, text) in type_files() {
        let rel = format!(".nlayer/types/{path}");
        write_generated(&options.dir, &rel, &text, &mut report)?;
    }
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

/// Writes a generated file, or leaves it when the bytes already match.
fn write_generated(
    dir: &Path,
    rel: &str,
    text: &str,
    report: &mut InitReport,
) -> Result<(), InitError> {
    let path = dir.join(rel);
    if path.is_file() && fs::read_to_string(&path).ok().as_deref() == Some(text) {
        report.kept.push(rel.to_owned());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, text)?;
    report.wrote.push(rel.to_owned());
    Ok(())
}
