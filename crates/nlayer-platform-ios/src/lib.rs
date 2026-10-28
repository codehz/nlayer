//! UIKit host for one nlayer surface (iOS and the iOS simulator).
//!
//! [`run_app`] builds a window and runs `bundle` in it. The engine thread
//! lays the app out; each [`nlayer_core::MountBatch`] is applied on the main
//! queue. Text is measured on the engine thread with the same CoreText lines
//! the views draw.
//!
//! Extracted from `nsh-platform-ios` (`app.rs`, `surface.rs`, `mount.rs`,
//! `text.rs`, `text_view.rs`, `decor.rs`, `image.rs`, `input.rs`) and cut to
//! views, text, images, one-axis scroll views and single-line inputs: frames,
//! fill, radius, opacity, overflow, a static transform, solid or dashed
//! uniform borders, per-side solid borders, `onPress`, `activeStyle` /
//! `hoverStyle` / `focusStyle`, safe area, PNG, JPEG or WebP from a bundled
//! file or `http(s)`, and a field with `value`, `onChange`, `placeholder`,
//! `disabled`, `secureTextEntry` and `keyboardType`. Refresh, zoom, paging
//! and text selection are not mounted.

#![cfg_attr(target_os = "ios", deny(unsafe_op_in_unsafe_fn))]

use std::path::{Path, PathBuf};

#[cfg(target_os = "ios")]
mod app;
#[cfg(target_os = "ios")]
mod host;
mod image;
mod input;
#[cfg(target_os = "ios")]
mod mount;
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
