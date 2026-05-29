// UI state model and list-selection navigation. No ratatui rendering here.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ratatui::widgets::ListState;

use crate::config::{Config, SortDirection, SortField};
use crate::scan::Summary;

#[derive(Clone)]
pub(crate) struct Row {
    pub(crate) type_name: &'static str,
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) size: Option<u64>,
    pub(crate) order: usize,
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

#[derive(Clone, Copy, Default)]
pub(crate) struct SystemStatus {
    pub(crate) cpu_percent: f32,
    pub(crate) memory_mb: u64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum ViewMode {
    List,
    Settings,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum SettingsAction {
    ShowHidden,
    SortField,
    SortDirection,
    Save,
    Cancel,
}

pub(crate) const SETTINGS_ACTIONS: [SettingsAction; 5] = [
    SettingsAction::ShowHidden,
    SettingsAction::SortField,
    SettingsAction::SortDirection,
    SettingsAction::Save,
    SettingsAction::Cancel,
];

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
    pub(crate) config: Config,
    pub(crate) draft_config: Option<Config>,
    pub(crate) view: ViewMode,
    pub(crate) settings_selected: usize,
    pub(crate) status: Option<String>,
    pub(crate) system_status: SystemStatus,
    // Fuzzy filter state. `filtering` is the live-input mode toggled by `/`;
    // `filter_query` persists after Enter so the list stays filtered.
    pub(crate) filtering: bool,
    pub(crate) filter_query: String,
    // Indices into `rows` (in sorted order) that pass the current filter.
    // The list view and `state` selection both index into this, not `rows`.
    pub(crate) visible: Vec<usize>,
}

impl App {
    pub(crate) fn new(initial_root: PathBuf, config: Config) -> Self {
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
            config,
            draft_config: None,
            view: ViewMode::List,
            settings_selected: 0,
            status: None,
            system_status: SystemStatus::default(),
            filtering: false,
            filter_query: String::new(),
            visible: Vec::new(),
        }
    }

    pub(crate) fn push_row(&mut self, row: Row) {
        self.rows.push(row);
        self.apply_sort_preserve_selection();
        // Select the first visible row as soon as one arrives so arrow keys work immediately.
        self.ensure_selection();
    }

    pub(crate) fn begin_scan(&mut self, path: PathBuf) -> u64 {
        self.scan_id = self.scan_id.wrapping_add(1);
        self.current_dir = path;
        self.rows.clear();
        self.visible.clear();
        self.state = ListState::default();
        self.summary = None;
        self.elapsed = None;
        self.scanning = true;
        // A fresh scan starts unfiltered; a stale query from the prior dir is meaningless here.
        self.filtering = false;
        self.filter_query.clear();
        self.scan_id
    }

    pub(crate) fn finish_scan(&mut self, summary: Summary, elapsed: Duration) {
        self.scanning = false;
        self.summary = Some(summary);
        self.elapsed = Some(elapsed);
        self.apply_sort_preserve_selection();
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
        self.filtering = false;
        self.filter_query.clear();
        self.apply_sort_preserve_selection();
        self.ensure_selection();
        true
    }

    pub(crate) fn selected_row(&self) -> Option<&Row> {
        let view_index = self.state.selected()?;
        let row_index = *self.visible.get(view_index)?;
        self.rows.get(row_index)
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

    pub(crate) fn clear_cache(&mut self) {
        self.cache.clear();
    }

    pub(crate) fn open_settings(&mut self) {
        self.draft_config = Some(self.config);
        self.settings_selected = 0;
        self.status = None;
        self.view = ViewMode::Settings;
    }

    pub(crate) fn close_settings(&mut self) {
        self.draft_config = None;
        self.status = None;
        self.view = ViewMode::List;
    }

    pub(crate) fn draft_config(&self) -> Config {
        self.draft_config.unwrap_or(self.config)
    }

    pub(crate) fn apply_draft_config(&mut self) -> bool {
        let draft = self.draft_config();
        let hidden_changed = draft.show_hidden != self.config.show_hidden;
        self.config = draft;
        self.draft_config = None;
        self.view = ViewMode::List;
        self.status = None;
        self.apply_sort_preserve_selection();
        hidden_changed
    }

    pub(crate) fn settings_action(&self) -> SettingsAction {
        SETTINGS_ACTIONS[self.settings_selected]
    }

    pub(crate) fn settings_up(&mut self) {
        self.settings_selected = self.settings_selected.saturating_sub(1);
    }

    pub(crate) fn settings_down(&mut self) {
        let last = SETTINGS_ACTIONS.len() - 1;
        self.settings_selected = self.settings_selected.saturating_add(1).min(last);
    }

    pub(crate) fn cycle_selected_setting(&mut self) {
        let Some(mut draft) = self.draft_config else {
            return;
        };

        match self.settings_action() {
            SettingsAction::ShowHidden => draft.show_hidden = !draft.show_hidden,
            SettingsAction::SortField => draft.sort_field = draft.sort_field.next(),
            SettingsAction::SortDirection => draft.sort_direction = draft.sort_direction.toggle(),
            SettingsAction::Save | SettingsAction::Cancel => {}
        }

        self.draft_config = Some(draft);
    }

    pub(crate) fn apply_sort_preserve_selection(&mut self) {
        // Capture the selected row's identity before mutating order/visibility.
        let selected = self.selected_row().map(row_key);

        if self.rows.len() >= 2 {
            let field = self.config.sort_field;
            let direction = self.config.sort_direction;
            self.rows
                .sort_by(|left, right| compare_rows(left, right, field, direction));
        }

        self.recompute_visible();

        // Re-anchor the selection onto the same row in the new visible set.
        match selected {
            Some(key) => {
                let pos = self
                    .visible
                    .iter()
                    .position(|&i| row_key(&self.rows[i]) == key);
                match pos {
                    Some(index) => self.state.select(Some(index)),
                    // Previously selected row was filtered out: fall back to the top.
                    None if !self.visible.is_empty() => self.state.select(Some(0)),
                    None => self.state.select(None),
                }
            }
            None if self.visible.is_empty() => self.state.select(None),
            None => {}
        }
    }

    // Rebuild `visible` from the current rows and filter query.
    fn recompute_visible(&mut self) {
        self.visible = (0..self.rows.len())
            .filter(|&i| fuzzy_match(&self.rows[i].name, &self.filter_query))
            .collect();
    }

    // Select the first visible row when nothing is selected yet.
    fn ensure_selection(&mut self) {
        if self.state.selected().is_none() && !self.visible.is_empty() {
            self.state.select(Some(0));
        }
    }

    pub(crate) fn start_filter(&mut self) {
        self.filtering = true;
    }

    pub(crate) fn filter_push_char(&mut self, c: char) {
        self.filter_query.push(c);
        self.apply_sort_preserve_selection();
        self.ensure_selection();
    }

    pub(crate) fn filter_backspace(&mut self) {
        self.filter_query.pop();
        self.apply_sort_preserve_selection();
        self.ensure_selection();
    }

    // Enter: keep the query, leave live-input mode.
    pub(crate) fn filter_confirm(&mut self) {
        self.filtering = false;
    }

    // Esc: drop the filter entirely and show every row again.
    pub(crate) fn filter_clear(&mut self) {
        self.filtering = false;
        self.filter_query.clear();
        self.apply_sort_preserve_selection();
        self.ensure_selection();
    }

    pub(crate) fn move_up(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        let i = self.state.selected().unwrap_or(0).saturating_sub(1);
        self.state.select(Some(i));
    }

    pub(crate) fn move_down(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() - 1;
        let i = self
            .state
            .selected()
            .unwrap_or(0)
            .saturating_add(1)
            .min(last);
        self.state.select(Some(i));
    }

    pub(crate) fn move_first(&mut self) {
        if !self.visible.is_empty() {
            self.state.select(Some(0));
        }
    }

    pub(crate) fn move_last(&mut self) {
        if let Some(last) = self.visible.len().checked_sub(1) {
            self.state.select(Some(last));
        }
    }

    pub(crate) fn page_down(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        let page = self.list_height.max(1);
        let last = self.visible.len() - 1;
        let i = self
            .state
            .selected()
            .unwrap_or(0)
            .saturating_add(page)
            .min(last);
        self.state.select(Some(i));
    }

    pub(crate) fn page_up(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        let page = self.list_height.max(1);
        let i = self.state.selected().unwrap_or(0).saturating_sub(page);
        self.state.select(Some(i));
    }
}

// Case-insensitive subsequence match: every query char appears in order in `name`.
// Empty query matches everything.
fn fuzzy_match(name: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut haystack = name.chars().map(|c| c.to_ascii_lowercase());
    'next: for needle in query.chars().map(|c| c.to_ascii_lowercase()) {
        for c in haystack.by_ref() {
            if c == needle {
                continue 'next;
            }
        }
        return false;
    }
    true
}

fn compare_rows(left: &Row, right: &Row, field: SortField, direction: SortDirection) -> Ordering {
    if field == SortField::Unsorted {
        return left.order.cmp(&right.order);
    }

    let ordering = match field {
        SortField::Unsorted => Ordering::Equal,
        SortField::Name => left
            .name
            .cmp(&right.name)
            .then_with(|| left.type_name.cmp(right.type_name))
            .then_with(|| left.size.cmp(&right.size)),
        SortField::Size => left
            .size
            .unwrap_or(u64::MAX)
            .cmp(&right.size.unwrap_or(u64::MAX))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.type_name.cmp(right.type_name)),
        SortField::Type => left
            .type_name
            .cmp(right.type_name)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.size.cmp(&right.size)),
    };

    match direction {
        SortDirection::Asc => ordering,
        SortDirection::Desc => ordering.reverse(),
    }
}

fn row_key(row: &Row) -> (String, &'static str, Option<u64>, usize) {
    (row.name.clone(), row.type_name, row.size, row.order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(type_name: &'static str, name: &str, size: Option<u64>) -> Row {
        Row {
            type_name,
            name: name.to_owned(),
            path: PathBuf::from(name),
            size,
            order: 0,
        }
    }

    fn app_with_rows(config: Config) -> App {
        let mut app = App::new(PathBuf::from("."), config);
        app.rows = vec![
            Row {
                order: 0,
                ..row("FILE", "b.txt", Some(20))
            },
            Row {
                order: 1,
                ..row("DIR", "a", Some(100))
            },
            Row {
                order: 2,
                ..row("OTHER", "c", None)
            },
        ];
        // Mirror the runtime invariant: `visible` indexes `rows`, and `state` indexes `visible`.
        app.visible = (0..app.rows.len()).collect();
        app.state.select(Some(1));
        app
    }

    #[test]
    fn unsorted_preserves_insertion_order() {
        let mut app = app_with_rows(Config::default());

        app.apply_sort_preserve_selection();

        let names: Vec<_> = app.rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["b.txt", "a", "c"]);
    }

    #[test]
    fn sorts_by_name_ascending() {
        let mut app = app_with_rows(Config {
            sort_field: SortField::Name,
            sort_direction: SortDirection::Asc,
            ..Config::default()
        });

        app.apply_sort_preserve_selection();

        let names: Vec<_> = app.rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["a", "b.txt", "c"]);
        assert_eq!(app.state.selected(), Some(0));
    }

    #[test]
    fn sorts_by_size_descending() {
        let mut app = app_with_rows(Config {
            sort_field: SortField::Size,
            sort_direction: SortDirection::Desc,
            ..Config::default()
        });

        app.apply_sort_preserve_selection();

        let names: Vec<_> = app.rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["c", "a", "b.txt"]);
    }

    #[test]
    fn fuzzy_match_is_case_insensitive_subsequence() {
        assert!(fuzzy_match("Cargo.toml", "ct"));
        assert!(fuzzy_match("Cargo.toml", "CARGO"));
        assert!(fuzzy_match("anything", ""));
        assert!(!fuzzy_match("Cargo.toml", "tc")); // wrong order
        assert!(!fuzzy_match("Cargo.toml", "xyz"));
    }

    #[test]
    fn filter_narrows_visible_set() {
        let mut app = app_with_rows(Config::default());

        app.filter_query = "a".to_owned();
        app.apply_sort_preserve_selection();

        // Only "a.*"-matching names survive; here "a" (DIR) and "b.txt"? "b.txt" has no 'a'.
        let visible: Vec<_> = app
            .visible
            .iter()
            .map(|&i| app.rows[i].name.as_str())
            .collect();
        assert_eq!(visible, ["a"]);
    }

    #[test]
    fn clearing_filter_restores_all_rows() {
        let mut app = app_with_rows(Config::default());

        app.filter_query = "zzz".to_owned();
        app.apply_sort_preserve_selection();
        assert!(app.visible.is_empty());
        assert_eq!(app.state.selected(), None);

        app.filter_clear();
        assert_eq!(app.visible.len(), 3);
        assert_eq!(app.state.selected(), Some(0));
    }

    #[test]
    fn sorts_by_type_ascending() {
        let mut app = app_with_rows(Config {
            sort_field: SortField::Type,
            sort_direction: SortDirection::Asc,
            ..Config::default()
        });

        app.apply_sort_preserve_selection();

        let types: Vec<_> = app.rows.iter().map(|row| row.type_name).collect();
        assert_eq!(types, ["DIR", "FILE", "OTHER"]);
    }
}
