use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("rll-{name}-{}-{nonce}", std::process::id()));
    fs::create_dir(&path).unwrap();
    path
}

fn rll_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rll"))
}

#[test]
fn lists_direct_files_and_directories() {
    let dir = temp_dir("basic");
    fs::write(dir.join("a.txt"), b"hello").unwrap();
    fs::create_dir(dir.join("src")).unwrap();

    let output = rll_command().current_dir(&dir).output().unwrap();

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.starts_with("TYPE  SIZE       NAME\n"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.lines().any(|line| line == "FILE  5 B        a.txt"),
        "stdout: {stdout}"
    );
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("DIR") && line.ends_with(" src")),
        "stdout: {stdout}"
    );
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("TOTAL 2 entries (1 files, 1 dirs, 0 other) in ")),
        "stdout: {stdout}"
    );

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn shows_recursive_directory_size_without_listing_nested_entries() {
    let dir = temp_dir("dir-size");
    let nested = dir.join("nested");
    let child_dir = nested.join("child");
    fs::create_dir(&nested).unwrap();
    fs::create_dir(&child_dir).unwrap();
    fs::write(nested.join("file.bin"), [0_u8; 10]).unwrap();
    fs::write(child_dir.join("child.bin"), [0_u8; 20]).unwrap();

    let output = rll_command().current_dir(&dir).output().unwrap();

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.lines().any(|line| line == "DIR   30 B       nested"),
        "stdout: {stdout}"
    );
    assert!(!stdout.contains("file.bin"), "stdout: {stdout}");
    assert!(!stdout.contains("child.bin"), "stdout: {stdout}");

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn stdout_has_only_table_output_on_success() {
    let dir = temp_dir("channels");
    fs::write(dir.join("file.bin"), [0_u8; 12]).unwrap();

    let output = rll_command().current_dir(&dir).output().unwrap();

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().next(), Some("TYPE  SIZE       NAME"));
    assert!(!stdout.contains("warning:"), "stdout: {stdout}");
    assert!(!stdout.contains("error:"), "stdout: {stdout}");

    fs::remove_dir_all(dir).unwrap();
}
