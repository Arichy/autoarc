//! Filesystem-level helpers: file-type classification, path conventions, and video renaming.

mod classify;
mod paths;
mod video;

pub use classify::{FileType, get_file_type, is_type_archive, is_type_video};
pub use paths::{create_outpath, relative_path, today_bak_dir_name, today_dir_name};
pub use video::rename_video;
