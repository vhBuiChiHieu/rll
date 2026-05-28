// Interactive TUI entrypoint and terminal lifecycle (raw mode, alternate screen,
// panic hook). Submodules own state, rendering, event handling, and the scan bridge.

use std::io::{self, Stdout};
use std::sync::mpsc;
use std::thread;

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::Terminal;

mod app;
mod event;
mod render;
mod scan;

use app::App;
use scan::{scan_into_channel, ScanEvent};

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
    let loop_res = event::event_loop(&mut terminal, &mut app, rx);

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
