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

#[test]
fn sorts_entries_by_size_ascending() {
    let dir = temp_dir("sort-asc");
    fs::write(dir.join("small.txt"), [0_u8; 1]).unwrap();
    fs::write(dir.join("large.txt"), [0_u8; 10]).unwrap();
    fs::create_dir(dir.join("medium")).unwrap();
    fs::write(dir.join("medium").join("file.bin"), [0_u8; 5]).unwrap();

    let output = rll_command()
        .args(["--o", "asc"])
        .current_dir(&dir)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<_> = stdout.lines().skip(1).take(3).collect();
    assert_eq!(
        rows,
        [
            "FILE  1 B        small.txt",
            "DIR   5 B        medium",
            "FILE  10 B       large.txt"
        ]
    );

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sorts_entries_by_size_descending() {
    let dir = temp_dir("sort-desc");
    fs::write(dir.join("small.txt"), [0_u8; 1]).unwrap();
    fs::write(dir.join("large.txt"), [0_u8; 10]).unwrap();
    fs::create_dir(dir.join("medium")).unwrap();
    fs::write(dir.join("medium").join("file.bin"), [0_u8; 5]).unwrap();

    let output = rll_command()
        .args(["--o", "desc"])
        .current_dir(&dir)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<_> = stdout.lines().skip(1).take(3).collect();
    assert_eq!(
        rows,
        [
            "FILE  10 B       large.txt",
            "DIR   5 B        medium",
            "FILE  1 B        small.txt"
        ]
    );

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rejects_invalid_order_option() {
    let output = rll_command().args(["--o", "bad"]).output().unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: --o requires asc or desc\n"
    );
}

#[test]
fn hides_dotfiles_by_default() {
    let dir = temp_dir("hide-dot");
    fs::write(dir.join("visible.txt"), [0_u8; 3]).unwrap();
    fs::write(dir.join(".secret"), [0_u8; 7]).unwrap();
    fs::create_dir(dir.join(".cache")).unwrap();
    fs::write(dir.join(".cache").join("inner.bin"), [0_u8; 99]).unwrap();

    let output = rll_command().current_dir(&dir).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains(".secret"), "stdout: {stdout}");
    assert!(!stdout.contains(".cache"), "stdout: {stdout}");
    assert!(stdout.contains("visible.txt"), "stdout: {stdout}");
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("TOTAL 1 entries (1 files, 0 dirs, 0 other) in ")),
        "stdout: {stdout}"
    );

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn shows_dotfiles_with_all_flag() {
    let dir = temp_dir("show-all");
    fs::write(dir.join("visible.txt"), [0_u8; 3]).unwrap();
    fs::write(dir.join(".secret"), [0_u8; 7]).unwrap();
    fs::create_dir(dir.join(".cache")).unwrap();
    fs::write(dir.join(".cache").join("inner.bin"), [0_u8; 99]).unwrap();

    let output = rll_command()
        .args(["--a"])
        .current_dir(&dir)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(".secret"), "stdout: {stdout}");
    assert!(
        stdout.lines().any(|line| line == "DIR   99 B       .cache"),
        "stdout: {stdout}"
    );
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("TOTAL 4 entries (3 files, 1 dirs, 0 other) in ")),
        "stdout: {stdout}"
    );

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn limits_rows_with_top_n() {
    let dir = temp_dir("top-n");
    fs::write(dir.join("a.txt"), [0_u8; 1]).unwrap();
    fs::write(dir.join("b.txt"), [0_u8; 2]).unwrap();
    fs::write(dir.join("c.txt"), [0_u8; 3]).unwrap();
    fs::write(dir.join("d.txt"), [0_u8; 4]).unwrap();

    let output = rll_command()
        .args(["--o", "desc", "--n", "2"])
        .current_dir(&dir)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<_> = stdout
        .lines()
        .skip(1)
        .take_while(|l| !l.starts_with("TOTAL"))
        .collect();
    assert_eq!(rows, ["FILE  4 B        d.txt", "FILE  3 B        c.txt"]);
    // Summary still reflects every visited entry, not the truncated row count.
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("TOTAL 4 entries (4 files, 0 dirs, 0 other) in ")),
        "stdout: {stdout}"
    );

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rejects_invalid_top_n() {
    let output = rll_command().args(["--n", "0"]).output().unwrap();
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: --n requires a positive integer\n"
    );

    let output = rll_command().args(["--n", "abc"]).output().unwrap();
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: --n requires a positive integer\n"
    );

    let output = rll_command().args(["--n", "-5"]).output().unwrap();
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: --n requires a positive integer\n"
    );
}

#[test]
fn top_n_without_order_implies_descending() {
    let dir = temp_dir("top-n-implicit");
    fs::write(dir.join("small.txt"), [0_u8; 1]).unwrap();
    fs::write(dir.join("mid.txt"), [0_u8; 5]).unwrap();
    fs::create_dir(dir.join("bigdir")).unwrap();
    fs::write(dir.join("bigdir").join("blob.bin"), [0_u8; 99]).unwrap();

    let output = rll_command()
        .args(["--n", "2"])
        .current_dir(&dir)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<_> = stdout
        .lines()
        .skip(1)
        .take_while(|l| !l.starts_with("TOTAL"))
        .collect();
    // Without --o, --n must surface the biggest entries first so the directory
    // (which arrives last in scan order) is not truncated away.
    assert_eq!(
        rows,
        ["DIR   99 B       bigdir", "FILE  5 B        mid.txt"]
    );
    // TOTAL still reflects every visited entry, not the truncated row count.
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("TOTAL 4 entries (3 files, 1 dirs, 0 other) in ")),
        "stdout: {stdout}"
    );

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn emits_ndjson_output() {
    let dir = temp_dir("json");
    fs::write(dir.join("a.txt"), [0_u8; 5]).unwrap();
    fs::create_dir(dir.join("sub")).unwrap();
    fs::write(dir.join("sub").join("inner.bin"), [0_u8; 12]).unwrap();

    let output = rll_command()
        .args(["--json"])
        .current_dir(&dir)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("TYPE"), "stdout: {stdout}");
    assert!(!stdout.contains("TOTAL"), "stdout: {stdout}");

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "stdout: {stdout}");

    assert!(
        lines
            .iter()
            .any(|l| l == &r#"{"type":"FILE","name":"a.txt","size":5}"#),
        "stdout: {stdout}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l == &r#"{"type":"DIR","name":"sub","size":12}"#),
        "stdout: {stdout}"
    );

    let summary = lines.last().unwrap();
    assert!(
        summary
            .starts_with(r#"{"summary":{"entries":3,"files":2,"dirs":1,"other":0,"duration_ns":"#),
        "summary: {summary}"
    );
    assert!(summary.ends_with("}}"));

    fs::remove_dir_all(dir).unwrap();
}
