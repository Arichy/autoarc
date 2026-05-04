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
    Autoarc { dir: PathBuf },
}
