//! The page-owned script: no QuickJS in the wasm module.

use std::process::Command;

#[test]
fn the_browser_host_mounts_without_quickjs() {
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "-p",
            "nlayer-platform-web",
            "--target",
            "wasm32-unknown-unknown",
            "--offline",
        ])
        .status()
        .expect("cargo builds the browser wasm");
    assert!(status.success(), "browser wasm build failed");
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/web/browser/host.test.mjs");
    let status = Command::new("node")
        .args(["--test", script])
        .status()
        .expect("node runs the browser host test");
    assert!(status.success(), "node --test {script}");
}
