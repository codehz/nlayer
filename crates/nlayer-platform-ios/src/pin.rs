//! `Documents/nlayer-test.json`, written by `nlayer test --target ios`
//! before launch, and `Documents/nlayer-dev.json`, written by `nlayer run`.

use std::path::Path;

use serde::Deserialize;

pub(crate) const FILE: &str = "nlayer-test.json";

/// Viewport, insets and the socket the test runner is listening on.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Pin {
    pub width: f32,
    pub height: f32,
    /// `None` follows the software keyboard. A number pins
    /// `env(keyboard-inset-height)` for this run.
    #[serde(default)]
    pub keyboard: Option<f32>,
    #[serde(default, rename = "safeArea")]
    pub safe_area: Area,
    /// `light` or `dark`, pinned by `nlayer test`; `None` follows the
    /// system.
    #[serde(default, rename = "colorScheme")]
    pub color_scheme: Option<String>,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
}

/// One `open`'s safe area, in points.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Area {
    #[serde(default)]
    pub top: f32,
    #[serde(default)]
    pub right: f32,
    #[serde(default)]
    pub bottom: f32,
    #[serde(default)]
    pub left: f32,
}

pub(crate) fn load(assets: &Path) -> Option<Pin> {
    let text = std::fs::read_to_string(assets.join(FILE)).ok()?;
    match serde_json::from_str(&text) {
        Ok(pin) => Some(pin),
        Err(error) => {
            eprintln!("nlayer-test.json: {error}");
            None
        }
    }
}

/// `Documents/nlayer-dev.json`. Separate from [`FILE`]: `run` follows the
/// window, and the file only names the dev server.
pub(crate) const DEV_FILE: &str = "nlayer-dev.json";

/// Where the running app reads patches from. Not a [`Pin`]: no viewport.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DevPin {
    pub host: String,
    pub port: u16,
    /// `true` for a test session. The run client does not connect.
    #[serde(default)]
    pub test: bool,
}

pub(crate) fn load_dev(assets: &Path) -> Option<DevPin> {
    let text = std::fs::read_to_string(assets.join(DEV_FILE)).ok()?;
    match serde_json::from_str(&text) {
        Ok(pin) => Some(pin),
        Err(error) => {
            eprintln!("nlayer-dev.json: {error}");
            None
        }
    }
}
