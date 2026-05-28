// UI state model and list-selection navigation. No ratatui rendering here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ratatui::widgets::ListState;

use crate::scan::Summary;

#[derive(Clone)]
pub(crate) struct Row {
    pub(crate) type_name: &'static str,
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) size: Option<u64>,
}

#[derive(Clone)]
pub(crate) struct CachedDir {
    pub(crate) rows: Vec<Row>,
    pub(crate) summary: Summary,
    pub(crate) elapsed: Duration,
}

pub(crate) struct ConfirmLeaveRoot {
    pub(crate) target: PathBuf,
}

pub(crate) struct App {
    pub(crate) rows: Vec<Row>,
    pub(crate) state: ListState,
    pub(crate) warnings: Vec<String>,
    pub(crate) summary: Option<Summary>,
    pub(crate) elapsed: Option<Duration>,
    pub(crate) scanning: bool,
    pub(crate) list_height: usize,
    pub(crate) initial_root: PathBuf,
    pub(crate) current_dir: PathBuf,
    pub(crate) cache: HashMap<PathBuf, CachedDir>,
    pub(crate) scan_id: u64,
    pub(crate) confirm_leave_root: Option<ConfirmLeaveRoot>,
}

impl App {
    pub(crate) fn new(initial_root: PathBuf) -> Self {
        Self {
            rows: Vec::new(),
            state: ListState::default(),
            warnings: Vec::new(),
            summary: None,
            elapsed: None,
            scanning: false,
            list_height: 10,
            current_dir: initial_root.clone(),
            initial_root,
            cache: HashMap::new(),
            scan_id: 0,
            confirm_leave_root: None,
        }
    }

    pub(crate) fn push_row(&mut self, row: Row) {
        self.rows.push(row);
        // Select the first row as soon as one arrives so arrow keys work immediately.
        if self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    pub(crate) fn begin_scan(&mut self, path: PathBuf) -> u64 {
        self.scan_id = self.scan_id.wrapping_add(1);
        self.current_dir = path;
        self.rows.clear();
        self.state = ListState::default();
        self.summary = None;
        self.elapsed = None;
        self.scanning = true;
        self.scan_id
    }

    pub(crate) fn finish_scan(&mut self, summary: Summary, elapsed: Duration) {
        self.scanning = false;
        self.summary = Some(summary);
        self.elapsed = Some(elapsed);
        self.cache.insert(
            self.current_dir.clone(),
            CachedDir {
                rows: self.rows.clone(),
                summary,
                elapsed,
            },
        );
    }

    pub(crate) fn apply_cached(&mut self, path: PathBuf) -> bool {
        let Some(cached) = self.cache.get(&path).cloned() else {
            return false;
        };
        self.scan_id = self.scan_id.wrapping_add(1);
        self.current_dir = path;
        self.rows = cached.rows;
        self.summary = Some(cached.summary);
        self.elapsed = Some(cached.elapsed);
        self.scanning = false;
        self.state = ListState::default();
        if !self.rows.is_empty() {
            self.state.select(Some(0));
        }
        true
    }

    pub(crate) fn selected_row(&self) -> Option<&Row> {
        self.state.selected().and_then(|i| self.rows.get(i))
    }

    pub(crate) fn selected_dir_path(&self) -> Option<PathBuf> {
        let row = self.selected_row()?;
        (row.type_name == "DIR").then(|| row.path.clone())
    }

    pub(crate) fn parent_path(&self) -> Option<PathBuf> {
        self.current_dir.parent().map(Path::to_path_buf)
    }

    pub(crate) fn parent_needs_confirmation(&self, parent: &Path) -> bool {
        !parent.starts_with(&self.initial_root)
    }

    pub(crate) fn clear_current_cache(&mut self) {
        self.cache.remove(&self.current_dir);
    }

    pub(crate) fn move_up(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let i = self.state.selected().unwrap_or(0).saturating_sub(1);
        self.state.select(Some(i));
    }

    pub(crate) fn move_down(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        let i = self
            .state
            .selected()
            .unwrap_or(0)
            .saturating_add(1)
            .min(last);
        self.state.select(Some(i));
    }

    pub(crate) fn move_first(&mut self) {
        if !self.rows.is_empty() {
            self.state.select(Some(0));
        }
    }

    pub(crate) fn move_last(&mut self) {
        if let Some(last) = self.rows.len().checked_sub(1) {
            self.state.select(Some(last));
        }
    }

    pub(crate) fn page_down(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let page = self.list_height.max(1);
        let last = self.rows.len() - 1;
        let i = self
            .state
            .selected()
            .unwrap_or(0)
            .saturating_add(page)
            .min(last);
        self.state.select(Some(i));
    }

    pub(crate) fn page_up(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let page = self.list_height.max(1);
        let i = self.state.selected().unwrap_or(0).saturating_sub(page);
        self.state.select(Some(i));
    }
}
