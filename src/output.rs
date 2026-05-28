// Table and NDJSON rendering, row buffering/sorting, and the summary line.

use std::cmp::Ordering;
use std::fs::{self, ReadDir};
use std::io::{self, BufWriter, ErrorKind, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::cli::{Options, SortOrder};
use crate::format::{format_duration, format_size};
use crate::scan::{is_hidden, scan_directories_parallel, EntryItem, Summary};

const HEADER: &str = "TYPE  SIZE       NAME\n";

pub(crate) fn run_path<P, W, E>(path: P, options: Options, stdout: W, mut stderr: E) -> u8
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

fn write_error_code(err: io::Error) -> u8 {
    if err.kind() == ErrorKind::BrokenPipe {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::{run_path, write_json_string};
    use crate::cli::Options;
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
