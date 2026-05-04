//! RAR backend powered by the [`unrar`] crate.

use std::path::{Path, PathBuf};

use anyhow::Result;
use tracing::debug;
use unrar::{
    Archive,
    error::{Code, UnrarError},
};

use crate::fs::{create_outpath, get_file_type, is_type_archive, is_type_video, rename_video};
use crate::progress::TaskReporter;

use super::{ExtractOutcome, Extractor};

/// `Extractor` implementation for RAR archives.
pub struct RarExtractor;

impl Extractor for RarExtractor {
    fn try_extract(
        path: &Path,
        password: &str,
        reporter: &TaskReporter,
    ) -> Result<ExtractOutcome> {
        debug!("[rar] try_extract {path:?}");
        match check_password(path, password.as_bytes()) {
            Ok(()) => {}
            Err(e) if e.code == Code::BadPassword => return Ok(ExtractOutcome::BadPassword),
            Err(e) => return Err(e.into()),
        }
        let children = unrar_with_password(path, password, reporter)?;
        Ok(ExtractOutcome::Success(children))
    }
}

/// Open the archive and run `test()` on the first header to verify the password.
fn check_password(archive_path: &Path, password: &[u8]) -> Result<(), UnrarError> {
    let archive = Archive::with_password(archive_path, password).open_for_processing()?;
    if let Some(header) = archive.read_header()? {
        header.test()?;
    }
    Ok(())
}

/// Walk every entry in the archive once the password is known, writing files and
/// collecting any nested archives to schedule next.
fn unrar_with_password(
    archive_path: &Path,
    password: &str,
    reporter: &TaskReporter,
) -> Result<Vec<PathBuf>> {
    let mut archive = Archive::with_password(archive_path, password).open_for_processing()?;

    // RAR doesn't expose entry count up-front, so stay in spinner mode.
    let mut nested = Vec::new();

    while let Some(header) = archive.read_header()? {
        if header.entry().is_directory() {
            archive = header.skip()?;
            continue;
        }

        let filename = header.entry().filename.clone();
        if filename.to_string_lossy().contains("__MACOSX") {
            archive = header.skip()?;
            continue;
        }

        let outpath = create_outpath(archive_path, &filename);
        archive = header.extract_to(&outpath)?;

        reporter.set_message(filename.to_string_lossy().into_owned());
        reporter.tick();

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
