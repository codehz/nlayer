//! `nlayer`, the command line tool.

use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};
use nlayer_cli::{BuildOptions, TestOptions, build, test};

#[derive(Parser)]
#[command(name = "nlayer", about = "Build and test nlayer apps")]
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
    }
}
