//! Video file post-processing: enforce the canonical extension for MP4/TS files.

use std::path::Path;

use crate::error::AutoarcError;
use crate::fs::FileType;

/// Rename `path` so its extension matches `file_type` (`.mp4` or `.ts`).
///
/// This is a no-op if the extension already matches. Any rename failure is wrapped
/// in [`AutoarcError::Io`] for consistent reporting.
pub fn rename_video(path: &Path, file_type: FileType) -> Result<(), AutoarcError> {
    let target_ext = match file_type {
        FileType::Mp4 => "mp4",
        FileType::TS => "ts",
        _ => return Ok(()), // not a video; nothing to do
    };

    // Skip if the extension is already correct (case-insensitive).
    if let Some(ext) = path.extension()
        && ext.to_string_lossy().to_ascii_lowercase() == target_ext
    {
        return Ok(());
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let new_path = parent.join(format!("{stem}.{target_ext}"));

    std::fs::rename(path, &new_path).map_err(|e| AutoarcError::io(path.to_path_buf(), e))?;
    Ok(())
}
