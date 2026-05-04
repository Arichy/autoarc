//! File-type classification using `infer` magic-byte sniffing, with a `file(1)` fallback.

use std::path::Path;

/// High-level classification of a single file.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum FileType {
    /// Single-volume ZIP archive.
    Zip,
    /// RAR archive (any version).
    Rar,
    /// 7-Zip archive.
    SevenZ,
    /// MP4 / M4V video container.
    Mp4,
    /// MPEG transport-stream video.
    TS,
    /// Multi-volume archive whose first part has a `.z01` or `.001` extension.
    /// Always handled by the `unar` subprocess backend.
    Multi,
    /// Anything we couldn't identify.
    Unknown,
}

/// Detect a file's [`FileType`].
///
/// Magic bytes are checked first via `infer`; if that yields nothing we fall back
/// to `file -I -b` for MPEG-TS detection, which `infer` does not currently support.
pub fn get_file_type(path: &Path) -> FileType {
    match infer::get_from_path(path) {
        Ok(Some(kind)) => match kind.mime_type() {
            "application/zip" => {
                // ZIP magic with a `.z01`/`.001` extension means a split archive.
                let is_multi = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext == "z01" || ext == "001");
                if is_multi {
                    FileType::Multi
                } else {
                    FileType::Zip
                }
            }
            "application/vnd.rar" => FileType::Rar,
            "application/x-7z-compressed" => FileType::SevenZ,
            "video/mp4" | "video/x-m4v" => FileType::Mp4,
            _ => FileType::Unknown,
        },
        Ok(None) => detect_via_file_cmd(path).unwrap_or(FileType::Unknown),
        Err(_) => FileType::Unknown,
    }
}

/// Fallback that invokes `file -I -b <path>` to spot MPEG-TS streams.
///
/// Returns `None` if the binary is missing, exits non-zero, or emits non-UTF-8 data;
/// in those cases the caller treats the file as [`FileType::Unknown`].
fn detect_via_file_cmd(path: &Path) -> Option<FileType> {
    let output = std::process::Command::new("file")
        .arg("-I")
        .arg("-b")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = std::str::from_utf8(&output.stdout).ok()?;
    if stdout.to_lowercase().starts_with("video/mp2t") {
        Some(FileType::TS)
    } else {
        None
    }
}

/// Returns `true` when the file is one of the supported archive formats.
pub fn is_type_archive(t: FileType) -> bool {
    matches!(
        t,
        FileType::Zip | FileType::Rar | FileType::SevenZ | FileType::Multi
    )
}

/// Returns `true` for the video formats the pipeline can post-process.
pub fn is_type_video(t: FileType) -> bool {
    matches!(t, FileType::Mp4 | FileType::TS)
}
