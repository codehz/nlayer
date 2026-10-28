//! Loads `app.js` and runs it.
//!
//! `nlayer run` writes that file into the simulator app's data container
//! (`Documents/app.js`), so a rebuild replaces the bundle without
//! reinstalling. A file next to this executable still runs. On any other
//! target this exits: UIKit is not there.

fn main() {
    init_log();
    #[cfg(target_os = "ios")]
    {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
        let paths = nlayer_platform_ios::bundle_paths(home.as_deref(), exe_dir.as_deref());
        for path in &paths {
            match std::fs::read_to_string(path) {
                Ok(bundle) => {
                    let assets = path
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_path_buf();
                    nlayer_platform_ios::run_app(bundle, assets);
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    eprintln!("cannot read {}: {err}", path.display());
                    let assets = path
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_path_buf();
                    nlayer_platform_ios::run_app(String::new(), assets);
                }
            }
        }
        let tried = paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("cannot read app.js ({tried})");
        nlayer_platform_ios::run_app(String::new(), std::path::PathBuf::new());
    }
    #[cfg(not(target_os = "ios"))]
    {
        eprintln!("nlayer-ios-app is the iOS runner; build it for an iOS target");
        std::process::exit(1);
    }
}

struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, meta: &log::Metadata<'_>) -> bool {
        meta.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "{} {} {}",
                record.level().as_str().to_lowercase(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

fn init_log() {
    let _ = log::set_logger(&StderrLog);
    log::set_max_level(log::LevelFilter::Warn);
}
