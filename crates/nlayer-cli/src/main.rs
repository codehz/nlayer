//! `nlayer`, the command line tool.

use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};
use nlayer_cli::{BuildOptions, build};

#[derive(Parser)]
#[command(name = "nlayer", about = "Build nlayer apps")]
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let Command::Build {
        entry,
        out,
        release,
    } = cli.command;
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
