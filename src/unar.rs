use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::{get_password_list, TaskParams,
    error::AutoarcError,
    utils::{get_file_type, is_type_archive, is_type_video, rename_video},
};
use tracing::{debug, error, trace};

pub fn lsar(archive_path: &Path) -> Result<Vec<PathBuf>> {
    debug!("lsar {}", archive_path.display());
    let output = std::process::Command::new("lsar")
        .arg(archive_path)
        .output()?;

    if output.status.success() {
        let stdout = output.stdout;
        let stdout_str = String::from_utf8(stdout.clone()).unwrap();
        trace!("lsar output: {stdout_str}");

        let mut lines = stdout_str.lines();
        lines.next();

        Ok(lines.map(|item| PathBuf::from(item.to_string())).collect())
    } else {
        error!(
            "lsar error for file {:?}: {}",
            archive_path,
            String::from_utf8_lossy(&output.stderr)
        );
        Err(AutoarcError::Other(format!(
            "lsar error for file {:?}: {}",
            archive_path,
            String::from_utf8_lossy(&output.stdout)
        ))
        .into())
    }
}

pub fn unar(archive_path: PathBuf, root: PathBuf) -> Result<Vec<TaskParams>> {
    debug!("[unrar] {archive_path:?}");
    let basename = archive_path.file_stem().unwrap();
    let dirname = archive_path.parent().unwrap();
    let outdir = PathBuf::from(&dirname).join(format!("{}_out", basename.to_string_lossy()));
    if !outdir.exists() {
        fs::create_dir_all(&outdir).unwrap();
    }

    for password in get_password_list() {
        let output = std::process::Command::new("unar")
            .arg("-o")
            .arg(&outdir)
            .arg("-p")
            .arg(password)
            .arg(&archive_path)
            .output()?;

        if output.status.success() {
            let output_files = lsar(&archive_path)?
                .iter()
                .filter_map(&mut |file: &PathBuf| {
                    let filepath = Path::new(&outdir).join(file);
                    let filetype = get_file_type(&filepath);
                    if is_type_archive(filetype) {
                        Some(TaskParams {
                            archive_path: PathBuf::from(&filepath),
                            root: root.clone(),
                        })
                    } else if is_type_video(filetype) {
                        rename_video(&filepath, filetype);
                        None
                    } else {
                        None
                    }
                })
                .collect();

            return Ok(output_files);
        } else {
            if outdir.exists() {
                fs::remove_dir_all(&outdir).unwrap();
            }
        }
    }

    error!(
        "unar error for file {:?}: no correct password",
        archive_path,
    );
    Err(AutoarcError::Other(format!(
        "unar error for file {:?}: no correct password",
        archive_path,
    ))
    .into())
}
