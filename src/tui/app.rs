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
        }
    }

    pub(crate) fn push_row(&mut self, row: Row) {
        self.rows.push(row);
        self.apply_sort_preserve_selection();
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
        self.apply_sort_preserve_selection();
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
        if self.rows.len() < 2 {
            return;
        }

        let selected = self
            .state
            .selected()
            .and_then(|i| self.rows.get(i))
            .map(row_key);

        let field = self.config.sort_field;
        let direction = self.config.sort_direction;
        self.rows
            .sort_by(|left, right| compare_rows(left, right, field, direction));

        if let Some(selected) = selected {
            if let Some(index) = self.rows.iter().position(|row| row_key(row) == selected) {
                self.state.select(Some(index));
            }
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
