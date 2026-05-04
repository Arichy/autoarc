//! Error types for the `autoarc` crate.

use std::path::PathBuf;
use thiserror::Error;

/// Top-level error type returned by `autoarc` operations.
#[derive(Debug, Error)]
pub enum AutoarcError {
    /// None of the configured passwords could decrypt the archive.
    #[error("No correct password found for archive")]
    NoCorrectPassword,

    /// The `AUTOARC_PASSWORDS` environment variable is missing or empty.
    #[error(
        "Missing or empty AUTOARC_PASSWORDS environment variable. \
         Set it to a comma-separated list of candidate passwords \
         (e.g. via a `.env` file at the project root)."
    )]
    MissingPasswords,

    /// An external CLI tool (`unar`, `lsar`, ...) is not available on `PATH`.
    #[error("External tool `{0}` not found in PATH (install via `brew install unar`)")]
    ToolNotFound(&'static str),

    /// The encountered file type is not supported by the extractor pipeline.
    #[error("Unsupported file type for extraction: {0:?}")]
    UnsupportedFileType(crate::fs::FileType),

    /// I/O failure tied to a specific path, for clearer diagnostics.
    #[error("IO error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Catch-all for situations that don't fit the above variants.
    #[error("{0}")]
    Other(String),
}

impl AutoarcError {
    /// Helper to attach a path to a raw `std::io::Error`.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
