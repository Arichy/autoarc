use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::Local;
use tracing::error;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum FileType {
    Zip,
    Rar,
    SevenZ,
    Mp4,
    TS,
    Multi,
    Unknown,
}

pub fn get_file_type(path: &Path) -> FileType {
    match infer::get_from_path(path) {
        Ok(Some(kind)) => match kind.mime_type() {
            "application/zip" => {
                let ext = path.extension();
                let is_multi = ext
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext == "z01" || ext == "001")
                    .unwrap_or(false);

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
        Ok(None) => {
            let output = std::process::Command::new("file")
                .arg("-I")
                .arg("-b")
                .arg(path)
                .output()
                .unwrap();

            if output.status.success() {
                let output = unsafe { str::from_utf8_unchecked(&output.stdout) };
                if output.to_lowercase().starts_with("video/mp2t") {
                    return FileType::TS;
                }
            }

            FileType::Unknown
        }
        Err(_) => FileType::Unknown,
    }
}

pub fn is_type_archive(r#type: FileType) -> bool {
    [
        FileType::Rar,
        FileType::SevenZ,
        FileType::Zip,
        FileType::Multi,
    ]
    .contains(&r#type)
}

pub fn is_type_video(r#type: FileType) -> bool {
    [FileType::Mp4, FileType::TS].contains(&r#type)
}

pub fn create_outpath(archive_path: &Path, filename: &Path) -> PathBuf {
    let basename = archive_path.file_stem().unwrap().to_str().unwrap();
    let dirname = archive_path.parent().unwrap();
    PathBuf::from(&dirname)
        .join(format!("{basename}_out"))
        .join(&filename)
}

pub fn today_dir_name(basedir: &Path) -> PathBuf {
    let today = Local::now();
    let date_string = today.format("%m-%d").to_string();
    PathBuf::from(basedir).join(date_string)
}

pub fn today_bak_dir_name(basedir: &Path) -> PathBuf {
    let today = Local::now();
    let date_string = today.format("%m-%d_bak").to_string();
    PathBuf::from(basedir).join(date_string)
}

pub fn rename_video(path: &Path, file_type: FileType) {
    let target_ext = match file_type {
        FileType::Mp4 => "mp4",
        FileType::TS => "ts",
        _ => {
            error!("Not a video: {path:?}");
            return;
        }
    };

    match path.extension() {
        Some(ext) => {
            if ext.to_string_lossy().to_ascii_lowercase() != target_ext {
                let parent = path.parent().unwrap();
                let file_stem = path.file_stem().unwrap().to_str().unwrap();
                let new_filename = format!("{}.{}", file_stem, target_ext);
                let new_path = parent.join(new_filename);

                fs::rename(path, new_path).unwrap();
            }
        }
        None => {
            let parent = path.parent().unwrap();
            let file_stem = path.file_stem().unwrap().to_str().unwrap();
            let new_filename = format!("{}.{}", file_stem, target_ext);
            let new_path = parent.join(new_filename);

            fs::rename(path, new_path).unwrap();
        }
    }
}

pub fn relative_path(dir: &Path, absolute_path: &Path) -> PathBuf {
    pathdiff::diff_paths(absolute_path, dir).unwrap_or_else(|| absolute_path.to_path_buf())
}
