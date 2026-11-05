//! `nlayer run --target web`.
//!
//! Serves the page, the wasm engine, the app's development bundle and files
//! next to the app (a demo's `quote.json`) on this machine. TSX and imported
//! assets are watched by polling their bytes. When JS changes and the new
//! bundle can patch the previous one, the server sends a patch frame and
//! then the new bundle on `/ws`. An asset-only change replaces files and
//! sends no patch. A failed rebuild sends the diagnostic and keeps the
//! previous file table and the previous bundle for new connections. The page
//! does not reload.
//!
//! One build is one [`bundle_hot`]: the page runs that bundle, so no cold
//! `bundle()` runs beside it. That one would compile every module a second
//! time, and report its classes a second time, for a `bundle.js` the page
//! never loads.

use std::{
    collections::HashMap,
    fs,
    hash::{Hash, Hasher},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use nlayer_bundler::{HotBundle, HotPatch, bundle_hot};
use nlayer_engine::protocol::{self, WS_PATH};

use crate::{BuildError, resolve_entry};

const PAGE_HTML: &str = include_str!("../../nlayer-platform-web/web/index.html");

/// The page `nlayer run --target web` serves: `PAGE_HTML` with the dev
/// flag the page reads. The packaged page keeps the plain form.
static DEV_PAGE: LazyLock<String> = LazyLock::new(|| {
    PAGE_HTML.replace(
        "<script type=\"module\">",
        "<script>window.__nlayerDev = true;</script>\n    <script type=\"module\">",
    )
});

/// How often the app's sources are checked. A save lands on the next poll.
const POLL: Duration = Duration::from_millis(150);

/// What one build attempt did.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildOutcome {
    /// The port the page is served on.
    pub port: u16,
    /// The app compiled.
    pub success: bool,
    /// The served files changed, or a new development bundle was published.
    pub updated: bool,
    /// This is the first build the page can run.
    pub first_success: bool,
    /// Wall time of the attempt, in milliseconds.
    pub elapsed_ms: f64,
    /// Colored diagnostic. Empty when `success` is set.
    pub error: String,
    /// The app's imported asset files, as absolute paths. A native host
    /// copies them into the device after a successful build.
    pub assets: Vec<PathBuf>,
}

/// What `nlayer run --target web` serves.
pub struct RunOptions {
    /// An entry module, or a directory whose `App.tsx` is the entry.
    pub entry: PathBuf,
    /// `0` asks the OS for a port.
    pub port: u16,
    /// Kept for callers of `nlayer run --release`. The development script is
    /// the hot bundle and is not minified, so the dev server does not read
    /// this.
    pub release: bool,
    /// The wasm the page instantiates (`nlayer.wasm`).
    pub wasm: Vec<u8>,
    /// Called after every build attempt, including the one before [`serve`]
    /// returns. The CLI prints it and opens a browser on the first success.
    pub on_build: Option<Arc<dyn Fn(BuildOutcome) + Send + Sync>>,
}

/// A failed `run`.
#[derive(Debug)]
pub struct RunError {
    message: String,
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RunError {}

impl RunError {
    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<BuildError> for RunError {
    fn from(error: BuildError) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl From<std::io::Error> for RunError {
    fn from(error: std::io::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

struct LiveBundle {
    version: u64,
    code: Arc<[u8]>,
    source_map: Option<Arc<[u8]>>,
}

struct Sub {
    id: u64,
    tx: Sender<Arc<DevEvent>>,
}

enum DevEvent {
    Patch {
        version: u64,
        base: u64,
        code: Arc<[u8]>,
        source_map: Option<Arc<[u8]>>,
    },
    Bundle {
        version: u64,
        code: Arc<[u8]>,
        source_map: Option<Arc<[u8]>>,
    },
    BuildFailed {
        message: String,
    },
}

struct Inner {
    generation: u64,
    error: Option<String>,
    /// Served bodies, keyed without a leading slash. `nlayer.wasm` stays
    /// across a failed rebuild; everything else is the last good app.
    files: HashMap<String, Arc<[u8]>>,
    watched: Vec<PathBuf>,
    stamp: u64,
    /// Last hot bundle a patch can apply to. Kept across a failed rebuild.
    hot: Option<HotBundle>,
    /// Script bytes of `generation`, for new sockets. Not the HTTP file:
    /// that one may carry a `sourceMappingURL` comment.
    live: Option<LiveBundle>,
    next_sub: u64,
    subs: Vec<Sub>,
}

struct Shared {
    stop: AtomicBool,
    port: u16,
    entry: PathBuf,
    app_dir: PathBuf,
    on_build: Arc<dyn Fn(BuildOutcome) + Send + Sync>,
    inner: Mutex<Inner>,
}

/// A page being served. Dropping it stops the server.
#[must_use = "dropping the server stops it"]
pub struct DevServer {
    shared: Arc<Shared>,
    stop_tx: Option<Sender<()>>,
    http: Option<JoinHandle<()>>,
    watch: Option<JoinHandle<()>>,
}

impl DevServer {
    /// The bound port.
    pub fn port(&self) -> u16 {
        self.shared.port
    }

    /// `http://127.0.0.1:<port>/`.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port())
    }

    /// Development-bundle version. `0` means no good build yet. Asset-only
    /// updates do not bump it.
    pub fn generation(&self) -> u64 {
        self.shared.inner.lock().expect("dev server").generation
    }

    /// The plain text of the latest failed build, if it is still the latest.
    pub fn last_error(&self) -> Option<String> {
        self.shared.inner.lock().expect("dev server").error.clone()
    }

    /// The latest successful development bundle: its version and script
    /// bytes. A native host writes the script as `app.js` before its first
    /// launch; later builds arrive over `/ws`.
    pub fn dev_bundle(&self) -> Option<(u64, Arc<[u8]>)> {
        let inner = self.shared.inner.lock().expect("dev server");
        inner
            .live
            .as_ref()
            .map(|live| (live.version, Arc::clone(&live.code)))
    }

    /// Blocks while the page is served.
    pub fn park(&self) {
        while !self.shared.stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for DevServer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.watch.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.http.take() {
            let _ = thread.join();
        }
    }
}

/// Serves `options` until the returned server is dropped.
///
/// The first build finishes before this returns. Later builds run on a
/// background thread. A compile error does not change what is served.
pub fn serve(options: RunOptions) -> Result<DevServer, RunError> {
    let entry = resolve_entry(&options.entry)?;
    let app_dir = entry
        .parent()
        .ok_or_else(|| RunError::message("entry has no parent directory"))?
        .to_path_buf();
    let listener = TcpListener::bind(("127.0.0.1", options.port))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let on_build = options.on_build.unwrap_or_else(|| Arc::new(|_outcome| {}));
    let mut files = HashMap::new();
    files.insert("nlayer.wasm".to_owned(), Arc::<[u8]>::from(options.wasm));
    let shared = Arc::new(Shared {
        stop: AtomicBool::new(false),
        port,
        entry,
        app_dir,
        on_build,
        inner: Mutex::new(Inner {
            generation: 0,
            error: None,
            files,
            watched: Vec::new(),
            stamp: 0,
            hot: None,
            live: None,
            next_sub: 1,
            subs: Vec::new(),
        }),
    });
    let http_shared = Arc::clone(&shared);
    let http = thread::Builder::new()
        .name("nlayer-http".to_owned())
        .spawn(move || http_loop(listener, http_shared))
        .map_err(RunError::from)?;
    rebuild(&shared);
    let (stop_tx, stop_rx) = channel();
    let watch_shared = Arc::clone(&shared);
    let watch = match thread::Builder::new()
        .name("nlayer-watch".to_owned())
        .spawn(move || watch_loop(watch_shared, stop_rx))
    {
        Ok(watch) => watch,
        Err(error) => {
            shared.stop.store(true, Ordering::Relaxed);
            let _ = http.join();
            return Err(error.into());
        }
    };
    Ok(DevServer {
        shared,
        stop_tx: Some(stop_tx),
        http: Some(http),
        watch: Some(watch),
    })
}

/// The wasm the page instantiates.
///
/// `NLAYER_WASM` wins, then `nlayer.wasm` beside the binary, then a fresh
/// build of `nlayer-platform-web` for `wasm32-unknown-unknown`. The build
/// uses `target/nlayer-wasm` so it does not take the target-dir lock a
/// surrounding `cargo run` already holds, and it turns debug info off: the
/// default dev profile embeds hundreds of megabytes of it, which the
/// browser then has to instantiate.
pub fn web_module() -> Result<Vec<u8>, RunError> {
    if let Some(path) = std::env::var_os("NLAYER_WASM") {
        return fs::read(&path).map_err(|error| {
            RunError::message(format!(
                "NLAYER_WASM {}: {error}",
                Path::new(&path).display()
            ))
        });
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let beside = dir.join("nlayer.wasm");
        if beside.is_file() {
            return fs::read(&beside).map_err(RunError::from);
        }
    }
    let Some(workspace) = workspace_root() else {
        return Err(RunError::message(
            "no web runtime. Set NLAYER_WASM, or place nlayer.wasm next to the nlayer binary",
        ));
    };
    let sources = newest_source(&workspace);
    let artifact = wasm_artifact(&workspace);
    if let Ok(modified) = fs::metadata(&artifact).and_then(|meta| meta.modified())
        && modified >= sources
    {
        return fs::read(&artifact).map_err(RunError::from);
    }
    let target = workspace.join("target/nlayer-wasm");
    eprintln!("building the web runtime");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = std::process::Command::new(cargo)
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .args([
            "build",
            "-p",
            "nlayer-platform-web",
            "--target",
            "wasm32-unknown-unknown",
            "--target-dir",
        ])
        .arg(&target)
        .current_dir(&workspace)
        .status()
        .map_err(|error| RunError::message(format!("cargo: {error}")))?;
    if !status.success() {
        return Err(RunError::message(
            "the web runtime failed to build (wasm32-unknown-unknown; `rustup target add wasm32-unknown-unknown`)",
        ));
    }
    let built = target.join("wasm32-unknown-unknown/debug/nlayer_platform_web.wasm");
    fs::read(&built).map_err(|error| {
        RunError::message(format!("web runtime {} missing: {error}", built.display()))
    })
}

fn wasm_artifact(workspace: &Path) -> PathBuf {
    workspace.join("target/nlayer-wasm/wasm32-unknown-unknown/debug/nlayer_platform_web.wasm")
}

pub(crate) fn workspace_root() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(dir) = std::env::current_dir() {
        starts.push(dir);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        starts.push(dir.to_path_buf());
    }
    starts.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    for start in starts {
        let mut dir = start;
        loop {
            let manifest = dir.join("Cargo.toml");
            if let Ok(text) = fs::read_to_string(&manifest)
                && text.contains("[workspace]")
                && text.contains("nlayer-platform-web")
            {
                return Some(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

fn newest_source(workspace: &Path) -> SystemTime {
    let mut newest = UNIX_EPOCH;
    consider(&workspace.join("Cargo.toml"), &mut newest);
    consider(&workspace.join("Cargo.lock"), &mut newest);
    consider(&workspace.join(".cargo/config.toml"), &mut newest);
    for name in ["nlayer-core", "nlayer-engine", "nlayer-platform-web"] {
        consider_tree(&workspace.join("crates").join(name), &mut newest);
    }
    newest
}

pub(crate) fn consider(path: &Path, newest: &mut SystemTime) {
    if let Ok(modified) = fs::metadata(path).and_then(|meta| meta.modified())
        && modified > *newest
    {
        *newest = modified;
    }
}

pub(crate) fn consider_tree(dir: &Path, newest: &mut SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" {
            continue;
        }
        if path.is_dir() {
            consider_tree(&path, newest);
        } else if name == "Cargo.toml"
            || path
                .extension()
                .is_some_and(|ext| ext == "rs" || ext == "java")
        {
            consider(&path, newest);
        }
    }
}

fn rebuild(shared: &Shared) {
    // One build: the page runs the hot bundle from the socket (a patch, then
    // the new bundle). A cold `bundle()` beside it would compile every module
    // again and report its classes twice for an output nothing loads.
    let started = Instant::now();
    let elapsed = |from: Instant| from.elapsed().as_secs_f64() * 1000.0;
    let outcome = match bundle_hot(&shared.entry, false) {
        Ok(hot) => match read_assets(&shared.app_dir, &hot) {
            Ok(app) => publish(shared, app, hot, elapsed(started)),
            Err(message) => fail_outcome(shared, message.clone(), message, elapsed(started)),
        },
        Err(error) => fail_outcome(shared, error.plain, error.colored, elapsed(started)),
    };
    (shared.on_build)(outcome);
}

fn publish(
    shared: &Shared,
    app: HashMap<String, Vec<u8>>,
    hot: HotBundle,
    elapsed_ms: f64,
) -> BuildOutcome {
    let (script, map) = hot.script();
    let assets = hot.assets.clone();
    let previous = shared.inner.lock().expect("dev server").hot.clone();
    let patch = previous.as_ref().map(|previous| hot.patch(previous));
    let watched = watch_paths(&shared.app_dir, Some(&hot));
    let stamp = stamp(&watched);
    let mut inner = shared.inner.lock().expect("dev server");
    let was_zero = inner.generation == 0;
    let files_changed = install_app(&mut inner.files, app);
    inner.error = None;
    inner.watched = watched;
    inner.stamp = stamp;
    // `Unchanged` is an asset-only (or empty) rebuild: replace files, no patch.
    let js_changed = !matches!(patch, Some(HotPatch::Unchanged));
    if js_changed {
        inner.generation += 1;
        let version = inner.generation;
        let code: Arc<[u8]> = Arc::from(script.into_bytes());
        let source_map = map.map(|text| Arc::<[u8]>::from(text.into_bytes()));
        inner.live = Some(LiveBundle {
            version,
            code: Arc::clone(&code),
            source_map: source_map.clone(),
        });
        if let Some(HotPatch::Update {
            code: patch_code,
            source_map: patch_map,
        }) = patch
        {
            let patch_code: Arc<[u8]> = Arc::from(patch_code.into_bytes());
            let patch_map = patch_map.map(|text| Arc::<[u8]>::from(text.into_bytes()));
            fanout(
                &mut inner,
                Arc::new(DevEvent::Patch {
                    version,
                    base: version - 1,
                    code: patch_code,
                    source_map: patch_map,
                }),
            );
        }
        fanout(
            &mut inner,
            Arc::new(DevEvent::Bundle {
                version,
                code,
                source_map,
            }),
        );
    }
    inner.hot = Some(hot);
    let first_success = was_zero && inner.generation > 0;
    BuildOutcome {
        port: shared.port,
        success: true,
        updated: files_changed || js_changed,
        first_success,
        elapsed_ms,
        error: String::new(),
        assets,
    }
}

fn fail_outcome(shared: &Shared, plain: String, colored: String, elapsed_ms: f64) -> BuildOutcome {
    fail(shared, plain);
    BuildOutcome {
        port: shared.port,
        success: false,
        updated: false,
        first_success: false,
        elapsed_ms,
        error: colored,
        assets: Vec::new(),
    }
}

fn fail(shared: &Shared, plain: String) {
    let watched = watch_paths(&shared.app_dir, None);
    let stamp = stamp(&watched);
    let mut inner = shared.inner.lock().expect("dev server");
    inner.error = Some(plain.clone());
    inner.watched = watched;
    inner.stamp = stamp;
    // The previous file table and live bundle stay. New sockets still get
    // that bundle; this frame is for sockets already connected.
    fanout(
        &mut inner,
        Arc::new(DevEvent::BuildFailed { message: plain }),
    );
}

fn install_app(files: &mut HashMap<String, Arc<[u8]>>, app: HashMap<String, Vec<u8>>) -> bool {
    let same = files
        .keys()
        .filter(|key| key.as_str() != "nlayer.wasm")
        .count()
        == app.len()
        && app.iter().all(|(key, bytes)| {
            files
                .get(key)
                .is_some_and(|old| old.as_ref() == bytes.as_slice())
        });
    if same {
        return false;
    }
    files.retain(|key, _| key == "nlayer.wasm");
    files.extend(
        app.into_iter()
            .map(|(key, bytes)| (key, Arc::<[u8]>::from(bytes))),
    );
    true
}

/// The app's imported assets, as the page fetches them. `bundle.js` is not
/// among them: the development bundle arrives on `/ws`.
fn read_assets(app_dir: &Path, hot: &HotBundle) -> Result<HashMap<String, Vec<u8>>, String> {
    let mut files = HashMap::new();
    for rel in &hot.assets {
        let key = rel.to_string_lossy().replace('\\', "/");
        if reserved(&key) {
            return Err(format!("asset `{key}` uses a path the page reserves"));
        }
        let bytes = fs::read(app_dir.join(rel)).map_err(|error| format!("asset {key}: {error}"))?;
        files.insert(key, bytes);
    }
    Ok(files)
}

fn reserved(path: &str) -> bool {
    crate::web_assets::reserved(path)
}

pub(crate) fn watch_paths(app_dir: &Path, hot: Option<&HotBundle>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    collect_sources(app_dir, &mut paths);
    if let Some(hot) = hot {
        paths.extend(hot.watch_files.iter().cloned());
        for rel in &hot.assets {
            paths.push(app_dir.join(rel));
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn collect_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_sources(&path, out);
        } else if is_watched(&path) {
            out.push(path);
        }
    }
}

fn is_watched(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "png" | "jpg" | "jpeg" | "webp")
    )
}

pub(crate) fn stamp(paths: &[PathBuf]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for path in paths {
        path.hash(&mut hasher);
        match fs::read(path) {
            Ok(bytes) => bytes.hash(&mut hasher),
            Err(error) => error.kind().hash(&mut hasher),
        }
    }
    hasher.finish()
}

fn watch_loop(shared: Arc<Shared>, stop: Receiver<()>) {
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        let watched = shared.inner.lock().expect("dev server").watched.clone();
        let next = stamp(&watched);
        let changed = shared.inner.lock().expect("dev server").stamp != next;
        if changed {
            rebuild(&shared);
        }
        match stop.recv_timeout(POLL) {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn http_loop(listener: TcpListener, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((socket, _)) => handle(socket, Arc::clone(&shared)),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

fn handle(mut socket: TcpStream, shared: Arc<Shared>) {
    // accept() on macOS keeps the listener's O_NONBLOCK. A body bigger than
    // the send buffer then stops at WouldBlock, and the page sees a cut-off
    // wasm. One request, one blocking response.
    if let Err(error) = socket.set_nonblocking(false) {
        eprintln!("nlayer: {error}");
        return;
    }
    let _ = socket.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 2048];
    while !buf.windows(4).any(|window| window == b"\r\n\r\n") && buf.len() < 16 * 1024 {
        match socket.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(read) => buf.extend_from_slice(&tmp[..read]),
        }
    }
    let Ok(text) = std::str::from_utf8(&buf) else {
        send(
            &mut socket,
            400,
            "Bad Request",
            "text/plain",
            b"bad request",
        );
        return;
    };
    let head = text.split("\r\n\r\n").next().unwrap_or("");
    match request_path(head) {
        Ok(path) if is_dev_socket(&path) => {
            let upgrade = websocket_upgrade(head);
            let complete = buf.windows(4).any(|window| window == b"\r\n\r\n");
            if !upgrade || !complete {
                send(
                    &mut socket,
                    400,
                    "Bad Request",
                    "text/plain",
                    b"expected a websocket upgrade",
                );
                return;
            }
            let shared = Arc::clone(&shared);
            if let Err(error) = thread::Builder::new()
                .name("nlayer-ws".to_owned())
                .spawn(move || websocket(socket, buf, shared))
            {
                eprintln!("nlayer: {error}");
            }
        }
        Ok(path) => match lookup(&shared, &path) {
            Ok((mime, body)) => send(&mut socket, 200, "OK", mime, body.as_slice()),
            Err(status) => send(
                &mut socket,
                status,
                if status == 404 {
                    "Not Found"
                } else {
                    "Bad Request"
                },
                "text/plain; charset=utf-8",
                if status == 404 {
                    b"not found"
                } else {
                    b"bad request"
                },
            ),
        },
        Err(405) => send(
            &mut socket,
            405,
            "Method Not Allowed",
            "text/plain",
            b"method not allowed",
        ),
        Err(_) => send(
            &mut socket,
            400,
            "Bad Request",
            "text/plain",
            b"bad request",
        ),
    }
}

enum Body {
    Text(&'static str),
    Bytes(Vec<u8>),
    Shared(Arc<[u8]>),
}

impl Body {
    fn as_slice(&self) -> &[u8] {
        match self {
            Body::Text(text) => text.as_bytes(),
            Body::Bytes(bytes) => bytes,
            Body::Shared(bytes) => bytes,
        }
    }
}

fn lookup(shared: &Shared, path: &str) -> Result<(&'static str, Body), u16> {
    if let Some((mime, text)) = host(path) {
        return Ok((mime, Body::Text(text)));
    }
    let known = {
        let inner = shared.inner.lock().expect("dev server");
        inner.files.get(path).map(Arc::clone)
    };
    if let Some(bytes) = known {
        return Ok((mime_of(path), Body::Shared(bytes)));
    }
    // A file beside the app, not an imported asset: `fetch("/quote.json")`.
    if path.is_empty() || path.ends_with('/') {
        return Err(404);
    }
    match fs::read(shared.app_dir.join(path)) {
        Ok(bytes) => Ok((mime_of(path), Body::Bytes(bytes))),
        Err(_) => Err(404),
    }
}

fn host(path: &str) -> Option<(&'static str, &'static str)> {
    match path {
        "" | "index.html" => Some(("text/html; charset=utf-8", dev_page())),
        _ => crate::web_assets::find(path),
    }
}

fn dev_page() -> &'static str {
    // The host connects to `/ws` (a hello, then patch and bundle frames).
    // This page does not poll or reload.
    DEV_PAGE.as_str()
}

fn mime_of(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("map" | "json") => "application/json",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("html") => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn request_path(head: &str) -> Result<String, u16> {
    let line = head.lines().next().unwrap_or("");
    let mut parts = line.split(' ');
    let method = parts.next().ok_or(400_u16)?;
    let url = parts.next().ok_or(400_u16)?;
    if method != "GET" {
        return Err(405);
    }
    route(url).map_err(|_| 400)
}

fn route(url: &str) -> Result<String, ()> {
    let path = url.split('?').next().unwrap_or("/");
    let path = percent_decode(path)?;
    if path.split(['/', '\\']).any(|part| part == "..") {
        return Err(());
    }
    Ok(path.trim_start_matches('/').to_owned())
}

fn percent_decode(input: &str) -> Result<String, ()> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = hex(bytes[index + 1]).ok_or(())?;
                let low = hex(bytes[index + 2]).ok_or(())?;
                out.push((high << 4) | low);
                index += 3;
            }
            b'%' => return Err(()),
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn send(socket: &mut TcpStream, status: u16, reason: &str, content_type: &str, body: &[u8]) {
    if let Err(error) = respond(socket, status, reason, content_type, body) {
        eprintln!("nlayer: {error}");
    }
}

fn respond(
    socket: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    write!(
        socket,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    socket.write_all(body)?;
    socket.flush()
}

fn is_dev_socket(path: &str) -> bool {
    path == WS_PATH.trim_start_matches('/')
}

fn websocket_upgrade(head: &str) -> bool {
    head.lines().any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.eq_ignore_ascii_case("upgrade") && value.trim().eq_ignore_ascii_case("websocket")
    })
}

fn open_sub(shared: &Shared) -> (u64, Receiver<Arc<DevEvent>>, Option<Arc<DevEvent>>) {
    let mut inner = shared.inner.lock().expect("dev server");
    let id = inner.next_sub;
    inner.next_sub = inner.next_sub.wrapping_add(1);
    let (tx, rx) = channel();
    inner.subs.push(Sub { id, tx });
    let snap = if let Some(live) = &inner.live {
        Some(Arc::new(DevEvent::Bundle {
            version: live.version,
            code: Arc::clone(&live.code),
            source_map: live.source_map.clone(),
        }))
    } else {
        inner
            .error
            .clone()
            .map(|message| Arc::new(DevEvent::BuildFailed { message }))
    };
    (id, rx, snap)
}

fn unsubscribe(shared: &Shared, id: u64) {
    shared
        .inner
        .lock()
        .expect("dev server")
        .subs
        .retain(|sub| sub.id != id);
}

fn fanout(inner: &mut Inner, event: Arc<DevEvent>) {
    inner
        .subs
        .retain(|sub| sub.tx.send(Arc::clone(&event)).is_ok());
}

/// Bytes already read as the HTTP request, then the rest of the socket.
struct Prefixed<T> {
    prefix: std::io::Cursor<Vec<u8>>,
    inner: T,
}

impl<T: Read> Read for Prefixed<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if (self.prefix.position() as usize) < self.prefix.get_ref().len() {
            let read = self.prefix.read(buf)?;
            if read > 0 {
                return Ok(read);
            }
        }
        self.inner.read(buf)
    }
}

impl<T: Write> Write for Prefixed<T> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

type Client = tungstenite::WebSocket<Prefixed<TcpStream>>;

fn websocket(socket: TcpStream, prefix: Vec<u8>, shared: Arc<Shared>) {
    // Handshake on a blocking socket. After that, reads are non-blocking:
    // tungstenite treats a read timeout as fatal, and WouldBlock is the
    // error it expects when the peer has not written a frame.
    let _ = socket.set_read_timeout(None);
    let stream = Prefixed {
        prefix: std::io::Cursor::new(prefix),
        inner: socket,
    };
    let mut ws = match tungstenite::accept(stream) {
        Ok(ws) => ws,
        Err(error) => {
            eprintln!("nlayer: websocket handshake failed: {error}");
            return;
        }
    };
    if let Err(error) = ws.get_mut().inner.set_nonblocking(true) {
        eprintln!("nlayer: {error}");
        return;
    }
    let (id, rx, snap) = open_sub(&shared);
    let mut failed = send_json(&mut ws, &protocol::Hello::new(false)).is_err();
    if !failed && let Some(event) = snap.as_ref() {
        failed = send_event(&mut ws, event).is_err();
    }
    while !failed && !shared.stop.load(Ordering::Relaxed) {
        while let Ok(event) = rx.try_recv() {
            if send_event(&mut ws, &event).is_err() {
                failed = true;
                break;
            }
        }
        if failed {
            break;
        }
        match ws.read() {
            Ok(tungstenite::Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => break,
        }
    }
    unsubscribe(&shared, id);
}

fn send_json(ws: &mut Client, value: &impl serde::Serialize) -> Result<(), ()> {
    let text = serde_json::to_string(value).map_err(|error| {
        eprintln!("nlayer: {error}");
    })?;
    send_message(ws, tungstenite::Message::text(text))
}

fn send_bin(ws: &mut Client, bytes: &[u8]) -> Result<(), ()> {
    send_message(ws, tungstenite::Message::binary(bytes.to_vec()))
}

fn send_message(ws: &mut Client, message: tungstenite::Message) -> Result<(), ()> {
    let started = Instant::now();
    let mut pending = Some(message);
    loop {
        if started.elapsed() > Duration::from_secs(30) {
            eprintln!("nlayer: websocket write timed out");
            return Err(());
        }
        if let Some(message) = pending.take() {
            match ws.send(message) {
                Ok(()) => return Ok(()),
                Err(tungstenite::Error::WriteBufferFull(message)) => pending = Some(message),
                Err(tungstenite::Error::Io(error))
                    if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => {
                    eprintln!("nlayer: {error}");
                    return Err(());
                }
            }
        }
        match ws.flush() {
            Ok(()) if pending.is_none() => return Ok(()),
            Ok(()) => {}
            Err(tungstenite::Error::Io(error))
                if error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                eprintln!("nlayer: {error}");
                return Err(());
            }
        }
    }
}

fn send_event(ws: &mut Client, event: &DevEvent) -> Result<(), ()> {
    match event {
        DevEvent::Patch {
            version,
            base,
            code,
            source_map,
        } => {
            send_json(
                ws,
                &protocol::Patch::new(*version, *base, source_map.is_some()),
            )?;
            send_bin(ws, code)?;
            if let Some(map) = source_map {
                send_bin(ws, map)?;
            }
        }
        DevEvent::Bundle {
            version,
            code,
            source_map,
        } => {
            send_json(ws, &protocol::Bundle::new(*version, source_map.is_some()))?;
            send_bin(ws, code)?;
            if let Some(map) = source_map {
                send_bin(ws, map)?;
            }
        }
        DevEvent::BuildFailed { message } => {
            send_json(ws, &protocol::BuildFailed::new(message.clone()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RunOptions, fs, route, serve};
    use std::sync::{Mutex, Once};

    /// `nlayer::build` lines this binary has logged.
    static BUILD_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

    struct BuildLog;

    impl log::Log for BuildLog {
        fn enabled(&self, meta: &log::Metadata<'_>) -> bool {
            meta.target() == "nlayer::build" && meta.level() <= log::Level::Warn
        }

        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                BUILD_LINES
                    .lock()
                    .expect("build lines")
                    .push(record.args().to_string());
            }
        }

        fn flush(&self) {}
    }

    fn capture_build_lines() {
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| {
            let _ = log::set_logger(&BuildLog);
            log::set_max_level(log::LevelFilter::Warn);
        });
    }

    #[test]
    fn routes_reject_dot_dot_after_decoding() {
        assert_eq!(route("/boot.js").unwrap(), "boot.js");
        assert_eq!(route("/img/icon.png?x=1").unwrap(), "img/icon.png");
        assert!(route("/../Cargo.toml").is_err());
        assert!(route("/%2e%2e/Cargo.toml").is_err());
        assert!(route("/%").is_err());
    }

    /// One build parses each class string once, so a class is reported once.
    /// A second compiler run per build would report it again.
    #[test]
    fn a_build_reports_an_unknown_class_once() {
        capture_build_lines();
        let dir = std::env::temp_dir().join(format!("nlayer-run-warn-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("App.tsx"),
            r#"import { mount } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
  return (
    <View class="classwarnzz">
      <Text>one</Text>
    </View>
  );
}
mount(App);
"#,
        )
        .expect("the app");
        BUILD_LINES.lock().expect("build lines").clear();
        let server = match serve(RunOptions {
            entry: dir.clone(),
            port: 0,
            release: false,
            wasm: Vec::new(),
            on_build: None,
        }) {
            Ok(server) => server,
            Err(error) => panic!("the server starts: {error}"),
        };
        let lines = BUILD_LINES.lock().expect("build lines").clone();
        drop(server);
        let _ = fs::remove_dir_all(&dir);
        let reported = lines
            .iter()
            .filter(|line| line.ends_with("unknown classes: classwarnzz"))
            .count();
        assert_eq!(reported, 1, "one build reports a class once: {lines:#?}");
    }
}
