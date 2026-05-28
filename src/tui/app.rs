// UI state model and list-selection navigation. No ratatui rendering here.

use std::time::Duration;

use ratatui::widgets::ListState;

use crate::scan::Summary;

// Row data the TUI cares about; decoupled from EntryItem so the scan thread can drop paths.
pub(crate) struct Row {
    pub(crate) type_name: &'static str,
    pub(crate) name: String,
    pub(crate) size: Option<u64>,
}

pub(crate) struct App {
    pub(crate) rows: Vec<Row>,
    pub(crate) state: ListState,
    pub(crate) warnings: Vec<String>,
    pub(crate) summary: Option<Summary>,
    pub(crate) elapsed: Option<Duration>,
    pub(crate) scanning: bool,
    // Last rendered list viewport height; used so PgUp/PgDn move by visible page.
    pub(crate) list_height: usize,
}

impl App {
    pub(crate) fn new() -> Self {
        Self {
            rows: Vec::new(),
            state: ListState::default(),
            warnings: Vec::new(),
            summary: None,
            elapsed: None,
            scanning: true,
            list_height: 10,
        }
    }

    pub(crate) fn push_row(&mut self, row: Row) {
        self.rows.push(row);
        // Select the first row as soon as one arrives so arrow keys work immediately.
        if self.state.selected().is_none() {
            self.state.select(Some(0));
        }
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
