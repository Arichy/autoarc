mod error;
mod rar;
mod sevenz;
mod unar;
mod utils;
mod zip;

use crate::rar::unrar;
use crate::sevenz::sevenz_unzip;
use crate::unar::{lsar, unar};
use crate::utils::{
    FileType, get_file_type, is_type_archive, is_type_video, relative_path, rename_video,
    today_bak_dir_name, today_dir_name,
};
use crate::zip::unzip;
use anyhow::Result;
use clap::{Parser, Subcommand};
use infer::get_from_path;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Notify, mpsc};
use tracing::{Instrument, Level, debug, info, span};
use tracing_subscriber;
use std::sync::OnceLock;

static PASSWORD_LIST: OnceLock<Vec<String>> = OnceLock::new();

pub fn get_password_list() -> &'static Vec<String> {
    PASSWORD_LIST.get_or_init(|| {
        let passwords = std::env::var("AUTOARC_PASSWORDS").expect("AUTOARC_PASSWORDS must be set");
        passwords.split(',').map(|s| s.to_string()).collect()
    })
}

#[derive(Debug, Clone)]
struct TaskParams {
    archive_path: PathBuf,
    root: PathBuf,
}

impl TaskParams {
    pub fn display(&self, dir: &Path) -> String {
        let rel_archive_path = relative_path(dir, &self.archive_path);
        let rel_root = relative_path(dir, &self.root);

        if rel_archive_path == rel_root {
            rel_archive_path.to_string_lossy().to_string()
        } else {
            format!(
                "{} <- {}",
                rel_archive_path.to_string_lossy(),
                rel_root.to_string_lossy()
            )
        }
    }
}

#[derive(Debug, Parser)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Type { filepath: PathBuf },
    Lsar { filepath: PathBuf },
    Autoarc { dir: PathBuf },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    match args.command {
        Commands::Type { filepath } => {
            let mime = get_from_path(&filepath);
            println!("MIME: {mime:?}");
            println!("{:?}", get_file_type(&filepath));
        }
        Commands::Lsar { filepath } => {
            println!("{:?}", lsar(&filepath));
        }
        Commands::Autoarc { dir } => autoarc_main(dir).await?,
    }

    Ok(())
}

async fn autoarc_main(dir: PathBuf) -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<TaskParams>(32);

    let active_tasks = Arc::new(AtomicUsize::new(0));
    let all_done_notify = Arc::new(Notify::new());
    let shutdown_notify = Arc::new(Notify::new());

    let tx_clone = tx.clone();
    let counter_clone = active_tasks.clone();
    let all_done_clone = all_done_notify.clone();
    let shutdown_clone = shutdown_notify.clone();
    let dir_clone = dir.clone();

    let consumer_handle = tokio::spawn(async move {
        info!("Consumer started");
        loop {
            tokio::select! {
                biased;

                _ = shutdown_clone.notified() => {
                    info!("Consumer exited due to shutdown notify");
                    break;
                }

                Some(task) = rx.recv() => {
                    let tx_clone_inner = tx_clone.clone();
                    let counter_clone_inner = counter_clone.clone();
                    let all_done_clone_inner = all_done_clone.clone();
                    let dir_clone_inner = dir_clone.clone();
                    let task_display = task.display(&dir_clone_inner);

                    let consumer_span = span!(Level::INFO, "task", task = task_display);

                    tokio::spawn(async move {
                        info!("[Task start]");

                        let current_span = tracing::Span::current();
                        let extract_result = tokio::task::spawn_blocking(move || {
                            current_span.in_scope(|| {
                                extract(task.archive_path, task.root)
                            })
                        }).await;


                        match extract_result {
                            Ok(Ok(new_tasks)) => {
                                info!("[Task Success]");
                                for new_task in new_tasks {
                                    counter_clone_inner.fetch_add(1, Ordering::SeqCst);
                                    if tx_clone_inner.send(new_task).await.is_err() {
                                        counter_clone_inner.fetch_sub(1, Ordering::SeqCst);
                                    }
                                }
                            }
                            Ok(Err(e)) => tracing::error!("[Task Failed] Extract error: {e}"),
                            Err(e) => tracing::error!("[Task Failed] Spawn blocking error: {e}"),
                        }

                        if counter_clone_inner.fetch_sub(1, Ordering::SeqCst) == 1 {
                            all_done_clone_inner.notify_one();
                        }
                    }.instrument(consumer_span));
                }

                else => {
                    info!("Consumer exited because channel is closed");
                    break;
                }
            }
        }
    });

    let initial_tasks = prepare_archives(&dir);
    debug!("init tasks: {initial_tasks:?}");

    if initial_tasks.is_empty() {
        return Ok(());
    }

    active_tasks.fetch_add(initial_tasks.len(), Ordering::SeqCst);
    for task in initial_tasks {
        if tx.send(task.clone()).await.is_err() {
            active_tasks.fetch_sub(1, Ordering::SeqCst);
        }
    }

    drop(tx);

    all_done_notify.notified().await;

    info!("Main: all tasks finished. Notify waiters.");
    shutdown_notify.notify_waiters();

    info!("Waiting for consumer to join...");
    if let Err(e) = consumer_handle.await {
        tracing::error!("Main: consumer await failed: {e}");
    }

    info!("All tasks finished, exit.");

    Ok(())
}

fn prepare_archives(target_dir: &Path) -> Vec<TaskParams> {
    let today_dir = today_dir_name(&target_dir);
    if !today_dir.exists() {
        fs::create_dir(&today_dir).unwrap();
    }

    let today_bak_dir = today_bak_dir_name(&target_dir);
    if !today_bak_dir.exists() {
        fs::create_dir(&today_bak_dir).unwrap();
    }

    let mut ret = Vec::new();

    let dir = std::fs::read_dir(target_dir).unwrap();
    for file in dir {
        let file = file.unwrap();
        let filetype = file.file_type().unwrap();
        if !filetype.is_file() {
            continue;
        }

        let filepath = file.path();
        let filetype = get_file_type(&filepath);

        if is_type_archive(filetype) {
            if filetype == FileType::Multi {
                ret.push(TaskParams {
                    archive_path: filepath.clone(),
                    root: filepath,
                });
                continue;
            }
        } else if is_type_video(filetype) {
            rename_video(&filepath, filetype);
            continue;
        } else {
            continue;
        }

        let filename = filepath.file_name().unwrap();
        let new_path = today_dir.join(filename);

        let bak_path = today_bak_dir.join(filename);

        fs::copy(&filepath, &bak_path).unwrap();
        fs::rename(filepath, &new_path).unwrap();

        ret.push(TaskParams {
            archive_path: new_path.clone(),
            root: new_path,
        });
    }

    ret
}

fn extract(path: PathBuf, root: PathBuf) -> Result<Vec<TaskParams>> {
    let file_type = get_file_type(path.as_path());

    let ret = match file_type {
        FileType::Mp4 | FileType::TS => {
            todo!()
        }
        FileType::Unknown => {
            todo!()
        }
        FileType::Rar => unrar(path, root)?,
        FileType::SevenZ => sevenz_unzip(path, root)?,
        FileType::Zip => {
            unzip(path, root)?
            //  match unzip(path.clone(), root.clone()) {
            //             Ok(ret) => ret,
            //             Err(e) => match e.downcast::<ZipError>() {
            //                 Ok(zip_error) => match zip_error {
            //                     ZipError::InvalidArchive(_) | ZipError::UnsupportedArchive(_) => {
            //                         unar(path, root)?
            //                     }
            //                     _ => return Err(zip_error.into()),
            //                 },
            //                 Err(e) => return Err(e),
            //             },
            //         }
        }
        FileType::Multi => unar(path, root)?,
    };

    Ok(ret)
}
