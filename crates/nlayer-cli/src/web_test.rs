//! `nlayer test --target web`.
//!
//! The test file still runs in QuickJS. `open` serves the app and loads it
//! in a headless browser: the page executes the bundle, and press, type, and
//! scroll go through that document. Assertions read the mounted DOM.

use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use nlayer_testing::{AppHost, HostOpen, HostedApp};
use serde_json::{Value, json};

use crate::{RunOptions, serve};

/// A Chromium browser the web target keeps for one `nlayer test` run.
pub(crate) struct WebHost {
    wasm: Vec<u8>,
    browser: Option<Arc<Mutex<Browser>>>,
}

impl WebHost {
    pub(crate) fn new(wasm: Vec<u8>) -> Self {
        Self {
            wasm,
            browser: None,
        }
    }

    fn browser(&mut self) -> Result<Arc<Mutex<Browser>>, String> {
        if self.browser.is_none() {
            self.browser = Some(Arc::new(Mutex::new(Browser::launch()?)));
        }
        Ok(Arc::clone(self.browser.as_ref().expect("browser")))
    }
}

impl AppHost for WebHost {
    fn open(&mut self, request: HostOpen) -> Result<Box<dyn HostedApp>, String> {
        let browser = self.browser()?;
        let server = serve(RunOptions {
            entry: request.entry.clone(),
            port: 0,
            release: false,
            wasm: self.wasm.clone(),
            on_build: None,
        })
        .map_err(|error| error.to_string())?;
        if server.generation() == 0 {
            return Err(server
                .last_error()
                .unwrap_or_else(|| "the app failed to build".to_owned()));
        }
        let url = pinned_url(&server.url(), &request);
        let mut guard = browser.lock().map_err(|error| error.to_string())?;
        let page = guard.open_page(&url, request.width, request.height)?;
        drop(guard);
        if let Err(error) = wait_ready(&browser, &page.session_id) {
            if let Ok(mut guard) = browser.lock() {
                guard.close_page(&page);
            }
            return Err(error);
        }
        Ok(Box::new(WebApp {
            browser,
            page: Some(page),
            server: Some(server),
        }))
    }
}

struct WebApp {
    browser: Arc<Mutex<Browser>>,
    page: Option<Page>,
    server: Option<crate::DevServer>,
}

impl WebApp {
    fn eval(&mut self, expression: &str) -> Result<Value, String> {
        let session = self
            .page
            .as_ref()
            .ok_or_else(|| "the app is closed".to_owned())?
            .session_id
            .clone();
        let mut browser = self.browser.lock().map_err(|error| error.to_string())?;
        browser.eval(&session, expression)
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let mut expression = format!("globalThis.__nlayerHost.{method}(");
        for (index, arg) in args.iter().enumerate() {
            if index > 0 {
                expression.push(',');
            }
            expression.push_str(&arg.to_string());
        }
        expression.push(')');
        self.eval(&expression)
    }
}

impl HostedApp for WebApp {
    fn texts(&mut self) -> Result<Vec<String>, String> {
        strings(self.call("texts", &[])?)
    }

    fn inputs(&mut self) -> Result<Vec<String>, String> {
        strings(self.call("inputs", &[])?)
    }

    fn press(&mut self, selector: &str) -> Result<(), String> {
        self.call("press", &[json!(selector)])?;
        Ok(())
    }

    fn type_text(&mut self, selector: &str, text: &str) -> Result<(), String> {
        self.call("type", &[json!(selector), json!(text)])?;
        Ok(())
    }

    fn scroll(&mut self, selector: &str, distance: f32) -> Result<(), String> {
        self.call("scroll", &[json!(selector), json!(distance)])?;
        Ok(())
    }

    fn wait_for(&mut self, selector: &str) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let value = self.call("query", &[json!(selector)])?;
            match value.get("state").and_then(Value::as_str) {
                Some("ready") => return Ok(()),
                Some("wait") => {}
                Some("fail") => {
                    return Err(value
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("the step failed")
                        .to_owned());
                }
                _ => return Err(format!("the page returned {value}")),
            }
            if Instant::now() >= deadline {
                return Err(format!("`{selector}` matches nothing"));
            }
            thread::sleep(Duration::from_millis(16));
        }
    }

    fn tree(&mut self) -> Result<Value, String> {
        self.call("tree", &[])
    }

    fn close(&mut self) {
        if let Some(page) = self.page.take()
            && let Ok(mut browser) = self.browser.lock()
        {
            browser.close_page(&page);
        }
        self.server.take();
    }
}

impl Drop for WebApp {
    fn drop(&mut self) {
        self.close();
    }
}

fn strings(value: Value) -> Result<Vec<String>, String> {
    value
        .as_array()
        .ok_or_else(|| "the page returned no list".to_owned())?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "the page returned a non-string".to_owned())
        })
        .collect()
}

fn pinned_url(base: &str, request: &HostOpen) -> String {
    let area = request.safe_area.unwrap_or(nlayer_testing::HostInsets {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    });
    let keyboard = request.keyboard.unwrap_or(0.0);
    format!(
        "{base}?pin=1&w={}&h={}&sat={}&sar={}&sab={}&sal={}&kb={keyboard}",
        request.width, request.height, area.top, area.right, area.bottom, area.left,
    )
}

const READY: &str = "(() => ({ ready: globalThis.__nlayerReady === true, failed: globalThis.__nlayerFailed || \"\" }))()";

fn wait_ready(browser: &Mutex<Browser>, session: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let value = {
            let mut browser = browser.lock().map_err(|error| error.to_string())?;
            browser.eval(session, READY)?
        };
        if value.get("ready").and_then(Value::as_bool) == Some(true) {
            return Ok(());
        }
        if let Some(error) = value.get("failed").and_then(Value::as_str)
            && !error.is_empty()
            && error != "Loading…"
        {
            return Err(error.to_owned());
        }
        if Instant::now() >= deadline {
            return Err("the page did not start".to_owned());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

struct Page {
    target_id: String,
    session_id: String,
}

struct Browser {
    child: Child,
    socket: Socket,
    next_id: u64,
    dir: PathBuf,
}

impl Browser {
    fn launch() -> Result<Self, String> {
        let bin = browser_bin()?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("nlayer-browser-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let err = fs::File::create(dir.join("chrome.err")).map_err(|error| error.to_string())?;
        let mut child = Command::new(&bin)
            .args([
                "--headless=new",
                "--disable-gpu",
                "--no-first-run",
                "--no-default-browser-check",
                "--remote-debugging-address=127.0.0.1",
                "--remote-debugging-port=0",
                "--remote-allow-origins=*",
            ])
            .arg(format!("--user-data-dir={}", dir.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(err))
            .spawn()
            .map_err(|error| format!("{}: {error}", bin.display()))?;
        let port = wait_port(&dir, &mut child)?;
        let path = fs::read_to_string(dir.join("DevToolsActivePort"))
            .ok()
            .and_then(|text| text.lines().nth(1).map(str::to_owned))
            .unwrap_or_else(|| "/json".to_owned());
        let socket = Socket::connect(port, &path)?;
        Ok(Self {
            child,
            socket,
            next_id: 1,
            dir,
        })
    }

    fn open_page(&mut self, url: &str, width: f32, height: f32) -> Result<Page, String> {
        let created = self.call(None, "Target.createTarget", json!({ "url": "about:blank" }))?;
        let target_id = created
            .get("targetId")
            .and_then(Value::as_str)
            .ok_or("the browser returned no page")?
            .to_owned();
        let attached = self.call(
            None,
            "Target.attachToTarget",
            json!({ "targetId": target_id, "flatten": true }),
        )?;
        let session_id = attached
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or("the browser returned no session")?
            .to_owned();
        let page = Page {
            target_id,
            session_id,
        };
        self.call(
            Some(&page.session_id),
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": width.round().max(1.0) as u32,
                "height": height.round().max(1.0) as u32,
                "deviceScaleFactor": 1,
                "mobile": false,
            }),
        )?;
        self.call(Some(&page.session_id), "Page.enable", json!({}))?;
        let navigated = self.call(
            Some(&page.session_id),
            "Page.navigate",
            json!({ "url": url }),
        )?;
        if let Some(error) = navigated.get("errorText").and_then(Value::as_str) {
            return Err(format!("{error} ({url})"));
        }
        Ok(page)
    }

    fn close_page(&mut self, page: &Page) {
        let _ = self.call(
            None,
            "Target.closeTarget",
            json!({ "targetId": page.target_id }),
        );
    }

    fn eval(&mut self, session: &str, expression: &str) -> Result<Value, String> {
        let result = self.call(
            Some(session),
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true,
            }),
        )?;
        if let Some(details) = result.get("exceptionDetails") {
            let message = details
                .pointer("/exception/description")
                .and_then(Value::as_str)
                .or_else(|| details.get("text").and_then(Value::as_str))
                .unwrap_or("the page call failed");
            return Err(message.to_owned());
        }
        Ok(result
            .get("result")
            .and_then(|result| result.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    fn call(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let mut message = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        self.socket
            .send_text(&message.to_string())
            .map_err(|error| error.to_string())?;
        loop {
            let raw = self.socket.recv_text()?;
            let value: Value = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(error.to_string());
            }
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn browser_bin() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("NLAYER_BROWSER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "NLAYER_BROWSER {} is not a browser",
            path.display()
        ));
    }
    let candidates = [
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Arc.app/Contents/MacOS/Arc",
    ];
    for path in candidates {
        if Path::new(path).is_file() {
            return Ok(PathBuf::from(path));
        }
    }
    Err("no Chromium browser for `nlayer test --target web`. Set NLAYER_BROWSER.".into())
}

fn wait_port(dir: &Path, child: &mut Child) -> Result<u16, String> {
    let path = dir.join("DevToolsActivePort");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = fs::read_to_string(&path)
            && let Some(port) = text.lines().next().and_then(|line| line.parse().ok())
        {
            return Ok(port);
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            let err = fs::read_to_string(dir.join("chrome.err")).unwrap_or_default();
            return Err(format!("the browser exited ({status}): {err}"));
        }
        if Instant::now() >= deadline {
            return Err("the browser did not open a debugger port".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

struct Socket {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl Socket {
    fn connect(port: u16, path: &str) -> Result<Self, String> {
        let mut stream =
            TcpStream::connect(("127.0.0.1", port)).map_err(|error| error.to_string())?;
        stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(30))).ok();
        let path = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        };
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| error.to_string())?;
        let mut buf = Vec::new();
        let mut tmp = [0_u8; 1024];
        while !buf.windows(4).any(|window| window == b"\r\n\r\n") {
            let n = stream.read(&mut tmp).map_err(|error| error.to_string())?;
            if n == 0 {
                return Err("the browser closed the debugger".into());
            }
            buf.extend_from_slice(&tmp[..n]);
            if buf.len() > 65_536 {
                return Err("the debugger handshake was too large".into());
            }
        }
        let head = String::from_utf8_lossy(&buf);
        if !head.starts_with("HTTP/1.1 101") && !head.contains(" 101 ") {
            return Err(format!(
                "debugger handshake failed: {}",
                head.lines().next().unwrap_or("")
            ));
        }
        let split = buf
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        Ok(Self {
            stream,
            buf: buf.split_off(split),
        })
    }

    fn send_text(&mut self, text: &str) -> std::io::Result<()> {
        self.send_frame(0x1, text.as_bytes())
    }

    fn send_frame(&mut self, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
        let mask = [0x11, 0x22, 0x33, 0x44];
        let mut header = Vec::with_capacity(14);
        header.push(0x80 | opcode);
        let len = payload.len();
        if len < 126 {
            header.push(0x80 | len as u8);
        } else if len < 65_536 {
            header.push(0x80 | 126);
            header.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            header.push(0x80 | 127);
            header.extend_from_slice(&(len as u64).to_be_bytes());
        }
        header.extend_from_slice(&mask);
        self.stream.write_all(&header)?;
        let mut masked = payload.to_vec();
        for (index, byte) in masked.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
        self.stream.write_all(&masked)?;
        self.stream.flush()
    }

    fn recv_text(&mut self) -> Result<String, String> {
        let mut message = Vec::new();
        loop {
            let (opcode, fin, payload) = self.read_frame()?;
            match opcode {
                0x1 | 0x0 => {
                    message.extend(payload);
                    if fin {
                        return String::from_utf8(message).map_err(|error| error.to_string());
                    }
                }
                0x8 => return Err("the browser closed the debugger".into()),
                0x9 => self
                    .send_frame(0xA, &payload)
                    .map_err(|error| error.to_string())?,
                _ => {}
            }
        }
    }

    fn read_frame(&mut self) -> Result<(u8, bool, Vec<u8>), String> {
        let head = self.take(2)?;
        let fin = head[0] & 0x80 != 0;
        let opcode = head[0] & 0x0f;
        let masked = head[1] & 0x80 != 0;
        let mut len = (head[1] & 0x7f) as usize;
        if len == 126 {
            let ext = self.take(2)?;
            len = u16::from_be_bytes([ext[0], ext[1]]) as usize;
        } else if len == 127 {
            let ext = self.take(8)?;
            let value = u64::from_be_bytes(ext.try_into().unwrap());
            len = usize::try_from(value).map_err(|_| "debugger frame is too large".to_owned())?;
        }
        if len > 32 * 1024 * 1024 {
            return Err("debugger frame is too large".into());
        }
        let mask = if masked { Some(self.take(4)?) } else { None };
        let mut payload = self.take(len)?;
        if let Some(mask) = mask {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
        }
        Ok((opcode, fin, payload))
    }

    fn take(&mut self, n: usize) -> Result<Vec<u8>, String> {
        while self.buf.len() < n {
            let mut tmp = [0_u8; 8192];
            let read = self
                .stream
                .read(&mut tmp)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                return Err("the browser closed the debugger".into());
            }
            self.buf.extend_from_slice(&tmp[..read]);
        }
        Ok(self.buf.drain(..n).collect())
    }
}
