//! Static web packaging using the same browser host as `nlayer run`.

use std::{collections::BTreeMap, error::Error, fs, path::Path};

use nlayer_bundler::{BundleOptions, bundle};
use sha2::{Digest, Sha256};

use crate::{resolve_entry, web_module};

const FILES: &[(&str, &str)] = &[
    (
        "boot.js",
        include_str!("../../nlayer-platform-web/web/boot.js"),
    ),
    (
        "nlayer.js",
        include_str!("../../nlayer-platform-web/web/nlayer.js"),
    ),
    (
        "host.js",
        include_str!("../../nlayer-platform-web/web/host.js"),
    ),
    (
        "base.css",
        include_str!("../../nlayer-platform-web/web/base.css"),
    ),
    (
        "browser/runtime.js",
        include_str!("../../nlayer-platform-web/web/browser/runtime.js"),
    ),
    (
        "browser/list.js",
        include_str!("../../nlayer-platform-web/web/browser/list.js"),
    ),
    (
        "browser/values.js",
        include_str!("../../nlayer-platform-web/web/browser/values.js"),
    ),
    (
        "browser/storage.js",
        include_str!("../../nlayer-platform-web/web/browser/storage.js"),
    ),
    (
        "browser/animation.js",
        include_str!("../../nlayer-platform-web/web/browser/animation.js"),
    ),
];

/// Writes a self-contained static site for an app.
pub fn package_web(entry: &Path, out: &Path) -> Result<(), Box<dyn Error>> {
    let entry = resolve_entry(entry)?;
    let root = entry.parent().ok_or("entry has no parent directory")?;
    let config = root.join("nlayer.json");
    let app_id = if config.exists() {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(config)?)?;
        value
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::to_owned)
    } else {
        None
    };
    let bundled = bundle(&BundleOptions {
        entry: entry.clone(),
        minify: true,
        test: false,
    })?;
    let wasm = web_module()?;
    fs::create_dir_all(out.join("browser"))?;
    for (path, content) in FILES {
        fs::write(out.join(path), content)?;
    }
    fs::write(out.join("nlayer.wasm"), wasm)?;
    fs::write(
        out.join("bundle.js"),
        format!("{}\n//# sourceMappingURL=bundle.js.map\n", bundled.code),
    )?;
    fs::write(out.join("bundle.js.map"), bundled.source_map)?;
    let mut assets = BTreeMap::new();
    for path in &bundled.assets {
        let data = fs::read(root.join(path))?;
        let digest = Sha256::digest(&data);
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("asset has no extension")?;
        let name = format!("assets/{digest:x}.{ext}");
        fs::create_dir_all(out.join("assets"))?;
        fs::write(out.join(&name), data)?;
        assets.insert(path.to_string_lossy().replace('\\', "/"), name);
    }
    let asset_json = serde_json::to_string(&assets)?.replace('<', "\\u003c");
    let id_json = serde_json::to_string(&app_id)?.replace('<', "\\u003c");
    let page = include_str!("../../nlayer-platform-web/web/index.html").replace(
        "assetBase: \"./\",",
        &format!(
            "assetBase: \"./\",\n          assetMap: {asset_json},\n          appId: {id_json},"
        ),
    );
    fs::write(out.join("index.html"), page)?;
    Ok(())
}
