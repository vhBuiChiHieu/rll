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
fn does_not_list_nested_entries() {
    let dir = temp_dir("nonrecursive");
    let nested = dir.join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("child.txt"), b"hidden from mvp").unwrap();

    let output = rll_command().current_dir(&dir).output().unwrap();

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.lines().any(|line| line.ends_with(" nested")),
        "stdout: {stdout}"
    );
    assert!(!stdout.contains("child.txt"), "stdout: {stdout}");

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
