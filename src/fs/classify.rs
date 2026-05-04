//! File-type classification using `infer` magic-byte sniffing, with a `file(1)` fallback.

use std::io::Read;
use std::path::Path;

/// How many bytes of a PE/EXE body we scan for an embedded archive signature.
/// SFX stubs are almost always smaller than this, and it bounds the I/O cost
/// for huge `.exe` files.
const SFX_SCAN_BYTES: usize = 4 * 1024 * 1024;

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
    /// Archive that must be handled by the `unar` subprocess backend:
    /// a multi-volume set whose first part has a `.z01` / `.001` / `.7z.001`
    /// extension. Native Rust crates can't join volumes, so we delegate.
    Multi,
    /// Self-extracting `.exe` archive (SFX): a Windows PE executable with a
    /// 7z / RAR / ZIP payload appended after the stub. Also handled by the
    /// `unar` subprocess backend, which transparently skips the PE prefix.
    Sfx,
    /// Anything we couldn't identify.
    Unknown,
}

/// Detect a file's [`FileType`].
///
/// Magic bytes are checked first via `infer`; if that yields nothing we fall back
/// to `file -I -b` for MPEG-TS detection, which `infer` does not currently support.
/// When `infer` reports a Windows PE executable we additionally scan the first
/// few megabytes for an embedded 7z / RAR / ZIP signature (self-extracting
/// archives), routing any hit through the `unar` backend.
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
            "application/x-7z-compressed" => {
                // A `.7z.001` first-part (extension `001`) is a multi-volume
                // split; the native 7z crate can't join volumes, so route it
                // to `unar` along with the rest of the split family.
                let is_multi = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext == "001");
                if is_multi {
                    FileType::Multi
                } else {
                    FileType::SevenZ
                }
            }
            "video/mp4" | "video/x-m4v" => FileType::Mp4,
            "application/vnd.microsoft.portable-executable" => {
                // Likely an SFX — scan the body for a payload signature.
                detect_sfx(path).unwrap_or(FileType::Unknown)
            }
            _ => FileType::Unknown,
        },
        Ok(None) => detect_via_file_cmd(path).unwrap_or(FileType::Unknown),
        Err(_) => FileType::Unknown,
    }
}

/// Search the first [`SFX_SCAN_BYTES`] of `path` for a 7z / RAR / ZIP magic
/// signature. Returns [`FileType::Sfx`] on any hit so the file is routed to
/// the `unar` subprocess, which handles SFX binaries natively.
fn detect_sfx(path: &Path) -> Option<FileType> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; SFX_SCAN_BYTES];
    // `read` may return short; that's fine — we just search what we got.
    let n = file.read(&mut buf).ok()?;
    let window = &buf[..n];

    // 7z:  37 7A BC AF 27 1C
    // RAR4: "Rar!" 1A 07 00
    // RAR5: "Rar!" 1A 07 01 00
    // ZIP:  "PK" 03 04
    const SEVENZ: &[u8] = &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];
    const RAR: &[u8] = b"Rar!\x1A\x07";
    const ZIP: &[u8] = b"PK\x03\x04";

    if contains(window, SEVENZ) || contains(window, RAR) || contains(window, ZIP) {
        Some(FileType::Sfx)
    } else {
        None
    }
}

/// Plain substring search — small enough that the standard library's naive
/// scan is fine for our ~4 MiB window.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
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
        FileType::Zip | FileType::Rar | FileType::SevenZ | FileType::Multi | FileType::Sfx
    )
}

/// Returns `true` for the video formats the pipeline can post-process.
pub fn is_type_video(t: FileType) -> bool {
    matches!(t, FileType::Mp4 | FileType::TS)
}
