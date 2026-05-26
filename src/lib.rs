use std::cmp::Ordering;
use std::env;
use std::fmt;
use std::fs::{self, DirEntry, FileType, ReadDir};
use std::io::{self, BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
    let options = match Options::parse(args) {
        Ok(options) => options,
        Err(err) => {
            let _ = writeln!(stderr, "error: {err}");
            return 1;
        }
    };

    run_path(".", options, stdout, stderr)
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
struct Options {
    order: Option<SortOrder>,
}

impl Options {
    fn parse<I, S>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut options = Self::default();
        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            match arg.as_ref() {
                "--o" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--o requires asc or desc".to_owned())?;
                    options.order = Some(SortOrder::parse(value.as_ref())?);
                }
                unknown => return Err(format!("unknown option: {unknown}")),
            }
        }

        Ok(options)
    }
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

    if let Err(err) = out.write_all(HEADER.as_bytes()) {
        return write_error_code(err);
    }
    if options.order.is_some() {
        if let Err(err) = out.flush() {
            return write_error_code(err);
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

        let item = match EntryItem::from_entry(entry, stderr) {
            Some(item) => item,
            None => continue,
        };

        match item.type_name {
            "FILE" => {
                summary.files += 1;
                let size = file_size(&item.path);
                let row = OutputRow::new(item, size);
                if options.order.is_some() {
                    rows.push(row);
                } else if let Err(err) = write_item_row(&mut out, &row) {
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
                if options.order.is_some() {
                    rows.push(row);
                } else if let Err(err) = write_item_row(&mut out, &row) {
                    return write_error_code(err);
                }
            }
        }
    }

    for result in scan_directories_parallel(dir_jobs) {
        for warning in result.warnings {
            let _ = writeln!(stderr, "{warning}");
        }

        let row = OutputRow::new(result.item, Some(result.size));
        if options.order.is_some() {
            rows.push(row);
        } else if let Err(err) = write_item_row(&mut out, &row) {
            return write_error_code(err);
        }
    }

    if let Some(order) = options.order {
        rows.sort_by(|left, right| compare_rows(left, right, order));
        for row in rows {
            if let Err(err) = write_item_row(&mut out, &row) {
                return write_error_code(err);
            }
        }
    }

    if let Err(err) = write_summary(&mut out, summary, start.elapsed()) {
        return write_error_code(err);
    }

    match out.flush() {
        Ok(()) => 0,
        Err(err) => write_error_code(err),
    }
}

struct EntryItem {
    path: PathBuf,
    name: String,
    type_name: &'static str,
}

impl EntryItem {
    fn from_entry<E>(entry: DirEntry, stderr: &mut E) -> Option<Self>
    where
        E: Write,
    {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                let _ = writeln!(
                    stderr,
                    "warning: cannot read file type for {:?}: {err}",
                    entry.file_name()
                );
                return Some(Self {
                    path: entry.path(),
                    name: entry.file_name().to_string_lossy().into_owned(),
                    type_name: "OTHER",
                });
            }
        };

        Some(Self {
            path: entry.path(),
            name: entry.file_name().to_string_lossy().into_owned(),
            type_name: entry_type(file_type),
        })
    }
}

struct DirectoryResult {
    item: EntryItem,
    size: u64,
    warnings: Vec<String>,
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

#[derive(Default)]
struct Summary {
    files: u64,
    dirs: u64,
    others: u64,
}

impl Summary {
    fn total(&self) -> u64 {
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

fn file_size(path: &Path) -> Option<u64> {
    fs::metadata(path).map(|metadata| metadata.len()).ok()
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

fn scan_directories_parallel(jobs: Vec<EntryItem>) -> Vec<DirectoryResult> {
    if jobs.is_empty() {
        return Vec::new();
    }

    let worker_count = worker_count().min(jobs.len());
    let jobs = Arc::new(Mutex::new(jobs.into_iter()));
    let (result_tx, result_rx) = mpsc::channel();
    let mut workers = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let jobs = Arc::clone(&jobs);
        let result_tx = result_tx.clone();
        workers.push(thread::spawn(move || loop {
            let item = {
                let mut jobs = jobs.lock().unwrap();
                jobs.next()
            };

            let Some(item) = item else {
                break;
            };

            let mut warnings = Vec::new();
            let size = directory_size(item.path.clone(), &mut warnings);
            if result_tx
                .send(DirectoryResult {
                    item,
                    size,
                    warnings,
                })
                .is_err()
            {
                break;
            }
        }));
    }

    drop(result_tx);

    let mut results = Vec::new();
    for result in result_rx {
        results.push(result);
    }

    for worker in workers {
        let _ = worker.join();
    }

    results
}

fn worker_count() -> usize {
    thread::available_parallelism()
        .map(|count| count.get() / 2)
        .unwrap_or(1)
        .max(1)
}

fn directory_size(path: PathBuf, warnings: &mut Vec<String>) -> u64 {
    let mut total = 0_u64;
    let mut stack = vec![path];

    // Use explicit stack so deep trees cannot overflow the call stack.
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) => {
                warnings.push(format!("warning: cannot read directory {:?}: {err}", dir));
                continue;
            }
        };

        for entry_result in entries {
            let entry = match entry_result {
                Ok(entry) => entry,
                Err(err) => {
                    warnings.push(format!("warning: cannot read directory entry: {err}"));
                    continue;
                }
            };

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
                stack.push(entry.path());
            } else if file_type.is_file() {
                match entry.metadata() {
                    Ok(metadata) => total = total.saturating_add(metadata.len()),
                    Err(err) => warnings.push(format!(
                        "warning: cannot read metadata for {:?}: {err}",
                        entry.path()
                    )),
                }
            }
        }
    }

    total
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

fn format_duration(duration: Duration) -> String {
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
    use super::{format_size, run_path, Options};
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
