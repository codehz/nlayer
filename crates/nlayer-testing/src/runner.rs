//! `nlayer test`: each `*.test.tsx` (and `.test.ts` / `.test.js` /
//! `.test.jsx`) on a VM of its own, importing `@nlayer/test`. `open` starts
//! the app on a [`AppHost`]. A file that is not a test file is an app entry:
//! it is opened, then the steps from standard input run. The default host is
//! headless. The app bundle does not serve that module.
use std::{
    cell::RefCell,
    fs,
    io::{IsTerminal, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use nlayer_bundler::{BundleOptions, bundle};
use nlayer_core::{
    KeyboardType, ROOT_ID, ResizeMode, ViewKind,
    geometry::{Bounds, Edges, Points, Size, px},
    transform::TransformKind,
};
use nlayer_engine::EngineError;
use nlayer_js::Host;
use serde::Deserialize;

use crate::{Headless, Screen, TreeNode};

const VIEWPORT_WIDTH: f32 = 390.0;
const VIEWPORT_HEIGHT: f32 = 844.0;

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static HOST: RefCell<Option<*mut (dyn AppHost + 'static)>> = const { RefCell::new(None) };
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
    /// A failure before any test ran.
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn new(message: impl Into<String>) -> Self {
        Self::message(message)
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

/// Safe area pinned for one `open`, in points.
#[derive(Debug, Clone, Copy)]
pub struct HostInsets {
    /// Top inset.
    pub top: f32,
    /// Right inset.
    pub right: f32,
    /// Bottom inset.
    pub bottom: f32,
    /// Left inset.
    pub left: f32,
}

/// What `open` asks the host to run.
#[derive(Debug, Clone)]
pub struct HostOpen {
    /// The app entry.
    pub entry: PathBuf,
    /// Viewport width, in points.
    pub width: f32,
    /// Viewport height, in points.
    pub height: f32,
    /// Pinned safe area. `None` is zero.
    pub safe_area: Option<HostInsets>,
    /// Pinned keyboard inset. `None` is zero.
    pub keyboard: Option<f32>,
}

/// One app a test has opened. Dropping it closes the app.
pub trait HostedApp {
    /// Text of every text view, in mount order.
    fn texts(&mut self) -> Result<Vec<String>, String>;
    /// Value of every input, in mount order.
    fn inputs(&mut self) -> Result<Vec<String>, String>;
    /// Presses what `selector` names, through this host's input.
    fn press(&mut self, selector: &str) -> Result<(), String>;
    /// Types `text` into the input `selector` names.
    fn type_text(&mut self, selector: &str, text: &str) -> Result<(), String>;
    /// Scrolls toward the end of the content by `distance` points.
    fn scroll(&mut self, selector: &str, distance: f32) -> Result<(), String>;
    /// Waits until `selector` names one view that is on screen.
    fn wait_for(&mut self, selector: &str) -> Result<(), String>;
    /// The mounted tree: kind, text, input, frame.
    fn tree(&mut self) -> Result<serde_json::Value, String>;
    /// Closes the app.
    fn close(&mut self);
}

/// Where `open` starts the app. The test file still runs in QuickJS.
pub trait AppHost {
    /// Starts `request` and returns the app the later steps drive.
    fn open(&mut self, request: HostOpen) -> Result<Box<dyn HostedApp>, String>;

    /// What the opened app's `platform.os` is. Headless and web are `web`.
    fn os(&self) -> &'static str {
        "web"
    }
}

/// Runs the test file `path`, or every test file under the directory
/// `path` (not under `node_modules`, `target` or a hidden directory).
/// A file that is not a test file is an app entry: it is opened, then the
/// steps from standard input run (`app` is that app; `expect` and `open`
/// are in scope). A terminal supplies no steps. `filter` runs only tests
/// whose name contains it. The app opens on the headless engine.
pub fn run(path: &Path, filter: Option<&str>) -> Result<Report, RunError> {
    run_with(path, filter, &mut HeadlessHost)
}

/// [`run`], but `open` starts the app on `host`. The same steps and the
/// same report, including an app entry's standard-input steps. A step the
/// host cannot do fails; it does not fall back.
pub fn run_with(
    path: &Path,
    filter: Option<&str>,
    host: &mut dyn AppHost,
) -> Result<Report, RunError> {
    HOST.with(|cell| *cell.borrow_mut() = Some(host_ptr(host)));
    let _guard = HostGuard;
    run_in(path, filter)
}

struct HostGuard;

impl Drop for HostGuard {
    fn drop(&mut self) {
        HOST.with(|cell| *cell.borrow_mut() = None);
    }
}

fn run_in(path: &Path, filter: Option<&str>) -> Result<Report, RunError> {
    run_path(path, filter, None)
}

/// [`run_in`]. `steps` replaces standard input when `path` is an app entry.
fn run_path(path: &Path, filter: Option<&str>, steps: Option<&str>) -> Result<Report, RunError> {
    if path.is_file() && !is_test_file(path) {
        let owned;
        let steps = match steps {
            Some(steps) => steps,
            None => {
                owned = read_steps()?;
                owned.as_str()
            }
        };
        return run_entry(path, steps, filter);
    }
    let mut files = Vec::new();
    if path.is_file() {
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

/// Steps from standard input. A terminal has none: the app is only opened.
fn read_steps() -> Result<String, RunError> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Ok(String::new());
    }
    let mut steps = String::new();
    stdin
        .lock()
        .read_to_string(&mut steps)
        .map_err(|error| RunError::new(format!("read the steps from standard input: {error}")))?;
    Ok(steps)
}

/// Opens `entry` and runs `steps` (`app` is the opened app). The report
/// names the entry, not the generated file.
fn run_entry(entry: &Path, steps: &str, filter: Option<&str>) -> Result<Report, RunError> {
    let file = StepsFile::create(entry, steps)?;
    let name = entry
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| entry.display().to_string());
    let result = match bundle_test(&file.0) {
        Ok(code) => execute_in(entry.parent().unwrap_or(entry), &name, &code, filter),
        Err(error) => FileResult {
            name,
            error: Some(error),
            filtered: false,
            cases: Vec::new(),
        },
    };
    Ok(Report {
        files: vec![result],
    })
}

fn steps_source(entry: &Path, steps: &str) -> Result<String, String> {
    let shown = entry
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "app".to_owned());
    // `open` joins a relative path onto the session directory, which is the
    // entry's parent. A relative entry would be joined twice
    // (`examples/gallery` + `examples/gallery/App.tsx`). The canonical path
    // is absolute, so `open` uses it as given.
    let entry_js =
        serde_json::to_string(&entry_literal(entry)?).map_err(|error| error.to_string())?;
    Ok(format!(
        "// Made by `nlayer test {shown}`: the steps of standard input.\n\
         // `app` is the opened app. `entry` is its path. `expect` and `open` are in scope.\n\
         import {{ expect, open, test }} from \"@nlayer/test\";\n\
         \n\
         test(\"steps\", () => {{\n\
         const entry = {entry_js};\n\
         const app = open(entry);\n\
         {steps}\n\
         }});\n"
    ))
}

/// Absolute path embedded for `open`. Windows `canonicalize` adds `\\?\`;
/// after the slash rewrite that prefix is not a path `open` can use.
fn entry_literal(entry: &Path) -> Result<String, String> {
    let absolute = fs::canonicalize(entry)
        .map_err(|error| format!("entry not found: {}: {error}", entry.display()))?;
    let text = absolute.to_string_lossy().replace('\\', "/");
    let text = text
        .strip_prefix("//?/UNC/")
        .map(|rest| format!("//{rest}"))
        .unwrap_or_else(|| text.strip_prefix("//?/").unwrap_or(&text).to_owned());
    Ok(text)
}

/// The generated steps file. Dropping it deletes the file.
struct StepsFile(PathBuf);

impl StepsFile {
    fn create(entry: &Path, steps: &str) -> Result<Self, RunError> {
        let id = ENTRIES.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("nlayer-steps-{}-{id}.tsx", std::process::id()));
        let source = steps_source(entry, steps).map_err(RunError::new)?;
        fs::write(&path, source)
            .map_err(|error| RunError::new(format!("write the steps: {error}")))?;
        Ok(Self(path))
    }
}

impl Drop for StepsFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
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
    execute_in(path.parent().unwrap_or(path), name, code, filter)
}

fn execute_in(dir: &Path, name: &str, code: &str, filter: Option<&str>) -> FileResult {
    let dir = dir.to_path_buf();
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
        if let Ok(os) = with_host(|app_host| app_host.os()) {
            host.set_global_string("__nlayerOs", os);
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
    apps: Vec<Option<Box<dyn HostedApp>>>,
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
    selector: String,
}

#[derive(Deserialize)]
struct TypeRequest {
    id: usize,
    selector: String,
    text: String,
}

#[derive(Deserialize)]
struct ScrollRequest {
    id: usize,
    selector: String,
    distance: f64,
}

#[derive(Deserialize)]
struct WaitRequest {
    id: usize,
    selector: String,
}

impl Session {
    fn call(&mut self, op: &str, payload: &str) -> Result<String, String> {
        match op {
            "open" => self.open(payload),
            "texts" => {
                let app = self.app_mut(parse_id(payload)?)?;
                json(&app.texts()?)
            }
            "inputs" => {
                let app = self.app_mut(parse_id(payload)?)?;
                json(&app.inputs()?)
            }
            "press" => {
                let request: PressRequest =
                    serde_json::from_str(payload).map_err(|e| e.to_string())?;
                self.app_mut(request.id)?.press(&request.selector)?;
                Ok("null".to_owned())
            }
            "type" => {
                let request: TypeRequest =
                    serde_json::from_str(payload).map_err(|e| e.to_string())?;
                self.app_mut(request.id)?
                    .type_text(&request.selector, &request.text)?;
                Ok("null".to_owned())
            }
            "scroll" => {
                let request: ScrollRequest =
                    serde_json::from_str(payload).map_err(|e| e.to_string())?;
                self.app_mut(request.id)?
                    .scroll(&request.selector, request.distance as f32)?;
                Ok("null".to_owned())
            }
            "waitFor" => {
                let request: WaitRequest =
                    serde_json::from_str(payload).map_err(|e| e.to_string())?;
                self.app_mut(request.id)?.wait_for(&request.selector)?;
                Ok("null".to_owned())
            }
            "tree" => {
                let app = self.app_mut(parse_id(payload)?)?;
                json(&app.tree()?)
            }
            "close" => {
                let id = parse_id(payload)?;
                if let Some(mut app) = self.apps.get_mut(id).and_then(Option::take) {
                    app.close();
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
        let options = request.options.unwrap_or_default();
        let opened = HostOpen {
            entry,
            width: options.width.unwrap_or(VIEWPORT_WIDTH),
            height: options.height.unwrap_or(VIEWPORT_HEIGHT),
            safe_area: options.safe_area.map(|area| HostInsets {
                top: area.top,
                right: area.right,
                bottom: area.bottom,
                left: area.left,
            }),
            keyboard: options.keyboard,
        };
        let app = with_host(|host| host.open(opened))??;
        let id = self.apps.len();
        self.apps.push(Some(app));
        Ok(id.to_string())
    }

    fn app_mut(&mut self, id: usize) -> Result<&mut dyn HostedApp, String> {
        match self.apps.get_mut(id) {
            Some(Some(app)) => Ok(app.as_mut()),
            _ => Err("the app is closed".to_owned()),
        }
    }

    fn end(&mut self) {
        for slot in &mut self.apps {
            if let Some(mut app) = slot.take() {
                app.close();
            }
        }
    }
}

fn host_ptr(host: &mut dyn AppHost) -> *mut (dyn AppHost + 'static) {
    let ptr = host as *mut (dyn AppHost + '_);
    // SAFETY: the pointer is only used on this thread until `run_with` clears it,
    // while `host` is still borrowed.
    unsafe { std::mem::transmute(ptr) }
}

fn with_host<T>(body: impl FnOnce(&mut dyn AppHost) -> T) -> Result<T, String> {
    let ptr = HOST
        .with(|cell| *cell.borrow())
        .ok_or_else(|| "no test host".to_owned())?;
    // SAFETY: `run_with` set this for the calling thread and clears it on return.
    Ok(body(unsafe { &mut *ptr }))
}

struct HeadlessHost;

impl AppHost for HeadlessHost {
    fn open(&mut self, request: HostOpen) -> Result<Box<dyn HostedApp>, String> {
        let app_dir = request
            .entry
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        let bundled = bundle(&BundleOptions {
            entry: request.entry,
            minify: false,
            test: false,
        })
        .map_err(|e| e.plain)?;
        let mut app = Headless::start(
            with_fixture_origin(&bundled.code, &app_dir),
            Size::new(px(request.width), px(request.height)),
        )
        .map_err(engine_error)?;
        let mut nudges = 0;
        if let Some(area) = request.safe_area {
            app.control().set_safe_area(Edges {
                top: px(area.top),
                right: px(area.right),
                bottom: px(area.bottom),
                left: px(area.left),
            });
            nudges += 1;
        }
        if let Some(height) = request.keyboard {
            app.control().set_keyboard_inset(px(height));
            nudges += 1;
        }
        settle(&mut app, nudges);
        Ok(Box::new(HeadlessApp { app: Some(app) }))
    }
}

struct HeadlessApp {
    app: Option<Headless>,
}

impl HeadlessApp {
    fn app(&mut self) -> Result<&mut Headless, String> {
        self.app
            .as_mut()
            .ok_or_else(|| "the app is closed".to_owned())
    }
}

impl HostedApp for HeadlessApp {
    fn texts(&mut self) -> Result<Vec<String>, String> {
        Ok(self.app()?.texts())
    }

    fn inputs(&mut self) -> Result<Vec<String>, String> {
        Ok(self.app()?.screen().input_values())
    }

    fn press(&mut self, selector: &str) -> Result<(), String> {
        let app = self.app()?;
        app.press_selector(selector)?;
        let _ = app.next_frame(Duration::from_secs(2));
        Ok(())
    }

    fn type_text(&mut self, selector: &str, text: &str) -> Result<(), String> {
        self.app()?.type_selector(selector, text)
    }

    fn scroll(&mut self, selector: &str, distance: f32) -> Result<(), String> {
        self.app()?.scroll_selector(selector, distance)
    }

    fn wait_for(&mut self, selector: &str) -> Result<(), String> {
        self.app()?.wait_selector(selector, Duration::from_secs(5))
    }

    fn tree(&mut self) -> Result<serde_json::Value, String> {
        let app = self.app()?;
        let screen = app.screen();
        let root = screen
            .tree(ROOT_ID)
            .ok_or_else(|| "the screen has no root".to_owned())?;
        Ok(node_json(&root, screen))
    }

    fn close(&mut self) {
        if let Some(app) = self.app.take() {
            app.stop();
        }
    }
}

impl Drop for HeadlessApp {
    fn drop(&mut self) {
        self.close();
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

fn node_json(node: &TreeNode, screen: &Screen) -> serde_json::Value {
    let view = screen.view(node.id);
    let mut map = serde_json::Map::new();
    map.insert("kind".into(), kind_name(node.kind).into());
    map.insert("frame".into(), frame_json(node.frame));
    if let Some(text) = &node.text {
        map.insert("text".into(), text.clone().into());
    }
    if let Some(test_id) = view.and_then(|view| view.test_id.as_ref()) {
        map.insert("testID".into(), test_id.as_ref().into());
    }
    if let Some(view) = view {
        if !view.spans.is_empty() {
            map.insert(
                "spans".into(),
                serde_json::Value::Array(view.spans.iter().map(span_json).collect()),
            );
        }
        if let Some(input) = &view.input {
            map.insert("value".into(), input.value.clone().into());
            map.insert(
                "placeholder".into(),
                input.config.placeholder.clone().into(),
            );
            map.insert("secure".into(), input.config.secure.into());
            map.insert(
                "keyboard".into(),
                keyboard_name(input.config.keyboard).into(),
            );
            map.insert("disabled".into(), input.config.disabled.into());
            map.insert("focused".into(), input.focused.into());
        }
        if let Some(source) = &view.source {
            map.insert("src".into(), source.uri.as_ref().into());
            map.insert("fit".into(), fit_name(view.resize_mode).into());
        }
        if node.kind == ViewKind::Scroll {
            map.insert(
                "horizontal".into(),
                view.scroll_horizontal.unwrap_or(false).into(),
            );
            map.insert(
                "offset".into(),
                serde_json::json!({ "x": view.scroll_x, "y": view.scroll_y }),
            );
            if let Some(size) = view.scroll_content {
                map.insert(
                    "content".into(),
                    serde_json::json!({
                        "width": size.width.0,
                        "height": size.height.0,
                    }),
                );
            }
        }
        if let Some(style) = &view.style
            && !style.transform.is_identity()
        {
            map.insert(
                "transform".into(),
                serde_json::Value::Array(
                    style
                        .transform
                        .ops()
                        .iter()
                        .map(|op| serde_json::json!({ transform_name(op.kind): op.value }))
                        .collect(),
                ),
            );
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

fn span_json(span: &crate::TextRun) -> serde_json::Value {
    serde_json::json!({
        "text": span.text,
        "fontSize": span.font_size,
        "fontWeight": span.font_weight,
        "italic": span.italic,
        "color": color_hex(span.color),
    })
}

fn color_hex(packed: u32) -> String {
    let rgb = packed >> 8;
    let alpha = packed & 0xff;
    if alpha == 0xff {
        format!("#{rgb:06x}")
    } else {
        format!("#{packed:08x}")
    }
}

fn keyboard_name(keyboard: KeyboardType) -> &'static str {
    match keyboard {
        KeyboardType::Text => "text",
        KeyboardType::Email => "email",
        KeyboardType::Number => "number",
        KeyboardType::Phone => "phone",
    }
}

fn fit_name(mode: ResizeMode) -> &'static str {
    match mode {
        ResizeMode::Cover => "cover",
        ResizeMode::Contain => "contain",
        ResizeMode::Stretch => "fill",
        ResizeMode::Center => "none",
        ResizeMode::ScaleDown => "scale-down",
    }
}

fn transform_name(kind: TransformKind) -> &'static str {
    match kind {
        TransformKind::TranslateX | TransformKind::TranslateXFraction => "translateX",
        TransformKind::TranslateY | TransformKind::TranslateYFraction => "translateY",
        TransformKind::Scale => "scale",
        TransformKind::ScaleX => "scaleX",
        TransformKind::ScaleY => "scaleY",
        TransformKind::Rotate => "rotate",
        TransformKind::RotateX => "rotateX",
        TransformKind::RotateY => "rotateY",
        TransformKind::SkewX => "skewX",
        TransformKind::SkewY => "skewY",
        TransformKind::Perspective => "perspective",
    }
}

/// Points the app's `fetch` at a loopback server of `root`, so a headless
/// run can read the same files `nlayer run` serves next to the page.
/// Android does not share that loopback: the caller forwards
/// [`fixture_file_port`] with `adb reverse`.
pub fn with_fixture_origin(code: &str, root: &Path) -> String {
    let origin = fixture_origin(root);
    format!("globalThis.__nlayerTestOrigin={origin:?};\n{code}")
}

/// The loopback port [`with_fixture_origin`] serves `root` on.
pub fn fixture_file_port(root: &Path) -> u16 {
    let _ = fixture_origin(root);
    FIXTURE.port
}

struct Fixture {
    port: u16,
    root: Arc<Mutex<PathBuf>>,
}

static FIXTURE: LazyLock<Fixture> = LazyLock::new(|| {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test file server");
    let port = listener.local_addr().expect("test file server").port();
    let root = Arc::new(Mutex::new(PathBuf::new()));
    let files = Arc::clone(&root);
    thread::Builder::new()
        .name("nlayer-test-files".to_owned())
        .spawn(move || {
            for connection in listener.incoming() {
                let Ok(socket) = connection else { continue };
                let root = files.lock().expect("test files").clone();
                let _ = serve_file(socket, &root);
            }
        })
        .expect("test file server");
    Fixture { port, root }
});

fn fixture_origin(root: &Path) -> String {
    let fixture = &*FIXTURE;
    *fixture.root.lock().expect("test files") = root.to_path_buf();
    format!("http://127.0.0.1:{}", fixture.port)
}

fn serve_file(mut socket: TcpStream, root: &Path) -> std::io::Result<()> {
    let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 1024];
    while !buf.windows(4).any(|window| window == b"\r\n\r\n") && buf.len() < 8192 {
        match socket.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(read) => buf.extend_from_slice(&tmp[..read]),
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let line = text.lines().next().unwrap_or("");
    let mut parts = line.split(' ');
    let method = parts.next().unwrap_or("");
    let url = parts.next().unwrap_or("/");
    if method != "GET" {
        return write_status(&mut socket, 405, "text/plain", b"method not allowed");
    }
    let path = url.split('?').next().unwrap_or("/");
    if path.split(['/', '\\']).any(|part| part == "..") {
        return write_status(&mut socket, 400, "text/plain", b"bad request");
    }
    let rel = path.trim_start_matches('/');
    let file = root.join(rel);
    let Ok(body) = fs::read(&file) else {
        return write_status(&mut socket, 404, "text/plain", b"not found");
    };
    let mime = if rel.ends_with(".json") {
        "application/json"
    } else if rel.ends_with(".png") {
        "image/png"
    } else {
        "application/octet-stream"
    };
    write_status(&mut socket, 200, mime, &body)
}

fn write_status(
    socket: &mut TcpStream,
    status: u16,
    mime: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    write!(
        socket,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    socket.write_all(body)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{HOST, HeadlessHost, HostGuard, host_ptr, run_path};

    const APP: &str = r#"
import { mount, signal } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
    const count = signal(0);
    return (
        <View style={{ width: 80, height: 40 }} onPress={() => count.set(count() + 1)}>
            <Text>Count: {count()}</Text>
        </View>
    );
}
mount(App);
"#;

    fn with_host<T>(body: impl FnOnce() -> T) -> T {
        let mut host = HeadlessHost;
        HOST.with(|cell| *cell.borrow_mut() = Some(host_ptr(&mut host)));
        let guard = HostGuard;
        let value = body();
        drop(guard);
        value
    }

    fn app(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nlayer-steps-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("App.tsx"), APP).unwrap();
        dir
    }

    #[test]
    fn an_entry_runs_the_steps_against_the_opened_app() {
        let dir = app("press");
        let report = with_host(|| {
            run_path(
                &dir.join("App.tsx"),
                None,
                Some(
                    "app.press(\"Count: 0\");\nexpect(app.texts()).toEqual([\"Count: 1\"]);\napp.close();\nconst again = open(\"./App.tsx\");\nexpect(again.texts()).toEqual([\"Count: 0\"]);\n",
                ),
            )
        })
        .expect("steps run");
        assert!(report.ok(), "{report}");
        assert_eq!(report.files[0].name, "App.tsx");
        assert_eq!(report.files[0].cases[0].name, "steps");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Deletes `path` when dropped, including after a panic.
    struct Remove(std::path::PathBuf);

    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_relative_entry_is_not_joined_onto_its_parent() {
        let root =
            std::path::PathBuf::from(format!("target/nlayer-steps-rel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("gallery");
        fs::create_dir_all(&dir).unwrap();
        let _remove = Remove(root);
        fs::write(dir.join("App.tsx"), APP).unwrap();
        let entry = dir.join("App.tsx");
        assert!(entry.is_relative(), "{}", entry.display());
        let report = with_host(|| {
            run_path(
                &entry,
                None,
                Some(
                    "expect(app.texts()).toEqual([\"Count: 0\"]);\napp.close();\nconst again = open(\"./App.tsx\");\nexpect(again.texts()).toEqual([\"Count: 0\"]);\n",
                ),
            )
        })
        .expect("steps run");
        assert!(report.ok(), "{report}");
        assert_eq!(report.files[0].name, "App.tsx");
    }

    #[test]
    fn a_failing_step_is_that_case() {
        let dir = app("fail");
        let report = with_host(|| {
            run_path(
                &dir.join("App.tsx"),
                None,
                Some("expect(app.texts()).toEqual([\"missing\"]);\n"),
            )
        })
        .expect("the file runs");
        assert!(!report.ok(), "{report}");
        assert_eq!(report.files[0].name, "App.tsx");
        let error = report.files[0].cases[0].error.as_deref().unwrap_or("");
        assert!(error.contains("missing"), "{report}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_steps_only_open_the_app() {
        let dir = app("empty");
        let report = with_host(|| run_path(&dir.join("App.tsx"), None, Some(""))).expect("opens");
        assert!(report.ok(), "{report}");
        assert_eq!(report.passed(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_step_that_does_not_parse_fails_the_file() {
        let dir = app("syntax");
        let report = with_host(|| run_path(&dir.join("App.tsx"), None, Some("const = ;\n")))
            .expect("the file is reported");
        assert!(!report.ok(), "{report}");
        assert!(report.files[0].error.is_some(), "{report}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_test_file_is_not_an_entry() {
        let dir = app("file");
        fs::write(
            dir.join("App.test.tsx"),
            r#"
import { expect, open, test } from "@nlayer/test";

test("shows the count", () => {
    const app = open("./App.tsx");
    expect(app.texts()).toEqual(["Count: 0"]);
});
"#,
        )
        .unwrap();
        let report = with_host(|| {
            run_path(
                &dir.join("App.test.tsx"),
                None,
                Some("throw new Error(\"steps\");\n"),
            )
        })
        .expect("the file runs");
        assert!(report.ok(), "{report}");
        assert_eq!(report.files[0].cases[0].name, "shows the count");
        let _ = fs::remove_dir_all(&dir);
    }
}
