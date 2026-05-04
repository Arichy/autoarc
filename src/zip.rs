use anyhow::{Context, Result};
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};
use tracing::{debug, info};
use zip::{ZipArchive, result::ZipError};

use crate::{
    TaskParams,
    error::AutoarcError,
    get_password_list,
    utils::{create_outpath, get_file_type, is_type_archive, is_type_video, rename_video},
};

pub fn unzip(archive_path: PathBuf, root: PathBuf) -> Result<Vec<TaskParams>> {
    debug!("[unzip] {archive_path:?}");
    let password = 'outer: loop {
        for password in get_password_list() {
            match check_password(&archive_path, password.as_bytes()) {
                Ok(()) => break 'outer password,
                Err(ZipError::InvalidPassword) => {}
                Err(e) => return Err(e.into()),
            }
        }
        return Err(AutoarcError::NoCorrectPassword.into());
    };

    unzip_with_password(archive_path, root, password)
}

fn check_password(archive_path: &Path, password: &[u8]) -> Result<(), ZipError> {
    let file = std::fs::File::open(archive_path)?;

    // 1. Get the file handle.
    // This doesn't check the password itself, but will return a ZipError if the index is invalid, etc.
    let mut archive = ZipArchive::new(file)?;
    let mut file = archive.by_index_decrypt(0, password)?;

    // 2. If it's a directory, there's no data to decrypt, so we assume the password is correct.
    if file.is_dir() {
        return Ok(());
    }

    // 3. Try to read one byte.
    // This is the key step that triggers decryption and password verification!
    // If the password is wrong, this step will return an io::Error.
    let mut buffer = [0u8; 1];
    file.read(&mut buffer)?; // `?` will return Err early if the password is wrong

    // 4. If read() succeeds (even if 0 bytes are read), it means the password is correct.
    Ok(())
}

fn unzip_with_password(
    archive_path: PathBuf,
    root: PathBuf,
    password: &str,
) -> Result<Vec<TaskParams>> {
    let file = std::fs::File::open(&archive_path)?;
    let mut archive = ZipArchive::new(file)?;

    let mut ret = vec![];

    for i in 0..archive.len() {
        let mut file = archive.by_index_decrypt(i, password.as_bytes())?;

        let path_clone = archive_path.clone();

        let filename = match String::from_utf8(file.name_raw().to_vec()) {
            Ok(s) => PathBuf::from(s),
            Err(_) => file.enclosed_name().ok_or_else(|| {
                AutoarcError::Other(format!("Invalid name found in {:?}", path_clone))
            })?,
        };

        if filename.to_string_lossy().contains("__MACOSX") {
            continue;
        }

        if file.is_dir() {
            continue;
        }

        let outpath = create_outpath(&archive_path, &filename);
        if let Some(parent) = outpath.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).context("create_dir_all")?;
            }
        }

        let mut outfile = File::create(&outpath).context("create")?;
        io::copy(&mut file, &mut outfile)?;
        info!(outfile = outpath.to_str());

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
