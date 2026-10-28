//! Loads `app.js` from the directory of this executable and runs it.
//!
//! `nlayer run` writes that file into the simulator app. On any other
//! target this exits: UIKit is not there.

fn main() {
    #[cfg(target_os = "ios")]
    {
        let dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
        let path = dir.unwrap_or_default().join("app.js");
        match std::fs::read_to_string(&path) {
            Ok(bundle) => nlayer_platform_ios::run_app(bundle),
            Err(err) => {
                eprintln!("cannot read {}: {err}", path.display());
                nlayer_platform_ios::run_app(String::new())
            }
        }
    }
    #[cfg(not(target_os = "ios"))]
    {
        eprintln!("nlayer-ios-app is the iOS runner; build it for an iOS target");
        std::process::exit(1);
    }
}
