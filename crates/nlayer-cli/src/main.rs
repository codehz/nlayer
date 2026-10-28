//! `nlayer`, the command line tool.

use std::{path::PathBuf, process::ExitCode, sync::Arc};

use clap::{Parser, Subcommand, ValueEnum};
use nlayer_cli::{
    BuildOptions, BuildOutcome, InitOptions, IosOptions, RunOptions, TestOptions, build, init,
    run_ios, serve, test, web_module,
};

#[derive(Parser)]
#[command(name = "nlayer", about = "Create, build, test and run nlayer apps")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Bundle an app and write the bundle, its source map, imported images and the TypeScript declarations.
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
    /// Run `*.test.tsx` files headless (`@nlayer/test`).
    Test {
        /// A test file, or a directory (default: the current one).
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Run only tests whose name contains this.
        #[arg(long)]
        filter: Option<String>,
    },
    /// Create a TSX app: `App.tsx`, its test, `tsconfig.json` and the editor's declarations.
    Init {
        /// The app directory (default: the current one).
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Run the app. Web serves a browser. iOS installs a simulator host once,
    /// then restarts it after a rebuild without reinstalling.
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

/// A host `run` can start.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum Target {
    /// This machine's browser.
    Web,
    /// The iOS simulator. A rebuild restarts the app and drops its state.
    Ios,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            entry,
            out,
            release,
        } => match build(&BuildOptions {
            entry,
            out,
            release,
        }) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        },
        Command::Test { path, filter } => match test(&TestOptions { path, filter }) {
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
        },
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
        } => match target {
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
        },
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

    use super::{Cli, Command, Target};

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
        assert!(Cli::try_parse_from(["nlayer", "run", "--target", "android"]).is_err());
    }
}
