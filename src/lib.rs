use std::cmp::Ordering;
use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, DirEntry, FileType, ReadDir};
use std::io::{self, BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

mod tui;

const HEADER: &str = "TYPE  SIZE       NAME\n";

pub fn run_stdio() -> u8 {
    let stdout = io::stdout();
    let stderr = io::stderr();
    run_with_args(env::args().skip(1), stdout.lock(), stderr.lock())
}

pub fn run<W, E>(stdout: W, stderr: E) -> u8
where
    W: Write,
    E: Write,
{
    run_with_args(std::iter::empty::<&str>(), stdout, stderr)
}

pub fn run_with_args<I, S, W, E>(args: I, stdout: W, mut stderr: E) -> u8
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    W: Write,
    E: Write,
{
    let parsed = match Options::parse(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            let _ = writeln!(stderr, "error: {err}");
            return 1;
        }
    };

    match parsed.mode {
        Mode::Cli => run_path(".", parsed, stdout, stderr),
        Mode::Tui => tui::run(parsed.show_all),
    }
}

fn run_path<P, W, E>(path: P, options: Options, stdout: W, mut stderr: E) -> u8
where
    P: AsRef<Path>,
    W: Write,
    E: Write,
{
    match fs::read_dir(path) {
        Ok(entries) => write_entries(stdout, &mut stderr, entries, options),
        Err(err) => {
            let _ = writeln!(stderr, "error: cannot read current directory: {err}");
            1
        }
    }
}

pub fn format_size(bytes: u64) -> String {
    Size(bytes).to_string()
}

#[derive(Clone, Copy, Default)]
pub(crate) enum Mode {
    #[default]
    Cli,
    Tui,
}

#[derive(Clone, Copy, Default)]
struct Options {
    order: Option<SortOrder>,
    // Include dotfile entries in top-level listing and recursive sizing.
    show_all: bool,
    // Cap the number of rows printed after sorting/collection.
    top_n: Option<usize>,
    // Emit NDJSON lines instead of the human table.
    json: bool,
    // Subcommand routing; default is the CLI listing path.
    mode: Mode,
}

impl Options {
    fn parse<I, S>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut options = Self::default();
        let mut args = args.into_iter().peekable();

        // First positional token before any flag may select the TUI mode.
        if let Some(first) = args.peek() {
            if first.as_ref() == "tui" {
                options.mode = Mode::Tui;
                let _ = args.next();
            }
        }

        while let Some(arg) = args.next() {
            match arg.as_ref() {
                "--o" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--o requires asc or desc".to_owned())?;
                    options.order = Some(SortOrder::parse(value.as_ref())?);
                }
                "--a" | "--all" => {
                    options.show_all = true;
                }
                "--n" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--n requires a positive integer".to_owned())?;
                    let parsed: usize = value
                        .as_ref()
                        .parse()
                        .map_err(|_| "--n requires a positive integer".to_owned())?;
                    if parsed == 0 {
                        return Err("--n requires a positive integer".to_owned());
                    }
                    options.top_n = Some(parsed);
                }
                "--json" => {
                    options.json = true;
                }
                unknown => return Err(format!("unknown option: {unknown}")),
            }
        }

        Ok(options)
    }

    // Stream rows as scanned when no sort or top-N limit is requested; otherwise
    // buffer everything so we can order and truncate before emitting.
    fn buffer_rows(&self) -> bool {
        self.order.is_some() || self.top_n.is_some()
    }

    // Effective sort order. `--n` without explicit `--o` implies `desc` so the
    // "top N biggest" reading matches what callers expect from `head` over `du`.
    // Without this, truncation could drop the directory rows that arrive last
    // from the recursive scan, leaving the printed table inconsistent with TOTAL.
    fn effective_order(&self) -> Option<SortOrder> {
        match (self.order, self.top_n) {
            (Some(order), _) => Some(order),
            (None, Some(_)) => Some(SortOrder::Desc),
            (None, None) => None,
        }
    }
}

// A leading '.' is the cross-platform dotfile convention. Caller passes a borrow
// of the already-allocated OsStr so this check itself adds no allocation.
pub(crate) fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

#[derive(Clone, Copy)]
enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "asc" => Ok(Self::Asc),
            "desc" => Ok(Self::Desc),
            _ => Err("--o requires asc or desc".to_owned()),
        }
    }
}

struct Size(u64);

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const KIB: u64 = 1024;
        const MIB: u64 = KIB * 1024;
        const GIB: u64 = MIB * 1024;

        match self.0 {
            0..=1023 => write!(f, "{} B", self.0),
            KIB..=1_048_575 => write!(f, "{:.1} KiB", self.0 as f64 / KIB as f64),
            MIB..=1_073_741_823 => write!(f, "{:.1} MiB", self.0 as f64 / MIB as f64),
            _ => write!(f, "{:.1} GiB", self.0 as f64 / GIB as f64),
        }
    }
}

fn write_entries<W, E>(stdout: W, stderr: &mut E, entries: ReadDir, options: Options) -> u8
where
    W: Write,
    E: Write,
{
    let start = Instant::now();
    let mut summary = Summary::default();
    let mut out = BufWriter::new(stdout);
    let mut rows = Vec::new();
    let mut dir_jobs = Vec::new();
    let buffered = options.buffer_rows();

    // Table mode prints a header before scanning; JSON mode emits nothing
    // until entries are produced so downstream parsers see only NDJSON.
    if !options.json {
        if let Err(err) = out.write_all(HEADER.as_bytes()) {
            return write_error_code(err);
        }
        if buffered {
            // Flush the header up front so stderr warnings cannot land before it
            // while we collect rows for sorting/truncation.
            if let Err(err) = out.flush() {
                return write_error_code(err);
            }
        }
    }

    for entry_result in entries {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(err) => {
                let _ = writeln!(stderr, "warning: cannot read directory entry: {err}");
                continue;
            }
        };

        // Single allocation point for the entry name; reused for the hidden-filter
        // check and threaded into `EntryItem::from_entry` so it is never called twice.
        let file_name = entry.file_name();
        if !options.show_all && is_hidden(&file_name) {
            continue;
        }

        let item = match EntryItem::from_entry(entry, file_name, stderr) {
            Some(item) => item,
            None => continue,
        };

        match item.type_name {
            "FILE" => {
                summary.files += 1;
                // Reuse cached metadata captured during directory enumeration
                // to avoid a redundant stat/CreateFile syscall per file.
                let size = item.size_hint;
                let row = OutputRow::new(item, size);
                if buffered {
                    rows.push(row);
                } else if let Err(err) = emit_row(&mut out, &row, options.json) {
                    return write_error_code(err);
                }
            }
            "DIR" => {
                summary.dirs += 1;
                dir_jobs.push(item);
            }
            _ => {
                summary.others += 1;
                let row = OutputRow::unknown(item);
                if buffered {
                    rows.push(row);
                } else if let Err(err) = emit_row(&mut out, &row, options.json) {
                    return write_error_code(err);
                }
            }
        }
    }

    let scan = scan_directories_parallel(dir_jobs, options.show_all);
    // Fold nested counts into the summary so the final line reports every entry
    // touched during the recursive scan, not just direct children.
    summary.files += scan.nested.files;
    summary.dirs += scan.nested.dirs;
    summary.others += scan.nested.others;

    for result in scan.results {
        for warning in result.warnings {
            let _ = writeln!(stderr, "{warning}");
        }

        let row = OutputRow::new(result.item, Some(result.size));
        if buffered {
            rows.push(row);
        } else if let Err(err) = emit_row(&mut out, &row, options.json) {
            return write_error_code(err);
        }
    }

    if buffered {
        if let Some(order) = options.effective_order() {
            rows.sort_by(|left, right| compare_rows(left, right, order));
        }
        // Apply --n after sorting so callers get the "top N by size" pairing.
        if let Some(limit) = options.top_n {
            rows.truncate(limit);
        }
        for row in rows {
            if let Err(err) = emit_row(&mut out, &row, options.json) {
                return write_error_code(err);
            }
        }
    }

    let summary_result = if options.json {
        write_summary_json(&mut out, summary, start.elapsed())
    } else {
        write_summary(&mut out, summary, start.elapsed())
    };
    if let Err(err) = summary_result {
        return write_error_code(err);
    }

    match out.flush() {
        Ok(()) => 0,
        Err(err) => write_error_code(err),
    }
}

fn emit_row<W>(out: &mut W, row: &OutputRow, json: bool) -> io::Result<()>
where
    W: Write,
{
    if json {
        write_json_row(out, row)
    } else {
        write_item_row(out, row)
    }
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
        file_name: std::ffi::OsString,
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

struct OutputRow {
    item: EntryItem,
    size: Option<u64>,
}

impl OutputRow {
    fn new(item: EntryItem, size: Option<u64>) -> Self {
        Self { item, size }
    }

    fn unknown(item: EntryItem) -> Self {
        Self { item, size: None }
    }

    fn display_size(&self) -> String {
        self.size.map(format_size).unwrap_or_else(|| "?".to_owned())
    }
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

fn write_item_row<W>(out: &mut W, row: &OutputRow) -> io::Result<()>
where
    W: Write,
{
    writeln!(
        out,
        "{type_name:<5} {size:<10} {name}",
        type_name = row.item.type_name,
        size = row.display_size(),
        name = row.item.name
    )
}

fn compare_rows(left: &OutputRow, right: &OutputRow, order: SortOrder) -> Ordering {
    let left_key = left.size.unwrap_or(u64::MAX);
    let right_key = right.size.unwrap_or(u64::MAX);
    let ordering = left_key
        .cmp(&right_key)
        .then_with(|| left.item.name.cmp(&right.item.name));

    match order {
        SortOrder::Asc => ordering,
        SortOrder::Desc => ordering.reverse(),
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

fn write_summary<W>(out: &mut W, summary: Summary, elapsed: Duration) -> io::Result<()>
where
    W: Write,
{
    writeln!(
        out,
        "TOTAL {} entries ({} files, {} dirs, {} other) in {}",
        summary.total(),
        summary.files,
        summary.dirs,
        summary.others,
        format_duration(elapsed)
    )
}

fn write_json_row<W>(out: &mut W, row: &OutputRow) -> io::Result<()>
where
    W: Write,
{
    out.write_all(b"{\"type\":\"")?;
    out.write_all(row.item.type_name.as_bytes())?;
    out.write_all(b"\",\"name\":")?;
    write_json_string(out, &row.item.name)?;
    match row.size {
        Some(size) => writeln!(out, ",\"size\":{size}}}"),
        None => writeln!(out, ",\"size\":null}}"),
    }
}

fn write_summary_json<W>(out: &mut W, summary: Summary, elapsed: Duration) -> io::Result<()>
where
    W: Write,
{
    writeln!(
        out,
        "{{\"summary\":{{\"entries\":{},\"files\":{},\"dirs\":{},\"other\":{},\"duration_ns\":{}}}}}",
        summary.total(),
        summary.files,
        summary.dirs,
        summary.others,
        elapsed.as_nanos()
    )
}

// Minimal JSON string escaper for entry names. Names come through to_string_lossy,
// so the input is always valid UTF-8; we only escape characters required by RFC 8259.
fn write_json_string<W>(out: &mut W, value: &str) -> io::Result<()>
where
    W: Write,
{
    out.write_all(b"\"")?;
    for ch in value.chars() {
        match ch {
            '"' => out.write_all(b"\\\"")?,
            '\\' => out.write_all(b"\\\\")?,
            '\n' => out.write_all(b"\\n")?,
            '\r' => out.write_all(b"\\r")?,
            '\t' => out.write_all(b"\\t")?,
            '\u{08}' => out.write_all(b"\\b")?,
            '\u{0c}' => out.write_all(b"\\f")?,
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32)?,
            c => {
                let mut buf = [0_u8; 4];
                out.write_all(c.encode_utf8(&mut buf).as_bytes())?;
            }
        }
    }
    out.write_all(b"\"")
}

pub(crate) fn format_duration(duration: Duration) -> String {
    let nanos = duration.as_nanos();

    if nanos < 1_000 {
        format!("{nanos} ns")
    } else if nanos < 1_000_000 {
        format!("{:.3} µs", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.3} ms", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.3} s", nanos as f64 / 1_000_000_000.0)
    }
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

fn write_error_code(err: io::Error) -> u8 {
    if err.kind() == ErrorKind::BrokenPipe {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::{format_size, run_path, write_json_string, Options};
    use std::io::{self, Write};

    struct FailingWriter(io::ErrorKind);

    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(self.0, "write failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn formats_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(912), "912 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn formats_kib_mib_gib() {
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(20 * 1024 * 1024 + 314_572), "20.3 MiB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn unreadable_directory_returns_exit_code_one() {
        let mut stderr = Vec::new();
        let code = run_path(
            "path-that-should-not-exist-for-rll-test",
            Options::default(),
            Vec::new(),
            &mut stderr,
        );

        assert_eq!(code, 1);
        assert!(String::from_utf8(stderr)
            .unwrap()
            .contains("error: cannot read current directory:"));
    }

    #[test]
    fn broken_pipe_returns_exit_code_zero() {
        let code = run_path(
            ".",
            Options::default(),
            FailingWriter(io::ErrorKind::BrokenPipe),
            Vec::new(),
        );

        assert_eq!(code, 0);
    }

    #[test]
    fn json_string_escapes_required_characters() {
        let mut buf = Vec::new();
        write_json_string(&mut buf, "a\"b\\c\nd\te\u{0001}f").unwrap();
        let actual = String::from_utf8(buf).unwrap();
        let expected = String::from("\"a\\\"b\\\\c\\nd\\te\\u0001f\"");
        assert_eq!(actual, expected);
    }

    #[test]
    fn non_broken_pipe_write_error_returns_exit_code_one() {
        let code = run_path(
            ".",
            Options::default(),
            FailingWriter(io::ErrorKind::Other),
            Vec::new(),
        );

        assert_eq!(code, 1);
    }
}
