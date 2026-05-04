//! Concurrent task runner that drives the extraction pipeline end-to-end.
//!
//! The runner enumerates initial archives, spawns one async task per work item, and
//! recursively re-enqueues any nested archives discovered during extraction. A
//! single [`Reporter`] coordinates the visual progress bars and aggregate stats.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use tokio::sync::{Notify, mpsc};
use tracing::{debug, info};

use crate::error::AutoarcError;
use crate::extractors;
use crate::fs::{
    FileType, get_file_type, is_type_archive, is_type_video, relative_path, rename_video,
    today_bak_dir_name, today_dir_name,
};
use crate::progress::Reporter;

/// One unit of work flowing through the channel.
#[derive(Debug, Clone)]
pub struct TaskParams {
    /// Path of the archive to extract.
    pub archive_path: PathBuf,
    /// Path of the *original* top-level archive that this work item descends from
    /// (used purely for human-readable labels in the progress UI).
    pub root: PathBuf,
}

impl TaskParams {
    /// Render a label like `foo.zip <- nested.7z` relative to `dir`.
    pub fn display(&self, dir: &Path) -> String {
        let rel_archive = relative_path(dir, &self.archive_path);
        let rel_root = relative_path(dir, &self.root);
        if rel_archive == rel_root {
            rel_archive.to_string_lossy().to_string()
        } else {
            format!(
                "{} <- {}",
                rel_archive.to_string_lossy(),
                rel_root.to_string_lossy()
            )
        }
    }
}

/// Top-level entry point invoked by the binary for the `autoarc autoarc <DIR>` subcommand.
///
/// `max_depth` controls how deep the initial directory scan walks. `1` only
/// inspects the immediate contents of `dir` (the historical behaviour);
/// `usize::MAX` means unlimited. Note that this only affects the **initial**
/// scan: any nested archives produced by extraction itself are always queued
/// recursively regardless of `max_depth`.
pub async fn run(dir: PathBuf, max_depth: usize) -> Result<()> {
    let initial_tasks = prepare_archives(&dir, max_depth)?;
    debug!("initial tasks: {initial_tasks:?}");

    if initial_tasks.is_empty() {
        println!("No archives found in {}", dir.display());
        return Ok(());
    }

    let reporter = Reporter::new(initial_tasks.len());
    crate::progress::init_tracing(&reporter);

    let (tx, mut rx) = mpsc::channel::<TaskParams>(32);
    let active = Arc::new(AtomicUsize::new(0));
    let all_done = Arc::new(Notify::new());
    let shutdown = Arc::new(Notify::new());

    // Consumer loop: spawns a per-task worker for every received TaskParams.
    let consumer_tx = tx.clone();
    let consumer_active = Arc::clone(&active);
    let consumer_done = Arc::clone(&all_done);
    let consumer_shutdown = Arc::clone(&shutdown);
    let consumer_dir = dir.clone();
    let consumer_reporter = reporter.clone();

    let consumer_handle = tokio::spawn(async move {
        info!("consumer started");
        loop {
            tokio::select! {
                biased;

                _ = consumer_shutdown.notified() => {
                    info!("consumer received shutdown");
                    break;
                }

                Some(task) = rx.recv() => {
                    spawn_task(
                        task,
                        consumer_dir.clone(),
                        consumer_tx.clone(),
                        Arc::clone(&consumer_active),
                        Arc::clone(&consumer_done),
                        consumer_reporter.clone(),
                    );
                }

                else => {
                    info!("consumer channel closed");
                    break;
                }
            }
        }
    });

    // Seed the channel with the initial tasks.
    active.fetch_add(initial_tasks.len(), Ordering::SeqCst);
    for task in initial_tasks {
        if tx.send(task).await.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
    drop(tx);

    all_done.notified().await;
    info!("all tasks finished; signalling shutdown");
    shutdown.notify_waiters();

    if let Err(e) = consumer_handle.await {
        tracing::error!("consumer join failed: {e}");
    }

    reporter.finish_summary();
    Ok(())
}

/// Spawn a single async worker that runs the extractor on a blocking thread.
fn spawn_task(
    task: TaskParams,
    dir: PathBuf,
    tx: mpsc::Sender<TaskParams>,
    active: Arc<AtomicUsize>,
    all_done: Arc<Notify>,
    reporter: Reporter,
) {
    let label = task.display(&dir);
    let task_reporter = reporter.task(label.clone());

    tokio::spawn(async move {
        let archive_path = task.archive_path.clone();
        let root = task.root.clone();
        let file_type = get_file_type(&archive_path);

        let extract_result = tokio::task::spawn_blocking(move || {
            extractors::run(file_type, archive_path, root, &task_reporter).inspect(|_children| {
                task_reporter.finish_ok();
            })
        })
        .await;

        match extract_result {
            Ok(Ok(new_tasks)) => {
                reporter.task_succeeded();
                if !new_tasks.is_empty() {
                    reporter.task_added(new_tasks.len());
                    for new_task in new_tasks {
                        active.fetch_add(1, Ordering::SeqCst);
                        if tx.send(new_task).await.is_err() {
                            active.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                reporter.task_failed(&label, &e);
            }
            Err(e) => {
                reporter.task_failed(&label, &e);
            }
        }

        if active.fetch_sub(1, Ordering::SeqCst) == 1 {
            all_done.notify_one();
        }
    });
}

/// Walk the requested depth of `target_dir`, sorting files into archives,
/// videos, or noise.
///
/// At `max_depth == 1` the historical behaviour is preserved: archives are
/// copied to today's `MM-DD_bak/` and moved into today's `MM-DD/` working
/// folder. For deeper scans the date-folder ritual is skipped and archives are
/// processed in place, with our own `_out` / `MM-DD*` artefact directories
/// pruned from the walk so we never re-process previous output.
fn prepare_archives(target_dir: &Path, max_depth: usize) -> Result<Vec<TaskParams>> {
    if max_depth <= 1 {
        prepare_top_level(target_dir)
    } else {
        prepare_recursive(target_dir, max_depth)
    }
}

/// Original behaviour: scan only the top level, move archives into
/// `MM-DD/` and back them up under `MM-DD_bak/`.
fn prepare_top_level(target_dir: &Path) -> Result<Vec<TaskParams>> {
    let today = today_dir_name(target_dir);
    if !today.exists() {
        std::fs::create_dir(&today).map_err(|e| AutoarcError::io(today.clone(), e))?;
    }

    let bak = today_bak_dir_name(target_dir);
    if !bak.exists() {
        std::fs::create_dir(&bak).map_err(|e| AutoarcError::io(bak.clone(), e))?;
    }

    let mut tasks = Vec::new();

    let entries = std::fs::read_dir(target_dir)
        .map_err(|e| AutoarcError::io(target_dir.to_path_buf(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| AutoarcError::io(target_dir.to_path_buf(), e))?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }

        let filepath = entry.path();
        let kind = get_file_type(&filepath);

        if is_type_archive(kind) {
            // Multi-volume archives mustn't be moved (their `.z02`, `.z03` siblings
            // live next to the first part), so keep them in place and queue directly.
            if kind == FileType::Multi {
                tasks.push(TaskParams {
                    archive_path: filepath.clone(),
                    root: filepath,
                });
                continue;
            }
        } else if is_type_video(kind) {
            rename_video(&filepath, kind)?;
            continue;
        } else {
            continue;
        }

        let filename = filepath
            .file_name()
            .ok_or_else(|| AutoarcError::Other(format!("missing file name: {filepath:?}")))?;
        let new_path = today.join(filename);
        let bak_path = bak.join(filename);

        std::fs::copy(&filepath, &bak_path).map_err(|e| AutoarcError::io(bak_path.clone(), e))?;
        std::fs::rename(&filepath, &new_path).map_err(|e| AutoarcError::io(new_path.clone(), e))?;

        tasks.push(TaskParams {
            archive_path: new_path.clone(),
            root: new_path,
        });
    }

    Ok(tasks)
}

/// Recursive scan: walk up to `max_depth` directory levels, leaving archives
/// where they were found and emitting them as tasks directly.
///
/// Three categories of directory are pruned from the walk to avoid feedback
/// loops with previous runs:
///   * `*_out` — extraction output of any prior archive
///   * `MM-DD` — today's (or any past day's) working directory
///   * `MM-DD_bak` — today's (or any past day's) backup directory
fn prepare_recursive(target_dir: &Path, max_depth: usize) -> Result<Vec<TaskParams>> {
    use walkdir::WalkDir;

    let mut tasks = Vec::new();

    let walker = WalkDir::new(target_dir)
        .max_depth(max_depth)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                let name = e.file_name().to_string_lossy();
                !is_artefact_dir(&name)
            } else {
                true
            }
        });

    for entry in walker {
        let entry = entry.map_err(|e| {
            AutoarcError::Other(format!("walkdir error under {target_dir:?}: {e}"))
        })?;
        if !entry.file_type().is_file() {
            continue;
        }

        let filepath = entry.into_path();
        let kind = get_file_type(&filepath);

        if is_type_archive(kind) {
            tasks.push(TaskParams {
                archive_path: filepath.clone(),
                root: filepath,
            });
        } else if is_type_video(kind) {
            rename_video(&filepath, kind)?;
        }
    }

    Ok(tasks)
}

/// Recognise our own output / backup / working directories so the recursive
/// scan never re-processes them.
fn is_artefact_dir(name: &str) -> bool {
    if name.ends_with("_out") {
        return true;
    }
    let stripped = name.strip_suffix("_bak").unwrap_or(name);
    is_md_date(stripped)
}

/// Match the `MM-DD` pattern produced by [`today_dir_name`].
fn is_md_date(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 5
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2] == b'-'
        && bytes[3].is_ascii_digit()
        && bytes[4].is_ascii_digit()
}
