//! Path-construction helpers shared by all extractors.

use std::path::{Path, PathBuf};

/// Build the sibling output directory **name** (not a full path) for an
/// archive, replacing every `.` in the file name with `_` and appending
/// `_out`.
///
/// Examples:
///   `foo.zip`        → `foo_zip_out`
///   `foo.7z`         → `foo_7z_out`
///   `split.7z.001`   → `split_7z_001_out`
///   `archive`        → `archive_out`
///
/// This disambiguates sibling archives that share a stem — e.g. `foo.zip`
/// and `foo.7z` no longer both target `foo_out/` and collide.
pub fn out_dir_name(archive_path: &Path) -> String {
    let filename = archive_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("archive");
    format!("{}_out", filename.replace('.', "_"))
}

/// Build the per-entry output path: `<archive_dir>/<out_dir_name>/<filename>`.
///
/// All extractors funnel writes through this helper so that nested archives can be
/// discovered next to their parent and visualised consistently.
pub fn create_outpath(archive_path: &Path, filename: &Path) -> PathBuf {
    let dirname = archive_path.parent().unwrap_or_else(|| Path::new("."));
    PathBuf::from(dirname)
        .join(out_dir_name(archive_path))
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

    // --- out_dir_name --------------------------------------------------------

    #[test]
    fn out_dir_name_replaces_single_extension_dot() {
        assert_eq!(out_dir_name(Path::new("foo.zip")), "foo_zip_out");
        assert_eq!(out_dir_name(Path::new("/tmp/foo.7z")), "foo_7z_out");
    }

    #[test]
    fn out_dir_name_disambiguates_same_stem_siblings() {
        // The whole point of this helper: `foo.zip` and `foo.7z` must not
        // collide on a single `foo_out` directory any more.
        assert_ne!(
            out_dir_name(Path::new("foo.zip")),
            out_dir_name(Path::new("foo.7z"))
        );
    }

    #[test]
    fn out_dir_name_replaces_every_dot_for_multi_volume() {
        // Multi-volume primaries (foo.7z.001) still get distinct, stable names.
        assert_eq!(out_dir_name(Path::new("split.7z.001")), "split_7z_001_out");
        assert_eq!(out_dir_name(Path::new("pack.001")), "pack_001_out");
    }

    #[test]
    fn out_dir_name_handles_no_extension() {
        assert_eq!(out_dir_name(Path::new("archive")), "archive_out");
    }

    #[test]
    fn out_dir_name_falls_back_when_no_file_name() {
        // `Path::new(".")` has no file_name; we must still emit a valid name.
        assert_eq!(out_dir_name(Path::new("..")), "archive_out");
    }

    // --- create_outpath ------------------------------------------------------

    #[test]
    fn create_outpath_places_entry_beside_archive() {
        // /tmp/foo.zip + bar.txt  -->  /tmp/foo_zip_out/bar.txt
        let archive = Path::new("/tmp/foo.zip");
        let out = create_outpath(archive, Path::new("bar.txt"));
        assert_eq!(out, PathBuf::from("/tmp/foo_zip_out/bar.txt"));
    }

    #[test]
    fn create_outpath_keeps_every_dot_segment_in_directory_name() {
        // `foo.tar.gz` now becomes `foo_tar_gz_out` (was `foo.tar_out`).
        let archive = Path::new("/a/b/foo.tar.gz");
        let out = create_outpath(archive, Path::new("x"));
        assert_eq!(out, PathBuf::from("/a/b/foo_tar_gz_out/x"));
    }

    #[test]
    fn create_outpath_preserves_nested_entry_paths() {
        let archive = Path::new("/root/pack.7z");
        let out = create_outpath(archive, Path::new("dir/sub/leaf.bin"));
        assert_eq!(out, PathBuf::from("/root/pack_7z_out/dir/sub/leaf.bin"));
    }

    #[test]
    fn create_outpath_defaults_to_cwd_when_archive_has_no_parent() {
        // `Path::new("lone.zip").parent()` returns `Some("")` (an empty path,
        // not None), so we fall through to joining an empty dir with the
        // computed out name — which collapses to a plain relative path.
        let archive = Path::new("lone.zip");
        let out = create_outpath(archive, Path::new("entry"));
        assert_eq!(out, PathBuf::from("lone_zip_out/entry"));
    }

    // --- relative_path -------------------------------------------------------

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
