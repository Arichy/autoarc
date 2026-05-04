//! Path-construction helpers shared by all extractors.

use std::path::{Path, PathBuf};

use chrono::Local;

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

/// Today's working directory inside `basedir`, named like `MM-DD`.
pub fn today_dir_name(basedir: &Path) -> PathBuf {
    let stamp = Local::now().format("%m-%d").to_string();
    basedir.join(stamp)
}

/// Today's backup directory inside `basedir`, named like `MM-DD_bak`.
pub fn today_bak_dir_name(basedir: &Path) -> PathBuf {
    let stamp = Local::now().format("%m-%d_bak").to_string();
    basedir.join(stamp)
}

/// Compute `absolute_path` relative to `dir`, falling back to the original path
/// when no relation exists (e.g. across different drives on Windows).
pub fn relative_path(dir: &Path, absolute_path: &Path) -> PathBuf {
    pathdiff::diff_paths(absolute_path, dir).unwrap_or_else(|| absolute_path.to_path_buf())
}
