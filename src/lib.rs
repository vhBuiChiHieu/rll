use std::env;
use std::io::{self, Write};

mod cli;
mod config;
mod format;
mod output;
mod scan;
mod tui;

use cli::{Mode, Options};

pub use format::format_size;

pub fn run_stdio() -> u8 {
    let stdout = io::stdout();
    let stderr = io::stderr();
    run_with_args(env::args().skip(1), stdout.lock(), stderr.lock())
}

pub fn run<W, E>(stdout: W, stderr: E) -> u8
where
    W: Write,
    E: Write,
{
    run_with_args(std::iter::empty::<&str>(), stdout, stderr)
}

pub fn run_with_args<I, S, W, E>(args: I, stdout: W, mut stderr: E) -> u8
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    W: Write,
    E: Write,
{
    let parsed = match Options::parse(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            let _ = writeln!(stderr, "error: {err}");
            return 1;
        }
    };

    match parsed.mode {
        Mode::Cli => output::run_path(".", parsed, stdout, stderr),
        Mode::Tui => tui::run(parsed.show_all),
    }
}
