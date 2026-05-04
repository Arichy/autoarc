//! ZIP backend powered by the [`zip`] crate.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::debug;
use zip::{ZipArchive, result::ZipError};

use crate::error::AutoarcError;
use crate::fs::{create_outpath, get_file_type, is_type_archive, is_type_video, rename_video};
use crate::progress::TaskReporter;

use super::{ExtractOutcome, Extractor};

/// `Extractor` implementation for single-volume ZIP archives.
pub struct ZipExtractor;

impl Extractor for ZipExtractor {
    fn try_extract(path: &Path, password: &str, reporter: &TaskReporter) -> Result<ExtractOutcome> {
        debug!("[zip] try_extract {path:?}");
        match check_password(path, password.as_bytes()) {
            Ok(()) => {}
            Err(ZipError::InvalidPassword) => return Ok(ExtractOutcome::BadPassword),
            // Some encrypted ZIPs report a wrong password as a generic IO/CRC failure.
            // Treat any IO error during the one-byte probe as a bad-password signal.
            Err(ZipError::Io(_)) => return Ok(ExtractOutcome::BadPassword),
            Err(e) => return Err(e.into()),
        }
        let children = unzip_with_password(path, password, reporter)?;
        Ok(ExtractOutcome::Success(children))
    }
}

/// Probe the first entry to validate `password` without writing anything to disk.
fn check_password(archive_path: &Path, password: &[u8]) -> Result<(), ZipError> {
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut entry = archive.by_index_decrypt(0, password)?;

    // Directories carry no encrypted payload; nothing to verify.
    if entry.is_dir() {
        return Ok(());
    }

    // Reading one byte forces the decryption code path; a bad key surfaces here.
    let mut buf = [0u8; 1];
    let _ = entry.read(&mut buf)?;
    Ok(())
}

/// Stream every entry to `<archive_dir>/<stem>_out/...` once we've confirmed `password`.
fn unzip_with_password(
    archive_path: &Path,
    password: &str,
    reporter: &TaskReporter,
) -> Result<Vec<PathBuf>> {
    let file =
        File::open(archive_path).map_err(|e| AutoarcError::io(archive_path.to_path_buf(), e))?;
    let mut archive = ZipArchive::new(file)?;

    reporter.set_length(archive.len() as u64);

    let mut nested = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index_decrypt(i, password.as_bytes())?;

        // Resolve the entry's name as a `PathBuf`, preferring the raw bytes
        // for non-UTF8 filenames common in legacy archives.
        let filename = match String::from_utf8(entry.name_raw().to_vec()) {
            Ok(s) => PathBuf::from(s),
            Err(_) => entry.enclosed_name().ok_or_else(|| {
                AutoarcError::Other(format!("Invalid name found in {:?}", archive_path))
            })?,
        };

        // Skip macOS metadata sidecars and pure directory entries.
        if filename.to_string_lossy().contains("__MACOSX") || entry.is_dir() {
            reporter.inc();
            continue;
        }

        let outpath = create_outpath(archive_path, &filename);
        if let Some(parent) = outpath.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent).context("create parent dir")?;
        }

        let mut out = File::create(&outpath).map_err(|e| AutoarcError::io(outpath.clone(), e))?;
        io::copy(&mut entry, &mut out).map_err(|e| AutoarcError::io(outpath.clone(), e))?;

        reporter.set_message(filename.to_string_lossy().into_owned());
        reporter.inc();

        // Classify the freshly-written file.
        let kind = get_file_type(&outpath);
        if is_type_archive(kind) {
            nested.push(outpath);
        } else if is_type_video(kind) {
            rename_video(&outpath, kind)?;
            reporter.note_video_renamed();
        }
    }

    Ok(nested)
}
