use anyhow::Result;
use sevenz_rust2::{ArchiveEntry, ArchiveReader, Error as SevenzError, Password};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
use tracing::field::debug;

use crate::{
    TaskParams,
    error::AutoarcError,
    get_password_list,
    utils::{create_outpath, get_file_type, is_type_archive, is_type_video, rename_video},
};

pub fn sevenz_unzip(archive_path: PathBuf, root: PathBuf) -> Result<Vec<TaskParams>> {
    debug("[7z_unzip] {archive_path:?}");

    let password = 'outer: loop {
        for password in get_password_list() {
            match check_password(&archive_path, password) {
                Ok(true) => break 'outer password,
                Ok(false) => {}
                Err(e) => return Err(e.into()),
            }
        }
        return Err(AutoarcError::NoCorrectPassword.into());
    };

    sevenz_unzip_with_password(archive_path, root, password)
}

fn check_password(archive_path: &Path, password_str: &str) -> Result<bool, SevenzError> {
    let file = File::open(archive_path)?;
    let pwd: Password = password_str.into();

    let mut reader = ArchiveReader::new(file, pwd)?;

    let mut found_encrypted = false;
    let result =
        reader.for_each_entries(&mut |entry: &ArchiveEntry, file_reader: &mut dyn Read| {
            if entry.has_stream() && entry.size() > 0 {
                found_encrypted = true;
                let mut buf = vec![0u8; 1024.min(entry.size() as usize)];
                file_reader.read_exact(&mut buf)?;
                return Ok(false);
            }
            Ok(true)
        });

    match result {
        Ok(_) => Ok(true),
        Err(SevenzError::MaybeBadPassword(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

fn sevenz_unzip_with_password(
    archive_path: PathBuf,
    root: PathBuf,
    password: &str,
) -> Result<Vec<TaskParams>> {
    let file = File::open(&archive_path)?;
    let password: Password = password.into();

    let mut reader = ArchiveReader::new(file, password)?;

    let mut ret = vec![];

    reader.for_each_entries(&mut |entry: &ArchiveEntry, file_reader: &mut dyn Read| {
        if entry.is_directory() {
            return Ok(true);
        }
        if entry.has_stream() {
            let filename = entry.name();
            let outpath = create_outpath(&archive_path, Path::new(filename));
            if let Some(parent) = outpath.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            let mut output = File::create(&outpath)?;
            std::io::copy(file_reader, &mut output)?;

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
        Ok(true)
    })?;

    Ok(ret)
}
