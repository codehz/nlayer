//! UIKit host for one nlayer surface (iOS and the iOS simulator).
//!
//! [`run_app`] builds a window and runs `bundle` in it. The engine thread
//! lays the app out; each [`nlayer_core::MountBatch`] is applied on the main
//! queue. Text is measured on the engine thread with the same CoreText lines
//! the views draw.
//!
//! Extracted from `nsh-platform-ios` (`app.rs`, `surface.rs`, `mount.rs`,
//! `text.rs`, `text_view.rs`, `decor.rs`, `image.rs`, `input.rs`) and cut to
//! views, text, images, one-axis scroll views, virtualized lists and inputs
//! (one line or several): frames,
//! fill, radius, opacity, overflow, a static transform, solid or dashed
//! uniform borders, per-side solid borders, `onPress`, the scroll events
//! (`onScroll` at most once per frame, drag and momentum; payload is the
//! content offset, content size and visible size), `activeStyle` /
//! `hoverStyle` / `focusStyle`, safe area, the software
//! keyboard's height (`env(keyboard-inset-height)`, applied at once, not
//! animated), PNG, JPEG or WebP from a bundled file or `http(s)`, and a field
//! with `value`, `onChange`, `placeholder`, `disabled`, `secureTextEntry` and
//! `keyboardType`. Refresh, zoom, paging and text selection are not mounted.

#![cfg_attr(target_os = "ios", deny(unsafe_op_in_unsafe_fn))]

use std::path::{Path, PathBuf};

#[cfg(target_os = "ios")]
mod app;
#[cfg(target_os = "ios")]
mod driver;
#[cfg(target_os = "ios")]
mod host;
mod image;
mod input;
mod keyboard;
#[cfg(target_os = "ios")]
mod mount;
#[cfg(target_os = "ios")]
mod pin;
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
mod scene;
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
mod scroll_extent;
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
mod scroll_frame;
#[cfg(target_os = "ios")]
mod synth;
#[cfg(target_os = "ios")]
mod text;
#[cfg(target_os = "ios")]
mod text_view;

/// Where the runner looks for `app.js`, in order.
///
/// `nlayer run` writes the first path (the simulator data container), so a
/// rebuild can replace the bundle without reinstalling the app. A file next
/// to the executable still runs when that path is absent.
pub fn bundle_paths(home: Option<&Path>, exe_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = home {
        paths.push(home.join("Documents/app.js"));
    }
    if let Some(dir) = exe_dir {
        paths.push(dir.join("app.js"));
    }
    paths
}

/// Runs `bundle` (an ES module) until the process exits.
///
/// `assets` is the directory bundled images are read from (`asset:/img/a.png`
/// is `<assets>/img/a.png`). An empty bundle shows the read error and does
/// not start the engine. Must be called on the main thread.
#[cfg(target_os = "ios")]
pub fn run_app(bundle: String, assets: PathBuf) -> ! {
    app::run_app(bundle, assets)
}

/// Runs the device-layer tests that need UIKit and the main thread.
///
/// `cargo test` runs each test on a worker thread, and UIKit cannot be
/// linked off iOS. The `host` test binary is a `harness = false` executable;
/// this runs there, on the process's main thread. `crates/nlayer-cli`'s
/// `ios_host_tests` builds it for the simulator and spawns it.
#[cfg(all(target_os = "ios", feature = "host-tests"))]
pub fn run_host_tests() -> std::process::ExitCode {
    let mut tests = Vec::new();
    tests.extend(host::tests::tests());
    tests.extend(driver::tests::tests());
    tests.extend(mount::tests::tests());
    tests.extend(text::tests::tests());
    println!("running {} tests", tests.len());
    let mut passed = 0;
    let mut failures = Vec::new();
    for (name, test) in tests {
        if std::panic::catch_unwind(test).is_ok() {
            println!("test {name} ... ok");
            passed += 1;
        } else {
            println!("test {name} ... FAILED");
            failures.push(name);
        }
    }
    println!(
        "\ntest result: {}. {passed} passed; {} failed; 0 ignored",
        if failures.is_empty() { "ok" } else { "FAILED" },
        failures.len()
    );
    if failures.is_empty() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

/// UIKit is not on this target.
#[cfg(not(target_os = "ios"))]
pub fn run_app(_bundle: String, _assets: PathBuf) -> ! {
    eprintln!("nlayer-platform-ios runs on iOS");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn the_data_container_is_read_before_the_app_bundle() {
        let paths = super::bundle_paths(Some(Path::new("/home")), Some(Path::new("/app")));
        assert_eq!(
            paths,
            [
                Path::new("/home/Documents/app.js").to_path_buf(),
                Path::new("/app/app.js").to_path_buf()
            ]
        );
        assert!(super::bundle_paths(None, None).is_empty());
    }
}
