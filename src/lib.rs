use std::fmt;
use std::fs::{self, DirEntry, FileType, ReadDir};
use std::io::{self, BufWriter, ErrorKind, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const HEADER: &str = "TYPE  SIZE       NAME\n";

pub fn run_stdio() -> u8 {
    let stdout = io::stdout();
    let stderr = io::stderr();
    run(stdout.lock(), stderr.lock())
}

pub fn run<W, E>(stdout: W, stderr: E) -> u8
where
    W: Write,
    E: Write,
{
    run_path(".", stdout, stderr)
}

fn run_path<P, W, E>(path: P, stdout: W, mut stderr: E) -> u8
where
    P: AsRef<Path>,
    W: Write,
    E: Write,
{
    match fs::read_dir(path) {
        Ok(entries) => write_entries(stdout, &mut stderr, entries),
        Err(err) => {
            let _ = writeln!(stderr, "error: cannot read current directory: {err}");
            1
        }
    }
}

pub fn format_size(bytes: u64) -> String {
    Size(bytes).to_string()
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

fn write_entries<W, E>(stdout: W, stderr: &mut E, entries: ReadDir) -> u8
where
    W: Write,
    E: Write,
{
    let start = Instant::now();
    let mut summary = Summary::default();
    let mut out = BufWriter::new(stdout);

    if let Err(err) = out.write_all(HEADER.as_bytes()) {
        return write_error_code(err);
    }

    for entry_result in entries {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(err) => {
                let _ = writeln!(stderr, "warning: cannot read directory entry: {err}");
                continue;
            }
        };

        if let Err(err) = write_entry(&mut out, stderr, &entry, &mut summary) {
            return write_error_code(err);
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

fn write_entry<W, E>(
    out: &mut W,
    stderr: &mut E,
    entry: &DirEntry,
    summary: &mut Summary,
) -> io::Result<()>
where
    W: Write,
    E: Write,
{
    let type_name = match entry.file_type() {
        Ok(file_type) => {
            let type_name = entry_type(file_type);
            match type_name {
                "FILE" => summary.files += 1,
                "DIR" => summary.dirs += 1,
                _ => summary.others += 1,
            }
            type_name
        }
        Err(err) => {
            let _ = writeln!(
                stderr,
                "warning: cannot read file type for {:?}: {err}",
                entry.file_name()
            );
            summary.others += 1;
            "OTHER"
        }
    };

    let size = match entry.metadata() {
        Ok(metadata) => format_size(metadata.len()),
        Err(_) => "?".to_owned(),
    };

    let name = entry.file_name();
    let name = name.to_string_lossy();

    writeln!(out, "{type_name:<5} {size:<10} {name}")
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
    use super::{format_size, run_path};
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
        let code = run_path(".", FailingWriter(io::ErrorKind::BrokenPipe), Vec::new());

        assert_eq!(code, 0);
    }

    #[test]
    fn non_broken_pipe_write_error_returns_exit_code_one() {
        let code = run_path(".", FailingWriter(io::ErrorKind::Other), Vec::new());

        assert_eq!(code, 1);
    }
}
