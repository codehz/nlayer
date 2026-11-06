//! `nlayer test --target ios`.
//!
//! The test file still runs in QuickJS. The first `open` writes the app into
//! the simulator and launches the host. Later opens send [`Recycle`] and a
//! new bundle on the same socket: the process stays, and the host starts a
//! new engine. A dead socket relaunches once.
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use nlayer_bundler::{BundleOptions, bundle};
use nlayer_testing::{AppHost, HostInsets, HostOpen, HostedApp, NodeId};
use serde_json::{Value, json};

use crate::ios::{self, BUNDLE_ID};
use crate::session::{RecyclePin, SharedSession};

/// The simulator host one `nlayer test --target ios` run drives.
pub(crate) struct IosHost {
    boot: Option<Boot>,
    /// The simulator `--device` named.
    device: Option<String>,
    /// The socket the running host is on. `None` until the first launch,
    /// and after that launch's socket dies.
    shared: Option<Arc<SharedSession>>,
}

struct Boot {
    udid: String,
}

impl Drop for IosHost {
    fn drop(&mut self) {
        self.shared.take();
        if let Some(boot) = &self.boot {
            let _ = ios::simctl(&["terminate", &boot.udid, BUNDLE_ID]);
        }
    }
}
impl IosHost {
    pub(crate) fn new(device: Option<String>) -> Self {
        Self {
            boot: None,
            device,
            shared: None,
        }
    }

    fn boot(&mut self) -> Result<&Boot, String> {
        if self.boot.is_none() {
            let host = ios::ios_host().map_err(|error| error.to_string())?;
            let (udid, name) =
                ios::simulator(self.device.as_deref()).map_err(|error| error.to_string())?;
            let app = ios::package(&host).map_err(|error| error.to_string())?;
            if ios::host_changed(&udid, &host).map_err(|error| error.to_string())? {
                eprintln!("installing {BUNDLE_ID}");
                let path = app.to_string_lossy().into_owned();
                ios::simctl(&["install", &udid, &path]).map_err(|error| error.to_string())?;
                ios::record_install(&udid, &host).map_err(|error| error.to_string())?;
            }
            eprintln!("ios tests on {name}");
            self.boot = Some(Boot { udid });
        }
        Ok(self.boot.as_ref().expect("booted"))
    }
}

impl AppHost for IosHost {
    fn os(&self) -> &'static str {
        "ios"
    }

    fn open(&mut self, request: HostOpen) -> Result<Box<dyn HostedApp>, String> {
        let entry = crate::resolve_entry(&request.entry).map_err(|error| error.to_string())?;
        let app_dir = entry
            .parent()
            .ok_or_else(|| "entry has no parent directory".to_owned())?
            .to_path_buf();
        let bundled = bundle(&BundleOptions {
            entry: entry.clone(),
            minify: false,
            test: false,
        })
        .map_err(|error| error.colored)?;
        let udid = self.boot()?.udid.clone();
        let docs = ios::data_container(&udid)
            .map_err(|error| error.to_string())?
            .join("Documents");
        // The test socket pushes the development bundle once, then again
        // after each recycle. `app.js` stays empty so the host does not
        // mount an ESM tree and then a second one.
        ios::publish(&docs, &app_dir, b"", &bundled.assets).map_err(|error| error.to_string())?;
        let pin = recycle_pin(&request);
        if let Some(shared) = self.shared.clone()
            && shared.alive()
        {
            let generation = shared.invalidate();
            match shared
                .recycle(&pin)
                .and_then(|_| shared.push_entry(&entry, true))
            {
                Ok(()) => {
                    return Ok(Box::new(IosApp {
                        shared,
                        generation,
                        open: true,
                        pin,
                    }));
                }
                Err(_) => shared.kill(),
            }
        }
        self.shared = None;
        let session = crate::TestSession::start(&entry, true)?;
        let port = session.port();
        let pin_file = json!({
            "width": pin.width,
            "height": pin.height,
            "keyboard": pin.keyboard,
            "safeArea": area(request.safe_area),
            "host": "127.0.0.1",
            "port": port,
            "test": true,
            "colorScheme": pin.color_scheme,
        });
        std::fs::write(docs.join("nlayer-test.json"), pin_file.to_string())
            .map_err(|error| error.to_string())?;
        ios::relaunch(&udid).map_err(|error| error.to_string())?;
        let shared = SharedSession::new(session);
        self.shared = Some(Arc::clone(&shared));
        Ok(Box::new(IosApp {
            shared,
            generation: 1,
            open: true,
            pin,
        }))
    }
}

struct IosApp {
    shared: Arc<SharedSession>,
    generation: u64,
    open: bool,
    pin: RecyclePin,
}

impl IosApp {
    fn call(
        &mut self,
        op: &str,
        selector: &str,
        text: &str,
        distance: f32,
    ) -> Result<Value, String> {
        self.call_ex(op, selector, text, distance, 0.0, 0.0, 0.0)
    }

    #[allow(clippy::too_many_arguments)]
    fn call_ex(
        &mut self,
        op: &str,
        selector: &str,
        text: &str,
        distance: f32,
        x: f32,
        y: f32,
        ms: f64,
    ) -> Result<Value, String> {
        if !self.open || !self.shared.current(self.generation) {
            return Err("the app is closed".into());
        }
        let op = crate::session::step_op(op)?;
        match self.shared.step(op, selector, text, distance, x, y, ms) {
            Err(error) if error == "the app stopped" => {
                self.shared.kill();
                Err("the simulator app stopped".into())
            }
            Err(error) if error == "the app did not connect" => {
                self.shared.kill();
                Err("the simulator did not connect".into())
            }
            other => other,
        }
    }
}

impl HostedApp for IosApp {
    fn texts(&mut self) -> Result<Vec<String>, String> {
        strings(self.call("texts", "", "", 0.0)?)
    }

    fn inputs(&mut self) -> Result<Vec<String>, String> {
        strings(self.call("inputs", "", "", 0.0)?)
    }

    fn press(&mut self, selector: &str) -> Result<(), String> {
        self.call("press", selector, "", 0.0)?;
        Ok(())
    }

    fn hover(&mut self, selector: &str) -> Result<(), String> {
        self.call("hover", selector, "", 0.0)?;
        Ok(())
    }

    fn long_press(&mut self, selector: &str) -> Result<(), String> {
        self.call("longPress", selector, "", 0.0)?;
        Ok(())
    }

    fn type_text(&mut self, selector: &str, text: &str) -> Result<(), String> {
        self.call("type", selector, text, 0.0)?;
        Ok(())
    }

    fn scroll(&mut self, selector: &str, distance: f32) -> Result<(), String> {
        self.call("scroll", selector, "", distance)?;
        Ok(())
    }

    fn wait_for(&mut self, selector: &str) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let value = self.call("query", selector, "", 0.0)?;
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
                _ => return Err(format!("the simulator returned {value}")),
            }
            if Instant::now() >= deadline {
                return Err(format!("`{selector}` matches nothing"));
            }
            thread::sleep(Duration::from_millis(16));
        }
    }

    fn tree(&mut self) -> Result<Value, String> {
        self.call("tree", "", "", 0.0)
    }

    fn selection_owner(&mut self, selector: &str) -> Result<Option<NodeId>, String> {
        match self.call("selectionOwner", selector, "", 0.0)? {
            Value::Null => Ok(None),
            Value::Number(number) => number
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .map(Some)
                .ok_or_else(|| format!("the simulator returned {number}")),
            other => Err(format!("the simulator returned {other}")),
        }
    }

    fn opened_urls(&mut self) -> Result<Vec<String>, String> {
        strings(self.call("openedUrls", "", "", 0.0)?)
    }

    fn drag(&mut self, selector: &str, x: f32, y: f32) -> Result<(), String> {
        self.call_ex("drag", selector, "", 0.0, x, y, 0.0)
            .map(|_| ())
    }

    fn drag_and_hold(&mut self, selector: &str, x: f32, y: f32) -> Result<(), String> {
        self.call_ex("dragAndHold", selector, "", 0.0, x, y, 0.0)
            .map(|_| ())
    }

    fn drop(&mut self) -> Result<(), String> {
        self.call("drop", "", "", 0.0).map(|_| ())
    }
    fn back(&mut self) -> Result<(), String> {
        self.call("back", "", "", 0.0).map(|_| ())
    }

    fn advance(&mut self, ms: f64) -> Result<(), String> {
        self.call_ex("advance", "", "", 0.0, 0.0, 0.0, ms)
            .map(|_| ())
    }

    fn focus(&mut self, selector: &str) -> Result<(), String> {
        self.call("focus", selector, "", 0.0)?;
        Ok(())
    }

    fn down(&mut self, selector: &str) -> Result<(), String> {
        self.call("down", selector, "", 0.0)?;
        Ok(())
    }

    fn up(&mut self) -> Result<(), String> {
        self.call("up", "", "", 0.0)?;
        Ok(())
    }

    fn warnings(&mut self) -> Result<Vec<String>, String> {
        strings(self.call("warnings", "", "", 0.0)?)
    }

    fn select(&mut self, selector: &str, start: u32, end: u32) -> Result<(), String> {
        self.call("select", selector, &format!("{start},{end}"), 0.0)?;
        Ok(())
    }

    fn compose(&mut self, selector: &str, text: &str, composing: bool) -> Result<(), String> {
        self.call_ex(
            "compose",
            selector,
            text,
            0.0,
            if composing { 1.0 } else { 0.0 },
            0.0,
            0.0,
        )?;
        Ok(())
    }

    fn close(&mut self) {
        if !self.open {
            return;
        }
        self.open = false;
        if !self.shared.current(self.generation) {
            return;
        }
        if self.shared.recycle(&self.pin).is_err() {
            self.shared.kill();
        }
    }
}

impl Drop for IosApp {
    fn drop(&mut self) {
        self.close();
    }
}

fn area(insets: Option<HostInsets>) -> Value {
    let insets = insets.unwrap_or(HostInsets {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    });
    json!({
        "top": insets.top,
        "right": insets.right,
        "bottom": insets.bottom,
        "left": insets.left,
    })
}

fn strings(value: Value) -> Result<Vec<String>, String> {
    value
        .as_array()
        .ok_or_else(|| "the simulator returned no list".to_owned())?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "the simulator returned a non-string".to_owned())
        })
        .collect()
}

fn recycle_pin(request: &HostOpen) -> RecyclePin {
    let insets = request.safe_area.unwrap_or(HostInsets {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    });
    RecyclePin {
        width: request.width,
        height: request.height,
        keyboard: request.keyboard.unwrap_or(0.0),
        top: insets.top,
        right: insets.right,
        bottom: insets.bottom,
        left: insets.left,
        color_scheme: request.color_scheme.map(|scheme| scheme.name().to_owned()),
    }
}
