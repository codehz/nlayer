//! `nlayer test --target android`.
//!
//! The test file still runs in QuickJS. The first `open` writes the app into
//! the device and launches it. Later opens send [`Recycle`] and a new bundle
//! on the same socket: the process stays, and the host starts a new engine.
//! A dead socket relaunches once.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use nlayer_bundler::{BundleOptions, bundle};
use nlayer_testing::{AppHost, HostInsets, HostOpen, HostedApp, NodeId};
use serde_json::{Value, json};

use crate::android::{self, PACKAGE};
use crate::session::{RecyclePin, SharedSession};

/// The emulator or device one `nlayer test --target android` run drives.
pub(crate) struct AndroidHost {
    boot: Option<Boot>,
    /// The socket the running host is on. `None` until the first launch,
    /// and after that launch's socket dies.
    shared: Option<Arc<SharedSession>>,
    /// `adb reverse` bindings for the live process.
    forward: Option<Forward>,
}

struct Boot {
    sdk: android::Sdk,
    serial: String,
}

struct Forward {
    port: u16,
    files: u16,
}

impl AndroidHost {
    pub(crate) fn new() -> Self {
        Self {
            boot: None,
            shared: None,
            forward: None,
        }
    }

    fn boot(&mut self) -> Result<&Boot, String> {
        if self.boot.is_none() {
            let sdk = android::Sdk::find().map_err(|error| error.to_string())?;
            let serial = android::device(&sdk).map_err(|error| error.to_string())?;
            let abi = android::device_abi(&sdk, &serial).map_err(|error| error.to_string())?;
            let apk = android::host_apk(&sdk, &abi, false).map_err(|error| error.to_string())?;
            android::install_if_needed(&sdk, &serial, &apk).map_err(|error| error.to_string())?;
            eprintln!("android tests on {serial}");
            self.boot = Some(Boot { sdk, serial });
        }
        Ok(self.boot.as_ref().expect("booted"))
    }
}

impl Drop for AndroidHost {
    fn drop(&mut self) {
        self.shared.take();
        let Some(boot) = &self.boot else {
            return;
        };
        let _ = std::process::Command::new(boot.sdk.adb())
            .args(["-s", &boot.serial, "shell", "am", "force-stop", PACKAGE])
            .status();
        if let Some(forward) = self.forward.take() {
            let _ = remove_reverse(&boot.sdk, &boot.serial, forward.port);
            if forward.files != forward.port {
                let _ = remove_reverse(&boot.sdk, &boot.serial, forward.files);
            }
        }
    }
}

impl AppHost for AndroidHost {
    fn os(&self) -> &'static str {
        "android"
    }

    fn open(&mut self, request: HostOpen) -> Result<Box<dyn HostedApp>, String> {
        let entry = crate::resolve_entry(&request.entry).map_err(|error| error.to_string())?;
        let app_dir = entry
            .parent()
            .ok_or_else(|| "entry has no parent directory".to_owned())?
            .to_path_buf();
        let mut bundled = bundle(&BundleOptions {
            entry: entry.clone(),
            minify: false,
            test: false,
        })
        .map_err(|error| error.colored)?;
        bundled.code = nlayer_testing::with_fixture_origin(&bundled.code, &app_dir);
        {
            let boot = self.boot()?;
            let serial = boot.serial.clone();
            android::publish(&boot.sdk, &serial, &app_dir, &bundled)
                .map_err(|error| error.to_string())?;
            // The socket's development bundle is the only script. An ESM
            // `app.js` would mount a second tree beside it.
            android::push_bytes(&boot.sdk, &serial, "files/app.js", b"")
                .map_err(|error| error.to_string())?;
        }
        let pin = recycle_pin(&request);
        let files = nlayer_testing::fixture_file_port(&app_dir);
        if let Some(shared) = self.shared.clone()
            && shared.alive()
        {
            if self
                .forward
                .as_ref()
                .is_some_and(|forward| forward.files != files)
            {
                {
                    let boot = self.boot()?;
                    bind_reverse(&boot.sdk, &boot.serial, files)?;
                }
                if let Some(forward) = self.forward.as_mut() {
                    forward.files = files;
                }
            }
            let generation = shared.invalidate();
            match shared
                .recycle(&pin)
                .and_then(|_| shared.push_entry(&entry, true))
            {
                Ok(()) => {
                    return Ok(Box::new(AndroidApp {
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
        let previous = self.forward.take();
        {
            let boot = self.boot()?;
            let sdk = &boot.sdk;
            let serial = boot.serial.clone();
            android::push_bytes(
                sdk,
                &serial,
                "files/nlayer-test.json",
                pin_file.to_string().as_bytes(),
            )
            .map_err(|error| error.to_string())?;
            if let Some(forward) = previous {
                let _ = remove_reverse(sdk, &serial, forward.port);
                if forward.files != forward.port {
                    let _ = remove_reverse(sdk, &serial, forward.files);
                }
            }
            bind_reverse(sdk, &serial, files)?;
            bind_reverse(sdk, &serial, port)?;
            android::relaunch(sdk, &serial).map_err(|error| error.to_string())?;
        }
        self.forward = Some(Forward { port, files });
        let shared = SharedSession::new(session);
        self.shared = Some(Arc::clone(&shared));
        Ok(Box::new(AndroidApp {
            shared,
            generation: 1,
            open: true,
            pin,
        }))
    }
}

struct AndroidApp {
    shared: Arc<SharedSession>,
    generation: u64,
    open: bool,
    pin: RecyclePin,
}

impl AndroidApp {
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
                Err("the Android app stopped".into())
            }
            Err(error) if error == "the app did not connect" => {
                self.shared.kill();
                Err("the device did not connect".into())
            }
            other => other,
        }
    }
}

impl HostedApp for AndroidApp {
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
        let deadline = Instant::now() + Duration::from_secs(5);
        // A draw can hold the UI thread for this whole budget. The clock is
        // real time, so the promise has settled by the time the thread is
        // free. Take one more look before failing.
        let mut trailed = false;
        loop {
            self.advance(50.0)?;
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
                _ => return Err(format!("the device returned {value}")),
            }
            if Instant::now() < deadline {
                continue;
            }
            if trailed {
                return Err(format!("`{selector}` matches nothing"));
            }
            trailed = true;
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
                .ok_or_else(|| format!("the device returned {number}")),
            other => Err(format!("the device returned {other}")),
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

impl Drop for AndroidApp {
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
        .ok_or_else(|| "the device returned no list".to_owned())?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "the device returned a non-string".to_owned())
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

fn bind_reverse(sdk: &android::Sdk, serial: &str, port: u16) -> Result<(), String> {
    let spec = format!("tcp:{port}");
    let _ = android::adb(sdk, serial, &["reverse", "--remove", &spec]);
    android::adb(sdk, serial, &["reverse", &spec, &spec])
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn remove_reverse(sdk: &android::Sdk, serial: &str, port: u16) -> Result<(), String> {
    let spec = format!("tcp:{port}");
    android::adb(sdk, serial, &["reverse", "--remove", &spec])
        .map(|_| ())
        .map_err(|error| error.to_string())
}
