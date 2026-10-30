//! `nlayer test --target android`.
//!
//! The test file still runs in QuickJS. `open` writes the app into the
//! device's private files and listens on a socket. `adb reverse` lets the
//! app connect, including the loopback file server its `fetch` uses. It
//! answers one JSON line per step. Press, type and scroll go through the
//! view system.

use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use nlayer_bundler::{BundleOptions, bundle};
use nlayer_testing::{AppHost, HostInsets, HostOpen, HostedApp};
use serde_json::{Value, json};

use crate::android::{self, PACKAGE};

/// The emulator or device one `nlayer test --target android` run drives.
pub(crate) struct AndroidHost {
    boot: Option<Boot>,
}

struct Boot {
    sdk: android::Sdk,
    serial: String,
}

impl AndroidHost {
    pub(crate) fn new() -> Self {
        Self { boot: None }
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
            entry,
            minify: false,
            test: false,
        })
        .map_err(|error| error.colored)?;
        bundled.code = nlayer_testing::with_fixture_origin(&bundled.code, &app_dir);
        let boot = self.boot()?;
        let sdk = &boot.sdk;
        let serial = boot.serial.clone();
        android::publish(sdk, &serial, &app_dir, &bundled).map_err(|error| error.to_string())?;
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let pin = json!({
            "width": request.width,
            "height": request.height,
            "keyboard": request.keyboard.unwrap_or(0.0),
            "safeArea": area(request.safe_area),
            "host": "127.0.0.1",
            "port": port,
            "colorScheme": request.color_scheme.map(|scheme| scheme.name()),
        });
        android::push_bytes(
            sdk,
            &serial,
            "files/nlayer-test.json",
            pin.to_string().as_bytes(),
        )
        .map_err(|error| error.to_string())?;
        let files = nlayer_testing::fixture_file_port(&app_dir);
        let files_forwarded = format!("tcp:{files}");
        android::adb(
            sdk,
            &serial,
            &["reverse", &files_forwarded, &files_forwarded],
        )
        .map_err(|error| error.to_string())?;
        let forwarded = format!("tcp:{port}");
        android::adb(sdk, &serial, &["reverse", &forwarded, &forwarded])
            .map_err(|error| error.to_string())?;
        android::relaunch(sdk, &serial).map_err(|error| error.to_string())?;
        let stream = accept(&listener)?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        let value = answer(&line)?;
        if value.as_str() != Some("ready") {
            return Err(format!("the device said {value}"));
        }
        Ok(Box::new(AndroidApp {
            writer: stream,
            reader,
            sdk_root: sdk.adb(),
            serial,
            port,
            files,
            open: true,
        }))
    }
}

struct AndroidApp {
    writer: std::net::TcpStream,
    reader: BufReader<std::net::TcpStream>,
    /// `adb` binary. Kept so close can drop the reverse without the SDK struct.
    sdk_root: std::path::PathBuf,
    serial: String,
    port: u16,
    files: u16,
    open: bool,
}

impl AndroidApp {
    fn call(
        &mut self,
        op: &str,
        selector: &str,
        text: &str,
        distance: f32,
    ) -> Result<Value, String> {
        if !self.open {
            return Err("the app is closed".into());
        }
        let line = json!({
            "op": op,
            "selector": selector,
            "text": text,
            "distance": distance,
        });
        writeln!(self.writer, "{line}").map_err(|error| error.to_string())?;
        self.writer.flush().map_err(|error| error.to_string())?;
        let mut response = String::new();
        self.reader
            .read_line(&mut response)
            .map_err(|error| error.to_string())?;
        if response.is_empty() {
            return Err("the Android app stopped".into());
        }
        answer(&response)
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
                _ => return Err(format!("the device returned {value}")),
            }
            if Instant::now() >= deadline {
                return Err(format!("`{selector}` matches nothing"));
            }
            thread::sleep(Duration::from_millis(30));
        }
    }

    fn tree(&mut self) -> Result<Value, String> {
        self.call("tree", "", "", 0.0)
    }

    fn close(&mut self) {
        if !self.open {
            return;
        }
        self.open = false;
        let _ = std::process::Command::new(&self.sdk_root)
            .args(["-s", &self.serial, "shell", "am", "force-stop", PACKAGE])
            .status();
        let forwarded = format!("tcp:{}", self.port);
        let files = format!("tcp:{}", self.files);
        let _ = std::process::Command::new(&self.sdk_root)
            .args(["-s", &self.serial, "reverse", "--remove", &forwarded])
            .status();
        let _ = std::process::Command::new(&self.sdk_root)
            .args(["-s", &self.serial, "reverse", "--remove", &files])
            .status();
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

fn accept(listener: &TcpListener) -> Result<std::net::TcpStream, String> {
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .map_err(|error| error.to_string())?;
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error.to_string()),
        }
        if Instant::now() >= deadline {
            return Err("the device did not connect".into());
        }
        thread::sleep(Duration::from_millis(30));
    }
}

fn answer(line: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(line.trim()).map_err(|error| error.to_string())?;
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(value.get("value").cloned().unwrap_or(Value::Null))
    } else {
        Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("the step failed")
            .to_owned())
    }
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
