// Event loop: drains scan events, throttles redraws, and maps keys to navigation.

use std::io::{self, Stdout};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Terminal;

use super::app::App;
use super::render::render;
use super::scan::ScanEvent;

pub(crate) fn event_loop(
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
