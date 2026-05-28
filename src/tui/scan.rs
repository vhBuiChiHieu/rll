// Background scan thread and the mpsc streaming protocol that feeds the UI.

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::app::Row;
use crate::scan::{is_hidden, scan_directories_parallel, EntryItem, Summary};

// Streaming protocol between background scan thread and UI loop.
pub(crate) enum ScanEvent {
    Row(Row),
    Warning(String),
    Done(Summary, Duration),
}

pub(crate) fn scan_into_channel(tx: mpsc::Sender<ScanEvent>, show_all: bool) {
    let start = Instant::now();
    let mut summary = Summary::default();
    let mut dir_jobs: Vec<EntryItem> = Vec::new();

    let entries = match std::fs::read_dir(Path::new(".")) {
        Ok(entries) => entries,
        Err(err) => {
            let _ = tx.send(ScanEvent::Warning(format!(
                "error: cannot read current directory: {err}"
            )));
            let _ = tx.send(ScanEvent::Done(summary, start.elapsed()));
            return;
        }
    };

    // Buffer warnings from EntryItem::from_entry into a Vec<u8> sink, then re-emit as
    // ScanEvent::Warning so they print to stderr after the TUI exits.
    let mut sink: Vec<u8> = Vec::new();

    for entry_result in entries {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(err) => {
                let _ = tx.send(ScanEvent::Warning(format!(
                    "warning: cannot read directory entry: {err}"
                )));
                continue;
            }
        };

        let file_name = entry.file_name();
        if !show_all && is_hidden(&file_name) {
            continue;
        }

        let item = match EntryItem::from_entry(entry, file_name, &mut sink) {
            Some(item) => item,
            None => continue,
        };

        match item.type_name {
            "FILE" => {
                summary.files += 1;
                let _ = tx.send(ScanEvent::Row(Row {
                    type_name: item.type_name,
                    name: item.name,
                    size: item.size_hint,
                }));
            }
            "DIR" => {
                summary.dirs += 1;
                dir_jobs.push(item);
            }
            _ => {
                summary.others += 1;
                let _ = tx.send(ScanEvent::Row(Row {
                    type_name: item.type_name,
                    name: item.name,
                    size: None,
                }));
            }
        }
    }

    // Flush buffered top-level warnings.
    flush_sink_warnings(&tx, sink);

    let scan = scan_directories_parallel(dir_jobs, show_all);
    summary.files += scan.nested.files;
    summary.dirs += scan.nested.dirs;
    summary.others += scan.nested.others;

    for result in scan.results {
        for warning in result.warnings {
            let _ = tx.send(ScanEvent::Warning(warning));
        }
        let _ = tx.send(ScanEvent::Row(Row {
            type_name: result.item.type_name,
            name: result.item.name,
            size: Some(result.size),
        }));
    }

    let _ = tx.send(ScanEvent::Done(summary, start.elapsed()));
}

fn flush_sink_warnings(tx: &mpsc::Sender<ScanEvent>, sink: Vec<u8>) {
    if sink.is_empty() {
        return;
    }
    if let Ok(text) = String::from_utf8(sink) {
        for line in text.lines().filter(|line| !line.is_empty()) {
            let _ = tx.send(ScanEvent::Warning(line.to_owned()));
        }
    }
}
