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
///
/// `dry_run` prints the planned work and exits without touching the filesystem.
/// `yes` skips the interactive `[y/N]` confirmation prompt (the prompt is also
/// skipped automatically when stdin is not a TTY, e.g. in CI).
pub async fn run(dir: PathBuf, max_depth: usize, dry_run: bool, yes: bool) -> Result<()> {
    use std::io::IsTerminal;

    // Phase 1 — scan: pure read pass, classifies every file.
    let scan_result = scan(&dir, max_depth)?;

    // Phase 2 — plan: fuse multi-volume parts into single logical entries.
    let plan = build_plan(scan_result.archives);

    if plan.is_empty() && scan_result.videos.is_empty() {
        println!("No archives or videos found in {}", dir.display());
        return Ok(());
    }

    // Phase 3 — render: show the user what will happen.
    print_plan(&plan, &scan_result.videos, &dir, max_depth);

    if dry_run {
        return Ok(());
    }

    if !yes && std::io::stdin().is_terminal() && !prompt_continue()? {
        println!("Aborted.");
        return Ok(());
    }

    // Phase 4 — execute: do the moves and produce TaskParams.
    let initial_tasks = execute(plan, scan_result.videos, &dir, max_depth)?;
    debug!("initial tasks: {initial_tasks:?}");

    if initial_tasks.is_empty() {
        // All work was video renames; nothing left to extract.
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

// ============================================================================
// Phase 1: scan — pure read pass that classifies files without touching them.
// ============================================================================

/// One archive file discovered during the scan.
#[derive(Debug, Clone)]
struct ScanItem {
    path: PathBuf,
    #[allow(dead_code)] // kept for future kind-aware grouping; currently re-derived at execute time.
    kind: FileType,
}

/// Outcome of [`scan`]: archives that need extraction + videos that need a rename.
struct ScanResult {
    archives: Vec<ScanItem>,
    videos: Vec<(PathBuf, FileType)>,
}

/// Walk `target_dir` (respecting `max_depth`) and classify every file.
///
/// This pass performs **no filesystem mutations** so it is safe to run in
/// dry-run mode and to surface to the user for confirmation.
fn scan(target_dir: &Path, max_depth: usize) -> Result<ScanResult> {
    if max_depth <= 1 {
        scan_top_level(target_dir)
    } else {
        scan_recursive(target_dir, max_depth)
    }
}

/// Top-level scan: only the immediate contents of `target_dir`.
fn scan_top_level(target_dir: &Path) -> Result<ScanResult> {
    let mut result = ScanResult {
        archives: Vec::new(),
        videos: Vec::new(),
    };

    let entries = std::fs::read_dir(target_dir)
        .map_err(|e| AutoarcError::io(target_dir.to_path_buf(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| AutoarcError::io(target_dir.to_path_buf(), e))?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let kind = get_file_type(&path);
        if is_type_archive(kind) {
            result.archives.push(ScanItem { path, kind });
        } else if is_type_video(kind) {
            result.videos.push((path, kind));
        }
    }
    Ok(result)
}

/// Recursive scan: walk up to `max_depth` directory levels, pruning our own
/// `*_out` / `MM-DD` / `MM-DD_bak` artefact directories from the walk.
fn scan_recursive(target_dir: &Path, max_depth: usize) -> Result<ScanResult> {
    use walkdir::WalkDir;

    let mut result = ScanResult {
        archives: Vec::new(),
        videos: Vec::new(),
    };

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
        let path = entry.into_path();
        let kind = get_file_type(&path);
        if is_type_archive(kind) {
            result.archives.push(ScanItem { path, kind });
        } else if is_type_video(kind) {
            result.videos.push((path, kind));
        }
    }
    Ok(result)
}

// ============================================================================
// Phase 2: plan — group multi-volume parts into single logical entries.
// ============================================================================

/// One row in the user-facing extraction plan.
#[derive(Debug, Clone)]
struct PlanItem {
    /// The path that will be passed to the extractor (e.g. the `.z01` for
    /// ZIP multi-volume sets).
    primary: PathBuf,
    /// All filesystem files belonging to this logical archive (≥ 1).
    parts: Vec<PathBuf>,
    /// Total bytes across all `parts`.
    total_size: u64,
    /// True when `parts.len() > 1` — the archive spans multiple volume files.
    is_multi_volume: bool,
}

/// Group scan items into plan items, fusing multi-volume sets into a single
/// row and de-duplicating sibling parts that the scan classified independently
/// (e.g. both `foo.zip` and `foo.z01` showing up as separate ScanItems).
fn build_plan(archives: Vec<ScanItem>) -> Vec<PlanItem> {
    use std::collections::HashSet;

    let mut absorbed: HashSet<PathBuf> = HashSet::new();
    let mut plan = Vec::new();

    // Sort for deterministic plan output.
    let mut sorted = archives;
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    for item in &sorted {
        if absorbed.contains(&item.path) {
            continue;
        }

        if let Some(parts) = discover_volume_parts(&item.path) {
            // Pick the primary the runtime should hand to the extractor.
            // Preference order: .z01 > .001 > .zip > whatever sorted first.
            let primary = parts
                .iter()
                .find(|p| has_ext(p, "z01"))
                .or_else(|| parts.iter().find(|p| has_ext(p, "001")))
                .or_else(|| parts.iter().find(|p| has_ext(p, "zip")))
                .cloned()
                .unwrap_or_else(|| parts[0].clone());
            let total_size: u64 = parts
                .iter()
                .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
                .sum();
            for p in &parts {
                absorbed.insert(p.clone());
            }
            plan.push(PlanItem {
                primary,
                parts,
                total_size,
                is_multi_volume: true,
            });
        } else {
            let size = std::fs::metadata(&item.path).map(|m| m.len()).unwrap_or(0);
            absorbed.insert(item.path.clone());
            plan.push(PlanItem {
                primary: item.path.clone(),
                parts: vec![item.path.clone()],
                total_size: size,
                is_multi_volume: false,
            });
        }
    }

    plan
}

/// If `primary` looks like one part of a multi-volume set, scan its parent
/// directory for siblings and return the full part list (including `primary`).
///
/// Returns `None` for solo archives (the caller should treat them as standalone).
fn discover_volume_parts(primary: &Path) -> Option<Vec<PathBuf>> {
    let parent = primary.parent()?;
    let name = primary.file_name()?.to_str()?;
    let lower = name.to_ascii_lowercase();

    // ZIP-style multi-volume: foo.zip + foo.z01 + foo.z02 + ... + foo.zNN.
    let zip_stem_len = lower
        .strip_suffix(".zip")
        .or_else(|| lower.strip_suffix(".z01"))
        .map(|s| s.len());
    if let Some(stem_len) = zip_stem_len {
        let stem_orig = &name[..stem_len];
        let mut parts = Vec::new();
        let zip_path = parent.join(format!("{stem_orig}.zip"));
        if zip_path.exists() {
            parts.push(zip_path);
        }
        for n in 1..=99 {
            let z = parent.join(format!("{stem_orig}.z{n:02}"));
            if z.exists() {
                parts.push(z);
            } else if n > 1 {
                break;
            }
        }
        if parts.len() > 1 {
            return Some(parts);
        }
    }

    // Generic numeric splits: foo.001 + foo.002 + ..., or foo.7z.001 + foo.7z.002 + ...
    if let Some(stem_len) = lower.strip_suffix(".001").map(|s| s.len()) {
        let stem_orig = &name[..stem_len];
        let mut parts = Vec::new();
        for n in 1..=999 {
            let p = parent.join(format!("{stem_orig}.{n:03}"));
            if p.exists() {
                parts.push(p);
            } else if n > 1 {
                break;
            }
        }
        if parts.len() > 1 {
            return Some(parts);
        }
    }

    None
}

/// Case-insensitive extension check.
fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

// ============================================================================
// Phase 3: render — print the plan and (optionally) prompt the user.
// ============================================================================

/// Print a human-readable extraction plan to stdout.
fn print_plan(
    plan: &[PlanItem],
    videos: &[(PathBuf, FileType)],
    dir: &Path,
    max_depth: usize,
) {
    use console::style;
    use indicatif::HumanBytes;

    let multi_count = plan.iter().filter(|p| p.is_multi_volume).count();
    let total_bytes: u64 = plan.iter().map(|p| p.total_size).sum();
    let depth_note = if max_depth == usize::MAX {
        "recursive".to_string()
    } else {
        format!("depth={max_depth}")
    };

    println!(
        "{} {} archives ({} multi-volume), {} total \u{2014} {} ({})",
        style("Plan:").bold().cyan(),
        plan.len(),
        multi_count,
        HumanBytes(total_bytes),
        dir.display(),
        depth_note,
    );

    let max_label = plan
        .iter()
        .map(|p| relative_path(dir, &p.primary).to_string_lossy().chars().count())
        .max()
        .unwrap_or(0);

    for item in plan {
        let kind_tag = match get_file_type(&item.primary) {
            FileType::Zip => "zip",
            FileType::Rar => "rar",
            FileType::SevenZ => "7z",
            FileType::Multi => "multi",
            _ => "?",
        };
        let rel = relative_path(dir, &item.primary);
        let label = rel.to_string_lossy();
        let pad = max_label.saturating_sub(label.chars().count());
        let spacer = " ".repeat(pad);
        let suffix = if item.is_multi_volume {
            format!(", {} parts", item.parts.len())
        } else {
            String::new()
        };
        println!(
            "  [{:<5}] {}{}  ({}{})",
            style(kind_tag).yellow(),
            label,
            spacer,
            HumanBytes(item.total_size),
            suffix,
        );
    }

    if !videos.is_empty() {
        println!(
            "\n{} {} video file(s) will be renamed in place.",
            style("Note:").dim(),
            videos.len()
        );
    }
    println!();
}

/// Read a y/N answer from stdin. Returns `Ok(true)` only when the user typed
/// `y` or `yes` (case-insensitive). Anything else (including just pressing
/// Enter) defaults to `false`.
fn prompt_continue() -> Result<bool> {
    use std::io::{BufRead, Write};

    print!("Continue? [y/N] ");
    std::io::stdout()
        .flush()
        .map_err(|e| AutoarcError::Other(format!("flush stdout: {e}")))?;

    let mut buf = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut buf)
        .map_err(|e| AutoarcError::Other(format!("read stdin: {e}")))?;

    Ok(matches!(
        buf.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

// ============================================================================
// Phase 4: execute — mutate the filesystem and produce TaskParams.
// ============================================================================

/// Apply the plan: move/back-up archives as appropriate, rename videos, and
/// emit one [`TaskParams`] per logical archive for the runner to consume.
fn execute(
    plan: Vec<PlanItem>,
    videos: Vec<(PathBuf, FileType)>,
    dir: &Path,
    max_depth: usize,
) -> Result<Vec<TaskParams>> {
    let tasks = if max_depth <= 1 {
        execute_top_level(plan, dir)?
    } else {
        execute_recursive(plan)
    };

    for (path, kind) in videos {
        rename_video(&path, kind)?;
    }

    Ok(tasks)
}

/// Top-level mode (depth=1): copy each non-multi archive to `MM-DD_bak/` and
/// move it to `MM-DD/`. Multi-volume entries (and any solo `FileType::Multi`)
/// are kept in place so their sibling parts remain reachable.
fn execute_top_level(plan: Vec<PlanItem>, dir: &Path) -> Result<Vec<TaskParams>> {
    let today = today_dir_name(dir);
    if !today.exists() {
        std::fs::create_dir(&today).map_err(|e| AutoarcError::io(today.clone(), e))?;
    }

    let bak = today_bak_dir_name(dir);
    if !bak.exists() {
        std::fs::create_dir(&bak).map_err(|e| AutoarcError::io(bak.clone(), e))?;
    }

    let mut tasks = Vec::new();
    for item in plan {
        let primary_kind = get_file_type(&item.primary);

        // Multi-volume sets and standalone Multi-classified files stay in place;
        // moving them would orphan their sibling parts.
        if item.is_multi_volume || primary_kind == FileType::Multi {
            tasks.push(TaskParams {
                archive_path: item.primary.clone(),
                root: item.primary,
            });
            continue;
        }

        // Solo single-file archive: copy to _bak, move to today's dir.
        let filename = item
            .primary
            .file_name()
            .ok_or_else(|| AutoarcError::Other(format!("missing file name: {:?}", item.primary)))?;
        let new_path = today.join(filename);
        let bak_path = bak.join(filename);

        std::fs::copy(&item.primary, &bak_path)
            .map_err(|e| AutoarcError::io(bak_path.clone(), e))?;
        std::fs::rename(&item.primary, &new_path)
            .map_err(|e| AutoarcError::io(new_path.clone(), e))?;

        tasks.push(TaskParams {
            archive_path: new_path.clone(),
            root: new_path,
        });
    }

    Ok(tasks)
}

/// Recursive mode (depth>1): no movement, just emit one task per plan item.
fn execute_recursive(plan: Vec<PlanItem>) -> Vec<TaskParams> {
    plan.into_iter()
        .map(|item| TaskParams {
            archive_path: item.primary.clone(),
            root: item.primary,
        })
        .collect()
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
