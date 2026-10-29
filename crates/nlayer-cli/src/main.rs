//! `nlayer`, the command line tool.

use std::{path::PathBuf, process::ExitCode, sync::Arc};

use clap::{Parser, Subcommand, ValueEnum};
use nlayer_cli::{
    BuildOptions, BuildOutcome, InitOptions, IosOptions, RunOptions, TestOptions, TestTarget,
    build, init, run_android, run_ios, serve, test, web_module,
};

#[derive(Parser)]
#[command(name = "nlayer", about = "Create, build, test and run nlayer apps")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Bundle an app and write the bundle, its source map and imported images.
    Build {
        /// The entry module, or the app's directory (default: the current one), whose `App.tsx` is the entry.
        #[arg(default_value = ".")]
        entry: PathBuf,
        /// Output directory.
        #[arg(short, long, default_value = ".nlayer/out")]
        out: PathBuf,
        /// Minify the bundle.
        #[arg(long)]
        release: bool,
    },
    /// Run `*.test.tsx` files (`@nlayer/test`).
    /// An app entry instead of a test file is opened, then the steps from
    /// standard input run (`app.press("Go")`). A terminal supplies no steps.
    /// The default engine is headless. `--target web` runs the opened app
    /// in a headless browser; `--target ios` runs it in the simulator;
    /// `--target android` runs it on an emulator or a connected device.
    /// `--target` applies to an entry's steps too.
    Test {
        /// A test file, a directory of them (default: the current one), or an app entry.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Run only tests whose name contains this.
        #[arg(long)]
        filter: Option<String>,
        /// Host that runs the app `open` starts.
        #[arg(long, value_enum)]
        target: Option<TestHost>,
    },
    /// Create a TSX app: `App.tsx`, its test, `tsconfig.json` and `package.json`.
    Init {
        /// The app directory (default: the current one).
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Run the app. Web serves a browser. iOS and Android install a host once,
    /// then restart it after a rebuild without reinstalling.
    /// A failed rebuild is printed; the previous app keeps running.
    Run {
        /// The entry module, or the app's directory (default: the current one), whose `App.tsx` is the entry.
        #[arg(default_value = ".")]
        entry: PathBuf,
        /// Which host runs the app.
        #[arg(long, value_enum, default_value = "web")]
        target: Target,
        /// Port. `0` picks a free one. Web only.
        #[arg(long, default_value_t = 8733)]
        port: u16,
        /// Minify the bundle.
        #[arg(long)]
        release: bool,
        /// Do not open a browser. Web only.
        #[arg(long)]
        no_open: bool,
    },
}

/// A host `test` can drive.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum TestHost {
    /// A headless browser. The page executes the app.
    Web,
    /// The iOS simulator. The steps go through UIKit.
    Ios,
    /// An Android emulator or a connected device. The steps go through
    /// the view system.
    Android,
}

/// A host `run` can start.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum Target {
    /// This machine's browser.
    Web,
    /// The iOS simulator. A rebuild restarts the app and drops its state.
    Ios,
    /// An Android emulator or a connected device. A rebuild restarts the
    /// app and drops its state.
    Android,
}

fn main() -> ExitCode {
    init_log();
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            entry,
            out,
            release,
        } => {
            warn_types(&entry);
            match build(&BuildOptions {
                entry,
                out,
                release,
            }) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("{err}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Test {
            path,
            filter,
            target,
        } => {
            warn_types(&path);
            let target = target.map(|target| match target {
                TestHost::Web => TestTarget::Web,
                TestHost::Ios => TestTarget::Ios,
                TestHost::Android => TestTarget::Android,
            });
            match test(&TestOptions {
                path,
                filter,
                target,
            }) {
                Ok(report) => {
                    println!("{report}");
                    if report.ok() {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Err(err) => {
                    eprintln!("{err}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Init { dir } => match init(&InitOptions { dir }) {
            Ok(report) => {
                print!("{report}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        },
        Command::Run {
            entry,
            target,
            port,
            release,
            no_open,
        } => {
            warn_types(&entry);
            match target {
                Target::Web => match run_web(entry, port, release, !no_open) {
                    Ok(server) => {
                        server.park();
                        ExitCode::SUCCESS
                    }
                    Err(err) => {
                        eprintln!("{err}");
                        ExitCode::FAILURE
                    }
                },
                Target::Ios => match run_ios(IosOptions { entry, release }) {
                    Ok(session) => {
                        session.park();
                        drop(session);
                        ExitCode::SUCCESS
                    }
                    Err(err) => {
                        eprintln!("{err}");
                        ExitCode::FAILURE
                    }
                },
                Target::Android => match run_android(nlayer_cli::AndroidOptions { entry, release })
                {
                    Ok(session) => {
                        session.park();
                        drop(session);
                        ExitCode::SUCCESS
                    }
                    Err(err) => {
                        eprintln!("{err}");
                        ExitCode::FAILURE
                    }
                },
            }
        }
    }
}

fn warn_types(start: &std::path::Path) {
    if let Some(warning) = nlayer_cli::type_package_warning(start) {
        eprintln!("{warning}");
    }
}

fn run_web(
    entry: PathBuf,
    port: u16,
    release: bool,
    open: bool,
) -> Result<nlayer_cli::DevServer, nlayer_cli::RunError> {
    let wasm = web_module()?;
    let announce = Arc::new(std::sync::Once::new());
    let on_build = Arc::new(move |outcome: BuildOutcome| {
        announce.call_once(|| {
            eprintln!("http://127.0.0.1:{}/", outcome.port);
        });
        if !outcome.success {
            eprintln!("{}", outcome.error.trim_end());
        } else if outcome.first_success {
            if open {
                open_browser(&format!("http://127.0.0.1:{}/", outcome.port));
            }
        } else if outcome.updated {
            eprintln!("rebuilt in {:.0} ms", outcome.elapsed_ms);
        }
    });
    serve(RunOptions {
        entry,
        port,
        release,
        wasm,
        on_build: Some(on_build),
    })
}

fn open_browser(url: &str) {
    if let Err(error) = browser(url).spawn() {
        eprintln!("could not open a browser: {error}");
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

#[cfg(target_os = "macos")]
fn browser(url: &str) -> std::process::Command {
    let mut command = std::process::Command::new("open");
    command.arg(url);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn browser(url: &str) -> std::process::Command {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(url);
    command
}

#[cfg(windows)]
fn browser(url: &str) -> std::process::Command {
    let mut command = std::process::Command::new("cmd");
    command.args(["/C", "start", "", url]);
    command
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, Target, TestHost};

    #[test]
    fn run_defaults_to_the_browser() {
        let cli = Cli::try_parse_from(["nlayer", "run"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Run {
                target: Target::Web,
                port: 8733,
                release: false,
                no_open: false,
                ..
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["nlayer", "run", "--target", "ios"])
                .unwrap()
                .command,
            Command::Run {
                target: Target::Ios,
                ..
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["nlayer", "run", "--target", "android"])
                .unwrap()
                .command,
            Command::Run {
                target: Target::Android,
                ..
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["nlayer", "test", "--target", "web"])
                .unwrap()
                .command,
            Command::Test {
                target: Some(TestHost::Web),
                ..
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["nlayer", "test", "--target", "ios"])
                .unwrap()
                .command,
            Command::Test {
                target: Some(TestHost::Ios),
                ..
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["nlayer", "test", "--target", "android"])
                .unwrap()
                .command,
            Command::Test {
                target: Some(TestHost::Android),
                ..
            }
        ));
    }
}
