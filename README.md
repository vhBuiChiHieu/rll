# rll

`rll` is a small Rust CLI for listing direct entries in the current directory with file sizes, recursive directory sizes, and a final total. It also ships an interactive `tui` subcommand.

## Features

- Lists only direct entries in the current directory.
- Computes directory sizes recursively with a parallel work-stealing scan.
- Streams output by default for fast feedback.
- Size sorting with `--o asc` / `--o desc`.
- Hidden entries via `--a` / `--all`; row cap via `--n N`; machine output via `--json` (NDJSON).
- Interactive full-screen list via `rll tui`.
- CLI path is std-only (no crate dependencies); the `tui` path adds `ratatui` + `crossterm`.

## Install

### GitHub Releases

Current release distribution targets Windows only.

1. Open the repository's GitHub Releases page.
2. Download the Windows binary for `v1.0.0`.
3. Rename it to `rll.exe` if needed.
4. Put `rll.exe` somewhere on your `PATH`.

Linux and macOS binaries are not published yet.

### Build from source

Requires Rust toolchain.

```bash
cargo build --release
```

The binary will be available at:

```bash
target/release/rll.exe
```

## Usage

```bash
rll                 # list direct entries + TOTAL summary
rll --o asc         # sort by computed size ascending
rll --o desc        # sort by computed size descending
rll --a             # include dotfile entries (alias: --all)
rll --n 20          # cap to first 20 rows (implies desc when --o omitted)
rll --json          # NDJSON: one object per row + final summary line
rll tui             # interactive full-screen list
rll tui --a         # interactive list including dotfiles
```

Invalid option values exit with a non-zero status:

```text
error: --o requires asc or desc
error: --n requires a positive integer
```

### TUI keys

| Key | Action |
|-----|--------|
| `↑` / `k` | up |
| `↓` / `j` | down |
| `Home` / `g` | first |
| `End` / `G` | last |
| `PgUp` / `u` | page up |
| `PgDn` / `d` | page down |
| `q` / `Esc` / `Ctrl-C` | quit |

## Release checklist

For `v1.0.0` Windows release:

```bash
cargo fmt --check
cargo test
cargo build --release
```

Then create a GitHub Release with tag `v1.0.0` and upload:

```text
target/release/rll.exe
```

Suggested asset name:

```text
rll-v1.0.0-x86_64-pc-windows-msvc.exe
```
