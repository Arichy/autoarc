//! `autoarc` library crate: concurrent multi-format archive extractor.
//!
//! See the [`runner`] module for the high-level pipeline and [`extractors`] for
//! the pluggable backends. The accompanying binary in `src/main.rs` is a thin
//! shell that just parses CLI arguments and dispatches to this crate.

pub mod cli;
pub mod config;
pub mod error;
pub mod extractors;
pub mod fs;
pub mod progress;
pub mod runner;

pub use error::AutoarcError;
