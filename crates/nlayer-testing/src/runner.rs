//! `nlayer test`: each `*.test.tsx` (and `.test.ts` / `.test.js` /
//! `.test.jsx`) on a VM of its own, importing `@nlayer/test`. `open` starts
//! the app on a headless engine. The app host does not serve that module.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use nlayer_bundler::{BundleOptions, bundle};
use nlayer_core::{
    ROOT_ID, ViewKind,
    geometry::{Bounds, Edges, Points, Size, px},
};
use nlayer_engine::EngineError;
use nlayer_js::Host;
use serde::Deserialize;

use crate::{Headless, Screen, TreeNode};

const VIEWPORT_WIDTH: f32 = 390.0;
const VIEWPORT_HEIGHT: f32 = 844.0;

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

static ENTRIES: AtomicU64 = AtomicU64::new(0);

/// How `run` failed before any test ran (nothing to run, a path that is
/// not a test file). A test that fails is a [`Report`], not this.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct RunError {
    message: String,
}

impl RunError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// One test's outcome. `error` is `None` when it passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// The name `test` was given.
    pub name: String,
    /// The failure, with its stack. `None`: it passed.
    pub error: Option<String>,
}

/// One test file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileResult {
    /// Path relative to the directory that was searched, or the file name.
    pub name: String,
    /// The file did not run (it did not bundle, or the VM rejected it).
    pub error: Option<String>,
    /// A filter excluded every test. An empty file without a filter is not
    /// this: that file failed.
    pub filtered: bool,
    /// The tests that ran.
    pub cases: Vec<CaseResult>,
}

impl FileResult {
    fn passed(&self) -> usize {
        self.cases
            .iter()
            .filter(|case| case.error.is_none())
            .count()
    }

    fn failures(&self) -> usize {
        if self.error.is_some() || (self.cases.is_empty() && !self.filtered) {
            return 1;
        }
        self.cases
            .iter()
            .filter(|case| case.error.is_some())
            .count()
    }
}

/// What `nlayer test` ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// One entry per file, in path order.
    pub files: Vec<FileResult>,
}

impl Report {
    /// Tests that passed.
    pub fn passed(&self) -> usize {
        self.files.iter().map(FileResult::passed).sum()
    }

    /// Tests that failed, plus files that did not run.
    pub fn failures(&self) -> usize {
        self.files.iter().map(FileResult::failures).sum()
    }

    /// No failure.
    pub fn ok(&self) -> bool {
        self.failures() == 0
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for file in &self.files {
            if let Some(error) = &file.error {
                writeln!(f, "fail {}", file.name)?;
                write_block(f, error)?;
                continue;
            }
            if file.cases.is_empty() {
                if !file.filtered {
                    writeln!(f, "fail {}  no tests", file.name)?;
                }
                continue;
            }
            for case in &file.cases {
                match &case.error {
                    Some(error) => {
                        writeln!(f, "fail {}  {}", file.name, case.name)?;
                        write_block(f, error)?;
                    }
                    None => writeln!(f, "ok   {}  {}", file.name, case.name)?,
                }
            }
        }
        if self.passed() == 0 && self.failures() == 0 {
            writeln!(f, "no tests matched")?;
        }
        write!(f, "{} passed, {} failed", self.passed(), self.failures())
    }
}

fn write_block(f: &mut std::fmt::Formatter<'_>, text: &str) -> std::fmt::Result {
    for line in text.lines() {
        writeln!(f, "  {line}")?;
    }
    Ok(())
}

/// Runs the test file `path`, or every test file under the directory
/// `path` (not under `node_modules`, `target` or a hidden directory).
/// `filter` runs only tests whose name contains it.
pub fn run(path: &Path, filter: Option<&str>) -> Result<Report, RunError> {
    let mut files = Vec::new();
    if path.is_file() {
        if !is_test_file(path) {
            return Err(RunError::new(format!(
                "{} is not a test file (*.test.ts, *.test.tsx, *.test.js, *.test.jsx)",
                path.display()
            )));
        }
        files.push(path.to_path_buf());
    } else if path.is_dir() {
        walk(path, &mut files).map_err(|e| RunError::new(format!("{}: {e}", path.display())))?;
        files.sort();
        if files.is_empty() {
            return Err(RunError::new(format!(
                "no test files in {}",
                path.display()
            )));
        }
    } else {
        return Err(RunError::new(format!("not found: {}", path.display())));
    }
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };
    let filter = filter.filter(|text| !text.is_empty());
    let files = files
        .iter()
        .map(|file| run_file(file, &root, filter))
        .collect();
    Ok(Report { files })
}

fn is_test_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    name.ends_with(".test.ts")
        || name.ends_with(".test.tsx")
        || name.ends_with(".test.js")
        || name.ends_with(".test.jsx")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        if path.is_dir() {
            walk(&path, out)?;
        } else if is_test_file(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn run_file(path: &Path, root: &Path, filter: Option<&str>) -> FileResult {
    let name = relative(root, path);
    match bundle_test(path) {
        Ok(code) => execute(path, &name, &code, filter),
        Err(error) => FileResult {
            name,
            error: Some(error),
            filtered: false,
            cases: Vec::new(),
        },
    }
}

/// The entry that imports the test file and then runs what it registered.
fn bundle_test(test_file: &Path) -> Result<String, String> {
    let test_file = fs::canonicalize(test_file).map_err(|e| e.to_string())?;
    let id = ENTRIES.fetch_add(1, Ordering::Relaxed);
    let entry = std::env::temp_dir().join(format!("nlayer-test-{}-{id}.js", std::process::id()));
    let imported =
        serde_json::to_string(&test_file.to_string_lossy().as_ref()).map_err(|e| e.to_string())?;
    let source = format!(
        "import * as suite from {imported};\nimport {{ finish }} from \"@nlayer/test\";\nfinish(suite);\n"
    );
    fs::write(&entry, &source).map_err(|e| e.to_string())?;
    let bundled = bundle(&BundleOptions {
        entry: entry.clone(),
        minify: false,
        test: true,
    });
    let _ = fs::remove_file(&entry);
    bundled.map(|bundled| bundled.code).map_err(|e| e.plain)
}

fn execute(path: &Path, name: &str, code: &str, filter: Option<&str>) -> FileResult {
    let dir = path.parent().unwrap_or(path).to_path_buf();
    SESSION.with(|slot| {
        *slot.borrow_mut() = Some(Session {
            dir,
            apps: Vec::new(),
        });
    });
    let _session = SessionGuard;
    let result = (|| {
        let host = Host::new().map_err(|e| e.to_string())?;
        let _bridge = BridgeGuard(&host);
        host.set_test_bridge(Box::new(dispatch));
        if let Some(filter) = filter {
            host.set_global_string("__nlayerFilter", filter);
        }
        host.eval_source(code).map_err(|e| e.to_string())?;
        let raw = host
            .global_string("__nlayerResult")
            .ok_or_else(|| "the test file produced no result".to_owned())?;
        parse_cases(&raw)
    })();
    match result {
        Ok(cases) => FileResult {
            name: name.to_owned(),
            error: None,
            filtered: cases.is_empty() && filter.is_some(),
            cases,
        },
        Err(error) => FileResult {
            name: name.to_owned(),
            error: Some(error),
            filtered: false,
            cases: Vec::new(),
        },
    }
}

struct SessionGuard;

impl Drop for SessionGuard {
    fn drop(&mut self) {
        SESSION.with(|slot| drop(slot.borrow_mut().take()));
    }
}

struct BridgeGuard<'a>(&'a Host);

impl Drop for BridgeGuard<'_> {
    fn drop(&mut self) {
        self.0.clear_test_bridge();
    }
}

fn dispatch(op: &str, payload: &str) -> Result<String, String> {
    SESSION.with(|slot| {
        let mut session = slot.borrow_mut();
        let session = session.as_mut().ok_or("no test is running")?;
        session.call(op, payload)
    })
}

#[derive(Deserialize)]
struct RawCase {
    name: String,
    ok: bool,
    error: Option<String>,
}

fn parse_cases(raw: &str) -> Result<Vec<CaseResult>, String> {
    let cases: Vec<RawCase> = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    Ok(cases
        .into_iter()
        .map(|case| CaseResult {
            name: case.name,
            error: if case.ok { None } else { case.error },
        })
        .collect())
}

struct Session {
    dir: PathBuf,
    apps: Vec<Option<Headless>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.end();
    }
}

#[derive(Deserialize)]
struct OpenRequest {
    entry: String,
    options: Option<OpenOptions>,
}

#[derive(Deserialize, Default)]
struct OpenOptions {
    width: Option<f32>,
    height: Option<f32>,
    #[serde(rename = "safeArea")]
    safe_area: Option<SafeArea>,
    keyboard: Option<f32>,
}

#[derive(Deserialize)]
struct SafeArea {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

#[derive(Deserialize)]
struct PressRequest {
    id: usize,
    text: String,
}

#[derive(Deserialize)]
struct TypeRequest {
    id: usize,
    value: String,
}

impl Session {
    fn call(&mut self, op: &str, payload: &str) -> Result<String, String> {
        match op {
            "open" => self.open(payload),
            "texts" => {
                let app = self.app_mut(parse_id(payload)?)?;
                json(&app.texts())
            }
            "inputs" => {
                let app = self.app_mut(parse_id(payload)?)?;
                json(&app.screen().input_values())
            }
            "press" => {
                let request: PressRequest =
                    serde_json::from_str(payload).map_err(|e| e.to_string())?;
                let app = self.app_mut(request.id)?;
                if !app.press_text(&request.text) {
                    return Err(format!("no view shows \"{}\"", request.text));
                }
                let _ = app.next_frame(Duration::from_secs(2));
                Ok("null".to_owned())
            }
            "type" => {
                let request: TypeRequest =
                    serde_json::from_str(payload).map_err(|e| e.to_string())?;
                let app = self.app_mut(request.id)?;
                let mut ids: Vec<_> = app
                    .screen()
                    .views()
                    .filter(|(_, view)| view.kind == ViewKind::Input)
                    .map(|(id, _)| id)
                    .collect();
                ids.sort_unstable();
                let Some(input) = ids.first().copied() else {
                    return Err("no input".to_owned());
                };
                app.type_text(input, request.value);
                let _ = app.next_frame(Duration::from_secs(2));
                Ok("null".to_owned())
            }
            "tree" => {
                let app = self.app_mut(parse_id(payload)?)?;
                tree_json(app)
            }
            "close" => {
                let id = parse_id(payload)?;
                if let Some(app) = self.apps.get_mut(id).and_then(Option::take) {
                    app.stop();
                }
                Ok("null".to_owned())
            }
            "end" => {
                self.end();
                Ok("null".to_owned())
            }
            other => Err(format!("unknown test op `{other}`")),
        }
    }

    fn open(&mut self, payload: &str) -> Result<String, String> {
        let request: OpenRequest = serde_json::from_str(payload).map_err(|e| e.to_string())?;
        let entry = resolve_entry(&self.dir, &request.entry)?;
        let bundled = bundle(&BundleOptions {
            entry,
            minify: false,
            test: false,
        })
        .map_err(|e| e.plain)?;
        let options = request.options.unwrap_or_default();
        let mut app = Headless::start(
            bundled.code,
            Size::new(
                px(options.width.unwrap_or(VIEWPORT_WIDTH)),
                px(options.height.unwrap_or(VIEWPORT_HEIGHT)),
            ),
        )
        .map_err(engine_error)?;
        let mut nudges = 0;
        if let Some(area) = options.safe_area {
            app.control().set_safe_area(Edges {
                top: px(area.top),
                right: px(area.right),
                bottom: px(area.bottom),
                left: px(area.left),
            });
            nudges += 1;
        }
        if let Some(height) = options.keyboard {
            app.control().set_keyboard_inset(px(height));
            nudges += 1;
        }
        settle(&mut app, nudges);
        let id = self.apps.len();
        self.apps.push(Some(app));
        Ok(id.to_string())
    }

    fn app_mut(&mut self, id: usize) -> Result<&mut Headless, String> {
        self.apps
            .get_mut(id)
            .and_then(Option::as_mut)
            .ok_or_else(|| "the app is closed".to_owned())
    }

    fn end(&mut self) {
        for slot in &mut self.apps {
            if let Some(app) = slot.take() {
                app.stop();
            }
        }
    }
}

fn engine_error(error: EngineError) -> String {
    error.to_string()
}

fn parse_id(payload: &str) -> Result<usize, String> {
    serde_json::from_str(payload).map_err(|e| e.to_string())
}

fn json(value: &impl serde::Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

fn resolve_entry(dir: &Path, entry: &str) -> Result<PathBuf, String> {
    let path = Path::new(entry);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    };
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("entry not found: {}", path.display()))
    }
}

/// Waits for the frames the insets asked for. One wait can already have
/// drained the next frame, so a miss stops instead of sitting out the
/// whole timeout.
fn settle(app: &mut Headless, nudges: u32) {
    let mut left = nudges;
    let deadline = Instant::now() + Duration::from_secs(2);
    while left > 0 && Instant::now() < deadline {
        let slice =
            Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now()));
        if slice.is_zero() || !app.next_frame(slice) {
            break;
        }
        left -= 1;
    }
}

fn tree_json(app: &mut Headless) -> Result<String, String> {
    let screen = app.screen();
    let root = screen
        .tree(ROOT_ID)
        .ok_or_else(|| "the screen has no root".to_owned())?;
    json(&node_json(&root, screen))
}

fn node_json(node: &TreeNode, screen: &Screen) -> serde_json::Value {
    let view = screen.view(node.id);
    let mut map = serde_json::Map::new();
    map.insert("kind".into(), kind_name(node.kind).into());
    map.insert("frame".into(), frame_json(node.frame));
    if let Some(text) = &node.text {
        map.insert("text".into(), text.clone().into());
    }
    if let Some(view) = view {
        if let Some(input) = &view.input {
            map.insert("value".into(), input.value.clone().into());
        }
        if let Some(source) = &view.source {
            map.insert("src".into(), source.uri.as_ref().into());
        }
    }
    map.insert(
        "children".into(),
        serde_json::Value::Array(
            node.children
                .iter()
                .map(|child| node_json(child, screen))
                .collect(),
        ),
    );
    serde_json::Value::Object(map)
}

fn kind_name(kind: ViewKind) -> &'static str {
    match kind {
        ViewKind::View => "view",
        ViewKind::Text => "text",
        ViewKind::Image => "image",
        ViewKind::Scroll => "scroll",
        ViewKind::Input => "input",
    }
}

fn frame_json(frame: Bounds<Points>) -> serde_json::Value {
    serde_json::json!({
        "x": frame.origin.x.0,
        "y": frame.origin.y.0,
        "width": frame.size.width.0,
        "height": frame.size.height.0,
    })
}
