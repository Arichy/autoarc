//! Command-line argument parsing.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Top-level CLI entry point.
#[derive(Debug, Parser)]
#[command(
    name = "autoarc",
    about = "Concurrent multi-format archive extractor with password trial-and-error",
    version
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

/// Available subcommands.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Print the detected archive/video file type for a single file.
    Type { filepath: PathBuf },

    /// Run `lsar` against a single archive and print its entry list.
    Lsar { filepath: PathBuf },

    /// Recursively unpack every supported archive in `dir`.
    Autoarc {
        /// Directory to scan for archives.
        dir: PathBuf,

        /// Maximum directory depth to scan for archives.
        ///
        /// `1` (the default) only inspects the immediate contents of `dir`.
        /// `2` also enters direct subdirectories, and so on. A value of `0`
        /// is treated the same as `--recursive`.
        #[arg(short, long, default_value_t = 1)]
        depth: usize,

        /// Shortcut for unlimited recursion (overrides `--depth`).
        #[arg(short, long, default_value_t = false)]
        recursive: bool,

        /// Print the extraction plan and exit without touching the filesystem.
        #[arg(short = 'n', long, default_value_t = false)]
        dry_run: bool,

        /// Skip the interactive confirmation prompt (assume "yes").
        ///
        /// Has no effect when stdin is not a TTY — in that case no prompt is
        /// shown and execution always proceeds.
        #[arg(short, long, default_value_t = false)]
        yes: bool,
    },
}
