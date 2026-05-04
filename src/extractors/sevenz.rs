//! 7-Zip backend powered by [`sevenz_rust2`].

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Result;
use sevenz_rust2::{ArchiveEntry, ArchiveReader, Error as SevenzError, Password};
use tracing::debug;

use crate::error::AutoarcError;
use crate::fs::{create_outpath, get_file_type, is_type_archive, is_type_video, rename_video};
use crate::progress::TaskReporter;

use super::{ExtractOutcome, Extractor};

/// `Extractor` implementation for `.7z` archives.
pub struct SevenzExtractor;

impl Extractor for SevenzExtractor {
    fn try_extract(path: &Path, password: &str, reporter: &TaskReporter) -> Result<ExtractOutcome> {
        debug!("[7z] try_extract {path:?}");
        match check_password(path, password) {
            Ok(true) => {}
            Ok(false) => return Ok(ExtractOutcome::BadPassword),
            Err(e) => return Err(e.into()),
        }
        let children = sevenz_with_password(path, password, reporter)?;
        Ok(ExtractOutcome::Success(children))
    }
}

/// Read up to one kilobyte of the first encrypted entry to validate `password`.
fn check_password(archive_path: &Path, password_str: &str) -> Result<bool, SevenzError> {
    let file = File::open(archive_path)?;
    let pwd: Password = password_str.into();
    let mut reader = ArchiveReader::new(file, pwd)?;

    let result =
        reader.for_each_entries(&mut |entry: &ArchiveEntry, file_reader: &mut dyn Read| {
            if entry.has_stream() && entry.size() > 0 {
                // Read a small probe to force decryption.
                let probe_len = 1024.min(entry.size() as usize);
                let mut buf = vec![0u8; probe_len];
                file_reader.read_exact(&mut buf)?;
                // Stop iteration after the first encrypted entry.
                return Ok(false);
            }
            Ok(true)
        });

    match result {
        Ok(_) => Ok(true),
        Err(SevenzError::MaybeBadPassword(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Stream every entry to `<archive_dir>/<stem>_out/...` once `password` is verified.
fn sevenz_with_password(
    archive_path: &Path,
    password: &str,
    reporter: &TaskReporter,
) -> Result<Vec<PathBuf>> {
    let file =
        File::open(archive_path).map_err(|e| AutoarcError::io(archive_path.to_path_buf(), e))?;
    let pwd: Password = password.into();
    let mut reader = ArchiveReader::new(file, pwd)?;

    // We don't get an O(1) entry count from `sevenz_rust2`, so use a spinner.
    let nested_paths: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
    let archive_path_owned = archive_path.to_path_buf();

    reader.for_each_entries(&mut |entry: &ArchiveEntry, file_reader: &mut dyn Read| {
        if entry.is_directory() {
            return Ok(true);
        }
        if entry.has_stream() {
            let filename = entry.name();
            let outpath = create_outpath(&archive_path_owned, Path::new(filename));
            if let Some(parent) = outpath.parent()
                && !parent.exists()
            {
                fs::create_dir_all(parent)?;
            }

            let mut output = File::create(&outpath)?;
            std::io::copy(file_reader, &mut output)?;

            reporter.set_message(filename.to_string());
            reporter.tick();

            let kind = get_file_type(&outpath);
            if is_type_archive(kind) {
                if let Ok(mut g) = nested_paths.lock() {
                    g.push(outpath);
                }
            } else if is_type_video(kind) {
                // Bubble the video-rename error up via the closure's io::Error channel.
                rename_video(&outpath, kind).map_err(|e| std::io::Error::other(e.to_string()))?;
                reporter.note_video_renamed();
            }
        }
        Ok(true)
    })?;

    Ok(nested_paths.into_inner().unwrap_or_default())
}
