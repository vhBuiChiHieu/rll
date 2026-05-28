// Directory entry model, hidden-file filter, and the parallel work-stealing
// recursive directory sizer. Std-only; no crate dependencies.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, DirEntry, FileType};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

// A leading '.' is the cross-platform dotfile convention. Caller passes a borrow
// of the already-allocated OsStr so this check itself adds no allocation.
pub(crate) fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

pub(crate) struct EntryItem {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) type_name: &'static str,
    // Pre-computed length for files captured while the DirEntry is still alive,
    // so Windows reuses the FindNextFile data and avoids a per-file stat call.
    pub(crate) size_hint: Option<u64>,
}

impl EntryItem {
    // Caller supplies the OsString returned by `DirEntry::file_name()` so we
    // pay the std-mandated allocation exactly once per entry, regardless of the
    // hidden-filter check, error-warning paths, or metadata branch.
    pub(crate) fn from_entry<E>(
        entry: DirEntry,
        file_name: OsString,
        stderr: &mut E,
    ) -> Option<Self>
    where
        E: Write,
    {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                let _ = writeln!(
                    stderr,
                    "warning: cannot read file type for {:?}: {err}",
                    file_name
                );
                return Some(Self {
                    path: entry.path(),
                    name: file_name.to_string_lossy().into_owned(),
                    type_name: "OTHER",
                    size_hint: None,
                });
            }
        };

        let type_name = entry_type(file_type);
        let size_hint = if type_name == "FILE" {
            match entry.metadata() {
                Ok(metadata) => Some(metadata.len()),
                Err(err) => {
                    let _ = writeln!(
                        stderr,
                        "warning: cannot read metadata for {:?}: {err}",
                        file_name
                    );
                    None
                }
            }
        } else {
            None
        };

        Some(Self {
            path: entry.path(),
            name: file_name.to_string_lossy().into_owned(),
            type_name,
            size_hint,
        })
    }
}

pub(crate) struct DirectoryResult {
    pub(crate) item: EntryItem,
    pub(crate) size: u64,
    pub(crate) warnings: Vec<String>,
}

#[derive(Default, Clone, Copy)]
pub(crate) struct Summary {
    pub(crate) files: u64,
    pub(crate) dirs: u64,
    pub(crate) others: u64,
}

impl Summary {
    pub(crate) fn total(&self) -> u64 {
        self.files + self.dirs + self.others
    }
}

struct ScanTask {
    // Index into the top-level job slice; identifies which directory this work belongs to
    // so subtree sums and warnings stay attributable after work-stealing.
    job_id: usize,
    path: PathBuf,
}

struct ScanInner {
    queue: Vec<ScanTask>,
    active: usize,
}

struct ScanState {
    // Single shared deque + Condvar lets idle workers steal subdirs pushed by busy workers,
    // so one giant top-level dir does not starve the rest of the pool.
    inner: Mutex<ScanInner>,
    cv: Condvar,
    // Per-job total updated lock-free by workers as they sum file sizes.
    totals: Vec<AtomicU64>,
    // Aggregate nested entry counts across all jobs; feeds the final recursive summary.
    nested_files: AtomicU64,
    nested_dirs: AtomicU64,
    nested_others: AtomicU64,
    // Per-job warning bucket; only locked when a worker has warnings to flush.
    warnings: Mutex<Vec<Vec<String>>>,
    // Mirror of Options::show_all so workers can skip dotfile subtrees too.
    show_all: bool,
}

#[derive(Default, Clone, Copy)]
pub(crate) struct NestedCounts {
    pub(crate) files: u64,
    pub(crate) dirs: u64,
    pub(crate) others: u64,
}

pub(crate) struct ParallelScan {
    pub(crate) results: Vec<DirectoryResult>,
    pub(crate) nested: NestedCounts,
}

pub(crate) fn scan_directories_parallel(jobs: Vec<EntryItem>, show_all: bool) -> ParallelScan {
    if jobs.is_empty() {
        return ParallelScan {
            results: Vec::new(),
            nested: NestedCounts::default(),
        };
    }

    let job_count = jobs.len();
    let mut initial_queue = Vec::with_capacity(job_count);
    let mut totals = Vec::with_capacity(job_count);
    let mut warnings_buckets = Vec::with_capacity(job_count);
    for (id, item) in jobs.iter().enumerate() {
        initial_queue.push(ScanTask {
            job_id: id,
            path: item.path.clone(),
        });
        totals.push(AtomicU64::new(0));
        warnings_buckets.push(Vec::new());
    }

    let state = Arc::new(ScanState {
        inner: Mutex::new(ScanInner {
            queue: initial_queue,
            active: 0,
        }),
        cv: Condvar::new(),
        totals,
        nested_files: AtomicU64::new(0),
        nested_dirs: AtomicU64::new(0),
        nested_others: AtomicU64::new(0),
        warnings: Mutex::new(warnings_buckets),
        show_all,
    });

    // Pool can exceed top-level job count because work-stealing generates more tasks
    // as subdirectories are discovered.
    let worker_count = worker_count();
    let mut workers = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let state = Arc::clone(&state);
        workers.push(thread::spawn(move || worker_loop(state)));
    }
    for worker in workers {
        let _ = worker.join();
    }

    let state = Arc::try_unwrap(state)
        .ok()
        .expect("worker threads must release shared state before join returns");
    let warnings = state.warnings.into_inner().unwrap();
    let totals: Vec<u64> = state
        .totals
        .into_iter()
        .map(|atomic| atomic.into_inner())
        .collect();
    let nested = NestedCounts {
        files: state.nested_files.into_inner(),
        dirs: state.nested_dirs.into_inner(),
        others: state.nested_others.into_inner(),
    };

    let mut totals_iter = totals.into_iter();
    let mut warnings_iter = warnings.into_iter();
    let results = jobs
        .into_iter()
        .map(|item| DirectoryResult {
            item,
            size: totals_iter.next().expect("totals aligned with jobs"),
            warnings: warnings_iter.next().expect("warnings aligned with jobs"),
        })
        .collect();

    ParallelScan { results, nested }
}

fn worker_loop(state: Arc<ScanState>) {
    loop {
        // Acquire next task or exit cleanly when no work remains and no peer is busy.
        let task = {
            let mut inner = state.inner.lock().unwrap();
            loop {
                if let Some(task) = inner.queue.pop() {
                    inner.active += 1;
                    break Some(task);
                }
                if inner.active == 0 {
                    // Nobody can produce more work; wake any remaining waiters so they exit too.
                    state.cv.notify_all();
                    break None;
                }
                inner = state.cv.wait(inner).unwrap();
            }
        };

        let Some(task) = task else {
            return;
        };

        let job_id = task.job_id;
        let level = scan_one_level(task, state.show_all);

        if level.total_size > 0 {
            state.totals[job_id].fetch_add(level.total_size, AtomicOrdering::Relaxed);
        }
        if level.files > 0 {
            state
                .nested_files
                .fetch_add(level.files, AtomicOrdering::Relaxed);
        }
        if level.dirs > 0 {
            state
                .nested_dirs
                .fetch_add(level.dirs, AtomicOrdering::Relaxed);
        }
        if level.others > 0 {
            state
                .nested_others
                .fetch_add(level.others, AtomicOrdering::Relaxed);
        }
        if !level.warnings.is_empty() {
            let mut warnings = state.warnings.lock().unwrap();
            warnings[job_id].extend(level.warnings);
        }

        let mut inner = state.inner.lock().unwrap();
        inner.active -= 1;
        let pushed = !level.subdirs.is_empty();
        if pushed {
            inner.queue.extend(level.subdirs);
            // Wake idle workers to steal the newly discovered subdirs.
            state.cv.notify_all();
        } else if inner.active == 0 && inner.queue.is_empty() {
            // Last worker finishing with nothing left; let everyone exit.
            state.cv.notify_all();
        }
    }
}

struct LevelScan {
    total_size: u64,
    files: u64,
    dirs: u64,
    others: u64,
    subdirs: Vec<ScanTask>,
    warnings: Vec<String>,
}

fn scan_one_level(task: ScanTask, show_all: bool) -> LevelScan {
    let mut total = 0_u64;
    let mut files = 0_u64;
    let mut dirs = 0_u64;
    let mut others = 0_u64;
    // Pre-size subdir buffer to skip the first few Vec growth reallocs on dense dirs;
    // warnings stay default-sized because they are rare on a healthy filesystem.
    let mut subdirs = Vec::with_capacity(8);
    let mut warnings: Vec<String> = Vec::new();

    let entries = match fs::read_dir(&task.path) {
        Ok(entries) => entries,
        Err(err) => {
            warnings.push(format!(
                "warning: cannot read directory {:?}: {err}",
                task.path
            ));
            return LevelScan {
                total_size: 0,
                files: 0,
                dirs: 0,
                others: 0,
                subdirs,
                warnings,
            };
        }
    };

    // Scan a single directory level; subdirs are returned for the shared queue rather
    // than recursed into locally, so peer workers can steal them.
    for entry_result in entries {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(err) => {
                warnings.push(format!("warning: cannot read directory entry: {err}"));
                continue;
            }
        };

        // Honor --a by skipping nested dotfile entries too, so reported sizes
        // and counts match the visible top-level filter.
        if !show_all && is_hidden(&entry.file_name()) {
            continue;
        }

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                warnings.push(format!(
                    "warning: cannot read file type for {:?}: {err}",
                    entry.path()
                ));
                continue;
            }
        };

        if file_type.is_dir() {
            dirs += 1;
            subdirs.push(ScanTask {
                job_id: task.job_id,
                path: entry.path(),
            });
        } else if file_type.is_file() {
            files += 1;
            match entry.metadata() {
                Ok(metadata) => total = total.saturating_add(metadata.len()),
                Err(err) => warnings.push(format!(
                    "warning: cannot read metadata for {:?}: {err}",
                    entry.path()
                )),
            }
        } else {
            others += 1;
        }
    }

    LevelScan {
        total_size: total,
        files,
        dirs,
        others,
        subdirs,
        warnings,
    }
}

fn worker_count() -> usize {
    // Allow operators to override the worker pool size via RLL_WORKERS.
    // Useful for tuning between SSD (more threads) and HDD (fewer threads).
    if let Ok(raw) = env::var("RLL_WORKERS") {
        if let Ok(parsed) = raw.trim().parse::<usize>() {
            if parsed >= 1 {
                return parsed;
            }
        }
    }

    // Directory traversal is I/O-bound, so default to the full hardware parallelism
    // hint instead of half — multiple in-flight directory reads overlap latency on SSDs.
    thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .max(1)
}

fn entry_type(file_type: FileType) -> &'static str {
    if file_type.is_file() {
        "FILE"
    } else if file_type.is_dir() {
        "DIR"
    } else {
        "OTHER"
    }
}
