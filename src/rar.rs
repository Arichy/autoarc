use std::path::{Path, PathBuf};

use anyhow::Result;
use tracing::debug;
use unrar::{
    Archive,
    error::{Code, UnrarError},
};

use crate::{
    TaskParams,
    error::AutoarcError,
    get_password_list,
    utils::{create_outpath, get_file_type, is_type_archive, is_type_video, rename_video},
};

pub fn unrar(archive_path: PathBuf, root: PathBuf) -> Result<Vec<TaskParams>> {
    debug!("[unrar] {archive_path:?}");

    let password = 'outer: loop {
        for password in get_password_list() {
            match check_password(&archive_path, password.as_bytes()) {
                Ok(()) => break 'outer password,
                Err(e) => {
                    if e.code == Code::BadPassword {
                    } else {
                        return Err(e.into());
                    }
                }
            }
        }
        return Err(AutoarcError::NoCorrectPassword.into());
    };

    unrar_with_password(archive_path, root, password)
}

fn check_password(archive_path: &Path, password: &[u8]) -> Result<(), UnrarError> {
    let archive = Archive::with_password(archive_path, password).open_for_processing()?;

    if let Some(header) = archive.read_header()? {
        match header.test() {
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

fn unrar_with_password(
    archive_path: PathBuf,
    root: PathBuf,
    password: &str,
) -> Result<Vec<TaskParams>> {
    let mut archive = Archive::with_password(&archive_path, password).open_for_processing()?;

    let mut ret = vec![];

    while let Some(header) = archive.read_header()? {
        if header.entry().is_directory() {
            archive = header.skip()?;
            continue;
        }

        let filename: &PathBuf = &header.entry().filename;

        if filename.to_string_lossy().contains("__MACOSX") {
            archive = header.skip()?;
            continue;
        }

        let outpath = create_outpath(&archive_path, &filename);

        archive = header.extract_to(&outpath)?;

        let filetype = get_file_type(&outpath);

        if is_type_archive(filetype) {
            let root = root.clone();

            ret.push(TaskParams {
                archive_path: outpath,
                root,
            });
        } else if is_type_video(filetype) {
            rename_video(&outpath, filetype);
        }
    }

    Ok(ret)
}
