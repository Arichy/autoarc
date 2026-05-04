//! `unar`/`lsar` subprocess backend, used for split archives and edge cases the
//! native crates can't handle.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use tracing::{debug, error, trace};

use crate::error::AutoarcError;
use crate::fs::{get_file_type, is_type_archive, is_type_video, rename_video};
use crate::progress::TaskReporter;

use super::{ExtractOutcome, Extractor};

/// `Extractor` implementation that shells out to the `unar` binary.
pub struct UnarExtractor;

impl Extractor for UnarExtractor {
    fn try_extract(path: &Path, password: &str, reporter: &TaskReporter) -> Result<ExtractOutcome> {
        debug!("[unar] try_extract {path:?}");

        let basename = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| AutoarcError::Other(format!("invalid file stem: {path:?}")))?;
        let dirname = path.parent().unwrap_or_else(|| Path::new("."));
        let outdir = dirname.join(format!("{basename}_out"));

        if !outdir.exists() {
            fs::create_dir_all(&outdir).map_err(|e| AutoarcError::io(outdir.clone(), e))?;
        }

        reporter.set_message(format!("unar -> {}", outdir.display()));
        reporter.tick();

        let output = std::process::Command::new("unar")
            .arg("-o")
            .arg(&outdir)
            .arg("-p")
            .arg(password)
            .arg(path)
            .output()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => AutoarcError::ToolNotFound("unar"),
                _ => AutoarcError::io(path.to_path_buf(), e),
            })?;

        if !output.status.success() {
            // unar's exit codes don't distinguish bad password from other errors,
            // so treat any failure as "try the next password" while clearing the
            // partially-extracted directory to keep the workspace tidy.
            if outdir.exists() {
                let _ = fs::remove_dir_all(&outdir);
            }
            return Ok(ExtractOutcome::BadPassword);
        }

        let entries = lsar(path)?;
        let mut nested = Vec::new();
        for entry in entries {
            let filepath = outdir.join(&entry);
            let kind = get_file_type(&filepath);
            if is_type_archive(kind) {
                nested.push(filepath);
            } else if is_type_video(kind) {
                rename_video(&filepath, kind)?;
                reporter.note_video_renamed();
            }
            reporter.tick();
        }

        Ok(ExtractOutcome::Success(nested))
    }
}

/// Run `lsar <archive>` and parse the entry list (one path per line, header skipped).
pub fn lsar(archive_path: &Path) -> Result<Vec<PathBuf>> {
    debug!("lsar {}", archive_path.display());
    let output = std::process::Command::new("lsar")
        .arg(archive_path)
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => AutoarcError::ToolNotFound("lsar"),
            _ => AutoarcError::io(archive_path.to_path_buf(), e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("lsar error for {:?}: {}", archive_path, stderr);
        return Err(AutoarcError::Other(format!(
            "lsar error for file {:?}: {}",
            archive_path, stderr
        ))
        .into());
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|e| AutoarcError::Other(format!("lsar produced non-UTF8 output: {e}")))?;
    trace!("lsar output: {stdout}");

    // The first line is `Archive: <name>` metadata; the entries follow.
    Ok(stdout
        .lines()
        .skip(1)
        .map(|line| PathBuf::from(line.to_string()))
        .collect())
}
