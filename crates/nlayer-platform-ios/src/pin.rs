//! `Documents/nlayer-test.json`, written by `nlayer test --target ios`
//! before launch. Absent for `nlayer run`.

use std::path::Path;

use serde::Deserialize;

pub(crate) const FILE: &str = "nlayer-test.json";

/// Viewport, insets and the socket the test runner is listening on.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Pin {
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub keyboard: f32,
    #[serde(default, rename = "safeArea")]
    pub safe_area: Area,
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
