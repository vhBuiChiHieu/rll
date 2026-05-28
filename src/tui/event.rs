// Event loop: drains scan events, throttles redraws, and maps keys to navigation.

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Terminal;

use super::app::{App, ConfirmLeaveRoot, SettingsAction, ViewMode};
use super::render::render;
use super::scan::{scan_into_channel, ScanEvent};

pub(crate) fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    tx: mpsc::Sender<ScanEvent>,
    rx: mpsc::Receiver<ScanEvent>,
) -> io::Result<()> {
    start_scan(app, &tx, app.current_dir.clone());

    // Cap redraws to ~30fps so a fast scan does not churn the terminal during streaming.
    let frame_budget = Duration::from_millis(33);
    let mut last_draw = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut dirty = true;

    loop {
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    if apply_scan_event(app, event) {
                        dirty = true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }

        if dirty && last_draw.elapsed() >= frame_budget {
            terminal.draw(|f| render(f, app))?;
            last_draw = Instant::now();
            dirty = false;
        }

        let poll_timeout = if app.scanning {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(100)
        };

        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match handle_key(app, &tx, key) {
                        KeyAction::Quit => return Ok(()),
                        KeyAction::Dirty => dirty = true,
                        KeyAction::None => {}
                    }
                }
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        } else if dirty {
            // Force at least one redraw even if the frame budget hadn't elapsed yet.
            terminal.draw(|f| render(f, app))?;
            last_draw = Instant::now();
            dirty = false;
        }
    }
}

enum KeyAction {
    None,
    Dirty,
    Quit,
}

fn apply_scan_event(app: &mut App, event: ScanEvent) -> bool {
    match event {
        ScanEvent::Row { scan_id, row } => {
            if scan_id != app.scan_id {
                return false;
            }
            app.push_row(row);
            true
        }
        ScanEvent::Warning { scan_id, warning } => {
            if scan_id == app.scan_id {
                app.warnings.push(warning);
            }
            false
        }
        ScanEvent::Done {
            scan_id,
            summary,
            elapsed,
        } => {
            if scan_id != app.scan_id {
                return false;
            }
            app.finish_scan(summary, elapsed);
            true
        }
    }
}

fn handle_key(app: &mut App, tx: &mpsc::Sender<ScanEvent>, key: KeyEvent) -> KeyAction {
    if app.confirm_leave_root.is_some() {
        return handle_modal_key(app, tx, key);
    }
    if app.view == ViewMode::Settings {
        return handle_settings_key(app, tx, key);
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => KeyAction::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyAction::Quit,
        KeyCode::Up | KeyCode::Char('k') => {
            app.move_up();
            KeyAction::Dirty
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.move_down();
            KeyAction::Dirty
        }
        KeyCode::Home | KeyCode::Char('g') => {
            app.move_first();
            KeyAction::Dirty
        }
        KeyCode::End | KeyCode::Char('G') => {
            app.move_last();
            KeyAction::Dirty
        }
        KeyCode::PageDown | KeyCode::Char('d') => {
            app.page_down();
            KeyAction::Dirty
        }
        KeyCode::PageUp | KeyCode::Char('u') => {
            app.page_up();
            KeyAction::Dirty
        }
        KeyCode::Enter | KeyCode::Char('l') => {
            if let Some(path) = app.selected_dir_path() {
                navigate_to(app, tx, path);
                KeyAction::Dirty
            } else {
                KeyAction::None
            }
        }
        KeyCode::Backspace | KeyCode::Char('h') => {
            if let Some(parent) = app.parent_path() {
                if app.parent_needs_confirmation(&parent) {
                    app.confirm_leave_root = Some(ConfirmLeaveRoot { target: parent });
                } else {
                    navigate_to(app, tx, parent);
                }
                KeyAction::Dirty
            } else {
                KeyAction::None
            }
        }
        KeyCode::Char('r') => {
            let path = app.current_dir.clone();
            app.clear_current_cache();
            start_scan(app, tx, path);
            KeyAction::Dirty
        }
        KeyCode::Char('c') => {
            app.open_settings();
            KeyAction::Dirty
        }
        _ => KeyAction::None,
    }
}

fn handle_settings_key(app: &mut App, tx: &mpsc::Sender<ScanEvent>, key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Char('q') => KeyAction::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyAction::Quit,
        KeyCode::Esc => {
            app.close_settings();
            KeyAction::Dirty
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.settings_up();
            KeyAction::Dirty
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.settings_down();
            KeyAction::Dirty
        }
        KeyCode::Enter | KeyCode::Char(' ') => match app.settings_action() {
            SettingsAction::Save => save_settings(app, tx),
            SettingsAction::Cancel => {
                app.close_settings();
                KeyAction::Dirty
            }
            _ => {
                app.cycle_selected_setting();
                KeyAction::Dirty
            }
        },
        KeyCode::Char('s') => save_settings(app, tx),
        _ => KeyAction::None,
    }
}

fn handle_modal_key(app: &mut App, tx: &mpsc::Sender<ScanEvent>, key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Char('q') => KeyAction::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyAction::Quit,
        KeyCode::Char('y') | KeyCode::Enter => {
            if let Some(confirm) = app.confirm_leave_root.take() {
                navigate_to(app, tx, confirm.target);
            }
            KeyAction::Dirty
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            app.confirm_leave_root = None;
            KeyAction::Dirty
        }
        _ => KeyAction::None,
    }
}

fn save_settings(app: &mut App, tx: &mpsc::Sender<ScanEvent>) -> KeyAction {
    let draft = app.draft_config();
    match draft.save() {
        Ok(path) => {
            let hidden_changed = app.apply_draft_config();
            if hidden_changed {
                let current = app.current_dir.clone();
                app.clear_cache();
                start_scan(app, tx, current);
            }
            app.status = Some(format!("saved {}", path.display()));
            KeyAction::Dirty
        }
        Err(err) => {
            app.status = Some(format!("error: cannot save config: {err}"));
            KeyAction::Dirty
        }
    }
}

fn navigate_to(app: &mut App, tx: &mpsc::Sender<ScanEvent>, path: PathBuf) {
    if !app.apply_cached(path.clone()) {
        start_scan(app, tx, path);
    }
}

fn start_scan(app: &mut App, tx: &mpsc::Sender<ScanEvent>, path: PathBuf) {
    let scan_id = app.begin_scan(path.clone());
    let show_all = app.config.show_hidden;
    let tx = tx.clone();
    thread::spawn(move || scan_into_channel(tx, show_all, path, scan_id));
}
