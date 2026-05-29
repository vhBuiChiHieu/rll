// OS integration: open with the default app, reveal in the file manager, and copy
// a path to the clipboard. Implemented with std::process only — no extra crates — so
// the TUI dependency surface stays at ratatui + sysinfo. Each function returns a short
// label on success for the footer status line.

use std::io;
use std::path::Path;
use std::process::Command;

// Open the selected entry with the OS default handler (file → associated app, dir → file manager).
#[cfg(windows)]
pub(crate) fn open_path(path: &Path) -> io::Result<&'static str> {
    // `start` is a cmd builtin; the empty "" is its title argument so a quoted path
    // is not mistaken for the window title.
    Command::new("cmd")
        .args(["/C", "start", "", &path.display().to_string()])
        .spawn()
        .map(|_| "opened")
}

// Reveal the entry in Explorer with it pre-selected in its parent folder.
#[cfg(windows)]
pub(crate) fn reveal_path(path: &Path) -> io::Result<&'static str> {
    // explorer expects "/select,<path>" as a single argument.
    Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map(|_| "revealed")
}

// Copy the full path to the clipboard via the built-in `clip` utility (reads stdin).
#[cfg(windows)]
pub(crate) fn copy_path(path: &Path) -> io::Result<&'static str> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("clip").stdin(Stdio::piped()).spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(path.display().to_string().as_bytes())?;
    }
    child.wait()?;
    Ok("copied path")
}

// Non-Windows fallbacks (best effort): xdg-open/open for navigation, xclip/pbcopy for clipboard.
#[cfg(not(windows))]
pub(crate) fn open_path(path: &Path) -> io::Result<&'static str> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(opener).arg(path).spawn().map(|_| "opened")
}

#[cfg(not(windows))]
pub(crate) fn reveal_path(path: &Path) -> io::Result<&'static str> {
    // No portable "select in folder"; open the containing directory instead.
    let dir = path.parent().unwrap_or(path);
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(opener).arg(dir).spawn().map(|_| "revealed")
}

#[cfg(not(windows))]
pub(crate) fn copy_path(path: &Path) -> io::Result<&'static str> {
    use std::io::Write;
    use std::process::Stdio;

    let (program, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("pbcopy", &[])
    } else {
        ("xclip", &["-selection", "clipboard"])
    };
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(path.display().to_string().as_bytes())?;
    }
    child.wait()?;
    Ok("copied path")
}
