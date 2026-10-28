//! `nlayer run --target web`.
//!
//! Serves the page, the wasm engine, the app's bundle and files next to the
//! app (a demo's `quote.json`) on this machine.
//! TSX and imported assets are watched by polling their bytes. A rebuild that
//! changes the bundle or an asset bumps a generation the page polls; the page
//! reloads, which drops the running app. A failed rebuild is reported and the
//! last good bundle stays. This is not hot reload: nothing is patched in place.

use std::{
    collections::HashMap,
    fs,
    hash::{Hash, Hasher},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use nlayer_bundler::{Bundle, BundleOptions, bundle};

use crate::{BuildError, resolve_entry};

const BOOT_JS: &str = include_str!("../../nlayer-platform-web/web/boot.js");
const MOUNTER_JS: &str = include_str!("../../nlayer-platform-web/web/nlayer.js");
const BASE_CSS: &str = include_str!("../../nlayer-platform-web/web/base.css");
const PAGE_HTML: &str = include_str!("../../nlayer-platform-web/web/index.html");
const RUNTIME_JS: &str = include_str!("../../nlayer-platform-web/web/browser/runtime.js");
const VALUES_JS: &str = include_str!("../../nlayer-platform-web/web/browser/values.js");
const HOST_JS: &str = include_str!("../../nlayer-platform-web/web/host.js");

/// How often the app's sources are checked. A save lands on the next poll.
const POLL: Duration = Duration::from_millis(150);

/// The page reloads when this changes. Full reload, not a patch.
const RELOAD: &str = r#"<script type="module">
let seen;
setInterval(async () => {
  try {
    const next = await (await fetch("/__nlayer/generation", { cache: "no-store" })).text();
    if (seen === undefined) seen = next;
    else if (seen !== next) location.reload();
  } catch {}
}, 200);
</script>
"#;

/// What one build attempt did.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildOutcome {
    /// The port the page is served on.
    pub port: u16,
    /// The app compiled.
    pub success: bool,
    /// The served bundle or an asset changed, so the page reloads.
    pub updated: bool,
    /// This is the first build the page can run.
    pub first_success: bool,
    /// Wall time of the attempt, in milliseconds.
    pub elapsed_ms: f64,
    /// Colored diagnostic. Empty when `success` is set.
    pub error: String,
}

/// What `nlayer run --target web` serves.
pub struct RunOptions {
    /// An entry module, or a directory whose `App.tsx` is the entry.
    pub entry: PathBuf,
    /// `0` asks the OS for a port.
    pub port: u16,
    /// Minify the bundle.
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

struct Inner {
    generation: u64,
    error: Option<String>,
    /// Served bodies, keyed without a leading slash. `nlayer.wasm` stays
    /// across a failed rebuild; everything else is the last good app.
    files: HashMap<String, Arc<[u8]>>,
    watched: Vec<PathBuf>,
    stamp: u64,
}

struct Shared {
    stop: AtomicBool,
    port: u16,
    entry: PathBuf,
    app_dir: PathBuf,
    release: bool,
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

    /// How many times the served app has changed. `0` means no good build yet.
    pub fn generation(&self) -> u64 {
        self.shared.inner.lock().expect("dev server").generation
    }

    /// The plain text of the latest failed build, if it is still the latest.
    pub fn last_error(&self) -> Option<String> {
        self.shared.inner.lock().expect("dev server").error.clone()
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
        release: options.release,
        on_build,
        inner: Mutex::new(Inner {
            generation: 0,
            error: None,
            files,
            watched: Vec::new(),
            stamp: 0,
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
    let started = Instant::now();
    let built = bundle(&BundleOptions {
        entry: shared.entry.clone(),
        minify: shared.release,
        test: false,
    });
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let outcome = match built {
        Ok(bundled) => match read_app(&shared.app_dir, &bundled) {
            Ok(app) => publish(shared, app, &bundled, bundled.elapsed_ms),
            Err(message) => {
                fail(shared, message.clone());
                BuildOutcome {
                    port: shared.port,
                    success: false,
                    updated: false,
                    first_success: false,
                    elapsed_ms,
                    error: message,
                }
            }
        },
        Err(error) => {
            fail(shared, error.plain);
            BuildOutcome {
                port: shared.port,
                success: false,
                updated: false,
                first_success: false,
                elapsed_ms,
                error: error.colored,
            }
        }
    };
    (shared.on_build)(outcome);
}

fn publish(
    shared: &Shared,
    app: HashMap<String, Vec<u8>>,
    bundled: &Bundle,
    elapsed_ms: f64,
) -> BuildOutcome {
    let watched = watch_paths(&shared.app_dir, Some(bundled));
    let stamp = stamp(&watched);
    let mut inner = shared.inner.lock().expect("dev server");
    let was_zero = inner.generation == 0;
    let updated = install_app(&mut inner.files, app);
    if updated {
        inner.generation += 1;
    }
    inner.error = None;
    inner.watched = watched;
    inner.stamp = stamp;
    let first_success = was_zero && inner.generation > 0;
    BuildOutcome {
        port: shared.port,
        success: true,
        updated,
        first_success,
        elapsed_ms,
        error: String::new(),
    }
}

fn fail(shared: &Shared, plain: String) {
    let watched = watch_paths(&shared.app_dir, None);
    let stamp = stamp(&watched);
    let mut inner = shared.inner.lock().expect("dev server");
    inner.error = Some(plain);
    inner.watched = watched;
    inner.stamp = stamp;
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

fn read_app(app_dir: &Path, bundled: &Bundle) -> Result<HashMap<String, Vec<u8>>, String> {
    let mut files = HashMap::new();
    let mut code = bundled.code.clone();
    if !code.ends_with('\n') {
        code.push('\n');
    }
    code.push_str("//# sourceMappingURL=bundle.js.map\n");
    files.insert("bundle.js".to_owned(), code.into_bytes());
    files.insert(
        "bundle.js.map".to_owned(),
        bundled.source_map.clone().into_bytes(),
    );
    for rel in &bundled.assets {
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
    path == "index.html"
        || path == "boot.js"
        || path == "nlayer.js"
        || path == "host.js"
        || path == "base.css"
        || path == "nlayer.wasm"
        || path == "bundle.js"
        || path == "bundle.js.map"
        || path == "browser/runtime.js"
        || path == "browser/values.js"
        || path.starts_with("__nlayer/")
}

pub(crate) fn watch_paths(app_dir: &Path, bundled: Option<&Bundle>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    collect_sources(app_dir, &mut paths);
    if let Some(bundled) = bundled {
        paths.extend(bundled.watch_files.iter().cloned());
        for rel in &bundled.assets {
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
            Ok((socket, _)) => handle(socket, &shared),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

fn handle(mut socket: TcpStream, shared: &Shared) {
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
        Ok(path) => match lookup(shared, &path) {
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
    if path == "__nlayer/generation" {
        let generation = shared.inner.lock().expect("dev server").generation;
        return Ok((
            "text/plain; charset=utf-8",
            Body::Bytes(generation.to_string().into_bytes()),
        ));
    }
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
    let (mime, text) = match path {
        "" | "index.html" => ("text/html; charset=utf-8", dev_page()),
        "boot.js" => ("text/javascript; charset=utf-8", BOOT_JS),
        "nlayer.js" => ("text/javascript; charset=utf-8", MOUNTER_JS),
        "host.js" => ("text/javascript; charset=utf-8", HOST_JS),
        "base.css" => ("text/css; charset=utf-8", BASE_CSS),
        "browser/runtime.js" => ("text/javascript; charset=utf-8", RUNTIME_JS),
        "browser/values.js" => ("text/javascript; charset=utf-8", VALUES_JS),
        _ => return None,
    };
    Some((mime, text))
}

fn dev_page() -> &'static str {
    static PAGE: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| match PAGE_HTML.rsplit_once("</body>") {
            Some((head, tail)) => format!("{head}{RELOAD}</body>{tail}"),
            None => format!("{PAGE_HTML}{RELOAD}"),
        });
    PAGE.as_str()
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

#[cfg(test)]
mod tests {
    use super::route;

    #[test]
    fn routes_reject_dot_dot_after_decoding() {
        assert_eq!(route("/boot.js").unwrap(), "boot.js");
        assert_eq!(route("/img/icon.png?x=1").unwrap(), "img/icon.png");
        assert!(route("/../Cargo.toml").is_err());
        assert!(route("/%2e%2e/Cargo.toml").is_err());
        assert!(route("/%").is_err());
    }
}
