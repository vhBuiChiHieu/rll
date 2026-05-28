use std::io::{self, Stdout};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};

use crate::{
    format_duration, format_size, is_hidden, scan_directories_parallel, EntryItem, Summary,
};

// Row data the TUI cares about; decoupled from EntryItem so the scan thread can drop paths.
struct Row {
    type_name: &'static str,
    name: String,
    size: Option<u64>,
}

// Streaming protocol between background scan thread and UI loop.
enum ScanEvent {
    Row(Row),
    Warning(String),
    Done(Summary, Duration),
}

struct App {
    rows: Vec<Row>,
    state: ListState,
    warnings: Vec<String>,
    summary: Option<Summary>,
    elapsed: Option<Duration>,
    scanning: bool,
    // Last rendered list viewport height; used so PgUp/PgDn move by visible page.
    list_height: usize,
}

impl App {
    fn new() -> Self {
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

    fn push_row(&mut self, row: Row) {
        self.rows.push(row);
        // Select the first row as soon as one arrives so arrow keys work immediately.
        if self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    fn move_up(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let i = self.state.selected().unwrap_or(0).saturating_sub(1);
        self.state.select(Some(i));
    }

    fn move_down(&mut self) {
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

    fn move_first(&mut self) {
        if !self.rows.is_empty() {
            self.state.select(Some(0));
        }
    }

    fn move_last(&mut self) {
        if let Some(last) = self.rows.len().checked_sub(1) {
            self.state.select(Some(last));
        }
    }

    fn page_down(&mut self) {
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

    fn page_up(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let page = self.list_height.max(1);
        let i = self.state.selected().unwrap_or(0).saturating_sub(page);
        self.state.select(Some(i));
    }
}

pub(crate) fn run(show_all: bool) -> u8 {
    // Restore terminal state on panic so a crash never leaves the user in raw mode
    // with the alternate screen still active.
    install_panic_hook();

    let mut terminal = match setup_terminal() {
        Ok(t) => t,
        Err(err) => {
            eprintln!("error: cannot initialize TUI: {err}");
            return 1;
        }
    };

    let (tx, rx) = mpsc::channel::<ScanEvent>();
    let scan_handle = thread::spawn(move || scan_into_channel(tx, show_all));

    let mut app = App::new();
    let loop_res = event_loop(&mut terminal, &mut app, rx);

    // Always restore terminal before printing warnings/errors so they land in the
    // user's normal shell instead of the alternate screen.
    let restore_res = restore_terminal(&mut terminal);
    let _ = scan_handle.join();

    for w in &app.warnings {
        eprintln!("{w}");
    }

    match (loop_res, restore_res) {
        (Ok(()), Ok(())) => 0,
        (Err(err), _) | (_, Err(err)) => {
            eprintln!("error: tui: {err}");
            1
        }
    }
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    let _ = terminal.show_cursor();
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original(info);
    }));
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    rx: mpsc::Receiver<ScanEvent>,
) -> io::Result<()> {
    // Cap redraws to ~30fps so a fast scan does not churn the terminal during streaming.
    let frame_budget = Duration::from_millis(33);
    let mut last_draw = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut dirty = true;
    let mut scan_alive = true;

    loop {
        // Drain pending scan events without blocking; ignore disconnect silently because
        // the scan thread joins after the loop returns.
        loop {
            match rx.try_recv() {
                Ok(ScanEvent::Row(row)) => {
                    app.push_row(row);
                    dirty = true;
                }
                Ok(ScanEvent::Warning(w)) => app.warnings.push(w),
                Ok(ScanEvent::Done(summary, elapsed)) => {
                    app.scanning = false;
                    app.summary = Some(summary);
                    app.elapsed = Some(elapsed);
                    dirty = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    scan_alive = false;
                    break;
                }
            }
        }

        if dirty && last_draw.elapsed() >= frame_budget {
            terminal.draw(|f| render(f, app))?;
            last_draw = Instant::now();
            dirty = false;
        }

        // Short poll keeps key latency snappy while still letting scan events stream in.
        let poll_timeout = if scan_alive && app.scanning {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(100)
        };

        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if handle_key(app, key) {
                        return Ok(());
                    }
                    dirty = true;
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

// Returns true when the app should exit.
fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
        KeyCode::Home | KeyCode::Char('g') => app.move_first(),
        KeyCode::End | KeyCode::Char('G') => app.move_last(),
        KeyCode::PageDown | KeyCode::Char('d') => app.page_down(),
        KeyCode::PageUp | KeyCode::Char('u') => app.page_up(),
        _ => {}
    }
    false
}

fn render(f: &mut Frame, app: &mut App) {
    // Vertical stack: title | header | list | footer.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    // Cache the list viewport height so PgUp/PgDn track the visible page.
    app.list_height = chunks[2].height as usize;

    // Title.
    let title = if app.scanning {
        Line::from(vec![
            Span::styled("rll", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  ./  "),
            Span::styled("scanning…", Style::default().fg(Color::Yellow)),
        ])
    } else {
        Line::from(vec![
            Span::styled("rll", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  ./  "),
            Span::styled(
                format!("{} entries", app.rows.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ])
    };
    f.render_widget(Paragraph::new(title), chunks[0]);

    // Column header — same column widths as the CLI table.
    let header = Line::from(Span::styled(
        format!("{:<5} {:<10} {}", "TYPE", "SIZE", "NAME"),
        Style::default().add_modifier(Modifier::DIM),
    ));
    f.render_widget(Paragraph::new(header), chunks[1]);

    // Rows.
    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|r| {
            let size = r.size.map(format_size).unwrap_or_else(|| "?".to_owned());
            let type_color = match r.type_name {
                "DIR" => Color::Cyan,
                "FILE" => Color::White,
                _ => Color::DarkGray,
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<5}", r.type_name),
                    Style::default().fg(type_color),
                ),
                Span::raw(" "),
                Span::styled(format!("{size:<10}"), Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::raw(r.name.clone()),
            ]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");
    // ListState carries the scroll offset; cloning is cheap (single usize + Option<usize>).
    let mut state = app.state.clone();
    f.render_stateful_widget(list, chunks[2], &mut state);
    app.state = state;

    // Footer.
    let footer = if let (Some(summary), Some(elapsed)) = (app.summary, app.elapsed) {
        Line::from(vec![
            Span::styled(
                format!("TOTAL {} ", summary.total()),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "({} files, {} dirs, {} other) ",
                    summary.files, summary.dirs, summary.others
                ),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("in {}", format_duration(elapsed)),
                Style::default().fg(Color::Green),
            ),
            Span::raw("   "),
            Span::styled(
                "↑/↓ navigate · q quit",
                Style::default().fg(Color::DarkGray),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                format!("{} items… ", app.rows.len()),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                "↑/↓ navigate · q quit",
                Style::default().fg(Color::DarkGray),
            ),
        ])
    };
    f.render_widget(Paragraph::new(footer), chunks[3]);
}

fn scan_into_channel(tx: mpsc::Sender<ScanEvent>, show_all: bool) {
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
