//! Command-line argument parsing.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Top-level CLI entry point.
///
/// The common case — "extract every archive in this directory" — is the
/// default action at the top level, so users type `autoarc <DIR>` (not
/// `autoarc autoarc <DIR>`). Introspection helpers live under real
/// subcommands (`autoarc type`, `autoarc lsar`).
#[derive(Debug, Parser)]
#[command(
    name = "autoarc",
    about = "Concurrent multi-format archive extractor with password trial-and-error",
    version,
    // `autoarc <DIR>` works without naming a subcommand; `autoarc type FILE`
    // dispatches to the subcommand. When a subcommand is present the top-level
    // flags/args are ignored.
    subcommand_negates_reqs = true,
)]
pub struct Args {
    /// Directory to scan for archives.
    ///
    /// Defaults to the current working directory (`.`) when omitted.
    /// Ignored when a subcommand (`type`, `lsar`, …) is used.
    #[arg(default_value = ".")]
    pub dir: PathBuf,

    /// Maximum directory depth to scan for archives.
    ///
    /// `1` (the default) only inspects the immediate contents of `dir`.
    /// `2` also enters direct subdirectories, and so on. A value of `0`
    /// is treated the same as `--recursive`.
    #[arg(short, long, default_value_t = 1)]
    pub depth: usize,

    /// Shortcut for unlimited recursion (overrides `--depth`).
    #[arg(short, long, default_value_t = false)]
    pub recursive: bool,

    /// Print the extraction plan and exit without touching the filesystem.
    #[arg(short = 'n', long, default_value_t = false)]
    pub dry_run: bool,

    /// Skip the interactive confirmation prompt (assume "yes").
    ///
    /// Has no effect when stdin is not a TTY — in that case no prompt is
    /// shown and execution always proceeds.
    #[arg(short, long, default_value_t = false)]
    pub yes: bool,

    /// Maximum number of archives to extract in parallel.
    ///
    /// `0` (the default) means "auto" — use
    /// [`std::thread::available_parallelism`] (falling back to `4`). Use
    /// `-j 1` to force strictly sequential extraction. Parallelism is a
    /// per-invocation knob and is *not* configurable via an environment
    /// variable.
    #[arg(short = 'j', long, default_value_t = 0)]
    pub jobs: usize,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Introspection subcommands. The main extraction flow lives at the top level.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Print the detected archive/video file type for a single file.
    Type { filepath: PathBuf },

    /// Run `lsar` against a single archive and print its entry list.
    Lsar { filepath: PathBuf },
}
