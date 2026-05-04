//! Path-construction helpers shared by all extractors.

use std::path::{Path, PathBuf};

/// Build the per-entry output path: `<archive_dir>/<archive_stem>_out/<filename>`.
///
/// All extractors funnel writes through this helper so that nested archives can be
/// discovered next to their parent and visualised consistently.
pub fn create_outpath(archive_path: &Path, filename: &Path) -> PathBuf {
    let basename = archive_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("archive");
    let dirname = archive_path.parent().unwrap_or_else(|| Path::new("."));
    PathBuf::from(dirname)
        .join(format!("{basename}_out"))
        .join(filename)
}

/// Compute `absolute_path` relative to `dir`, falling back to the original path
/// when no relation exists (e.g. across different drives on Windows).
pub fn relative_path(dir: &Path, absolute_path: &Path) -> PathBuf {
    pathdiff::diff_paths(absolute_path, dir).unwrap_or_else(|| absolute_path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_outpath_places_entry_beside_archive() {
        // /tmp/foo.zip + bar.txt  -->  /tmp/foo_out/bar.txt
        let archive = Path::new("/tmp/foo.zip");
        let out = create_outpath(archive, Path::new("bar.txt"));
        assert_eq!(out, PathBuf::from("/tmp/foo_out/bar.txt"));
    }

    #[test]
    fn create_outpath_uses_stem_not_full_name() {
        // `.tar` in `foo.tar.gz` is part of the stem; only the last extension is stripped.
        let archive = Path::new("/a/b/foo.tar.gz");
        let out = create_outpath(archive, Path::new("x"));
        assert_eq!(out, PathBuf::from("/a/b/foo.tar_out/x"));
    }

    #[test]
    fn create_outpath_preserves_nested_entry_paths() {
        let archive = Path::new("/root/pack.7z");
        let out = create_outpath(archive, Path::new("dir/sub/leaf.bin"));
        assert_eq!(out, PathBuf::from("/root/pack_out/dir/sub/leaf.bin"));
    }

    #[test]
    fn create_outpath_defaults_to_cwd_when_archive_has_no_parent() {
        // `Path::new("lone.zip").parent()` returns `Some("")` (an empty path,
        // not None), so we fall through to joining an empty dir with the
        // stem — which collapses to a plain relative `lone_out/entry`.
        let archive = Path::new("lone.zip");
        let out = create_outpath(archive, Path::new("entry"));
        assert_eq!(out, PathBuf::from("lone_out/entry"));
    }

    #[test]
    fn relative_path_produces_relative_diff() {
        let base = Path::new("/a/b");
        let target = Path::new("/a/b/c/d.txt");
        assert_eq!(relative_path(base, target), PathBuf::from("c/d.txt"));
    }

    #[test]
    fn relative_path_falls_back_to_absolute_when_no_relation() {
        // Two absolute paths in unrelated roots can always be diffed with ../
        // climbs, so pathdiff returns Some. The only real fallback is when one
        // path is relative and the other absolute.
        let base = Path::new("relative/base");
        let target = Path::new("/absolute/target");
        assert_eq!(
            relative_path(base, target),
            PathBuf::from("/absolute/target")
        );
    }
}
