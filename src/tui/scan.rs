// Background scan thread and the mpsc streaming protocol that feeds the UI.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::app::Row;
use crate::scan::{is_hidden, scan_directories_parallel, EntryItem, Summary};

pub(crate) enum ScanEvent {
    Row {
        scan_id: u64,
        row: Row,
    },
    Warning {
        scan_id: u64,
        warning: String,
    },
    Done {
        scan_id: u64,
        summary: Summary,
        elapsed: Duration,
    },
}

pub(crate) fn scan_into_channel(
    tx: mpsc::Sender<ScanEvent>,
    show_all: bool,
    path: PathBuf,
    scan_id: u64,
) {
    let start = Instant::now();
    let mut summary = Summary::default();
    let mut dir_jobs: Vec<EntryItem> = Vec::new();

    let entries = match std::fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(err) => {
            send_warning(
                &tx,
                scan_id,
                format!("error: cannot read {}: {err}", path.display()),
            );
            send_done(&tx, scan_id, summary, start.elapsed());
            return;
        }
    };

    // Buffer warnings from EntryItem::from_entry so they print after the TUI exits.
    let mut sink: Vec<u8> = Vec::new();

    for entry_result in entries {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(err) => {
                send_warning(
                    &tx,
                    scan_id,
                    format!("warning: cannot read directory entry: {err}"),
                );
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
                let row_path = path.join(&item.name);
                let _ = tx.send(ScanEvent::Row {
                    scan_id,
                    row: Row {
                        type_name: item.type_name,
                        name: item.name,
                        path: row_path,
                        size: item.size_hint,
                    },
                });
            }
            "DIR" => {
                summary.dirs += 1;
                dir_jobs.push(item);
            }
            _ => {
                summary.others += 1;
                let row_path = path.join(&item.name);
                let _ = tx.send(ScanEvent::Row {
                    scan_id,
                    row: Row {
                        type_name: item.type_name,
                        name: item.name,
                        path: row_path,
                        size: None,
                    },
                });
            }
        }
    }

    flush_sink_warnings(&tx, scan_id, sink);

    let scan = scan_directories_parallel(dir_jobs, show_all);
    summary.files += scan.nested.files;
    summary.dirs += scan.nested.dirs;
    summary.others += scan.nested.others;

    for result in scan.results {
        for warning in result.warnings {
            send_warning(&tx, scan_id, warning);
        }
        let row_path = path.join(&result.item.name);
        let _ = tx.send(ScanEvent::Row {
            scan_id,
            row: Row {
                type_name: result.item.type_name,
                name: result.item.name,
                path: row_path,
                size: Some(result.size),
            },
        });
    }

    send_done(&tx, scan_id, summary, start.elapsed());
}

fn flush_sink_warnings(tx: &mpsc::Sender<ScanEvent>, scan_id: u64, sink: Vec<u8>) {
    if sink.is_empty() {
        return;
    }
    if let Ok(text) = String::from_utf8(sink) {
        for line in text.lines().filter(|line| !line.is_empty()) {
            send_warning(tx, scan_id, line.to_owned());
        }
    }
}

fn send_warning(tx: &mpsc::Sender<ScanEvent>, scan_id: u64, warning: String) {
    let _ = tx.send(ScanEvent::Warning { scan_id, warning });
}

fn send_done(tx: &mpsc::Sender<ScanEvent>, scan_id: u64, summary: Summary, elapsed: Duration) {
    let _ = tx.send(ScanEvent::Done {
        scan_id,
        summary,
        elapsed,
    });
}
