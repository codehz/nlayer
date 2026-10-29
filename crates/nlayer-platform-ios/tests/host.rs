//! The device-layer tests, on the process's main thread.
//!
//! `cargo test` runs test bodies on worker threads; UIKit, CoreText views
//! and the main queue need the real main thread. This binary has a plain
//! `main`, so every test in `run_host_tests` runs where the app would run.
//! `crates/nlayer-cli` builds it for `aarch64-apple-ios-sim` and spawns it.

#[cfg(all(target_os = "ios", feature = "host-tests"))]
fn main() -> std::process::ExitCode {
    nlayer_platform_ios::run_host_tests()
}

#[cfg(not(all(target_os = "ios", feature = "host-tests")))]
fn main() {}
