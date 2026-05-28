# CLAUDE.md

## Project

`rll` is a Rust CLI that lists direct entries in the current directory. It computes recursive sizes for direct directories and prioritizes fast output. A `tui` subcommand opens an interactive list view powered by `ratatui` + `crossterm` (the only runtime crate deps).

## Toolchain

- Default toolchain: `stable-x86_64-pc-windows-gnullvm` (self-contained; no Visual Studio Build Tools or MinGW required on Windows).
- `.cargo/config.toml` pins `linker = "rust-lld"` and `rustflags = ["-C", "target-feature=+crt-static"]` so the release binary is statically linked and has no runtime DLL dependencies.
- `build.rs` generates empty `ar` stub archives for Windows import libs that the gnullvm self-contained sysroot omits (`advapi32`, `cfgmgr32`, `gdi32`, `msimg32`, `opengl32`, `synchronization`, `winspool`). `crossterm`/`winapi` reference these via `#[link(name = "...")]` but never call into them on our code paths, so empty stubs satisfy `rust-lld` without pulling in MinGW. A future dep that actually calls into one of these DLLs will surface as an "undefined symbol" link error, signaling we need real import libs.
- First-time setup on a new machine: `rustup toolchain install stable-x86_64-pc-windows-gnullvm && rustup default stable-x86_64-pc-windows-gnullvm`.

## Commands

```bash
cargo fmt --check
cargo test
cargo build --release
./target/release/rll.exe
./target/release/rll.exe --o asc
./target/release/rll.exe --o desc
./target/release/rll.exe tui
./target/release/rll.exe tui --a
```

Perf checks:

```bash
# `-C linker=rust-lld -C target-feature=+crt-static` mirrors .cargo/config.toml so the
# script binary is statically linked; without them it exits 0xC0000135 STATUS_DLL_NOT_FOUND on Windows.
rustc scripts/check_perf.rs -O -C linker=rust-lld -C target-feature=+crt-static -o target/check_perf.exe
./target/check_perf.exe ./target/release/rll.exe 10000 5
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/measure_windows.ps1 -Binary target/release/rll.exe -Entries 10000 -Runs 5
```

## CLI behavior

- Default command scans current directory only and prints direct entries plus final summary.
- `--o asc|desc` sorts direct entries by computed size ascending/descending.
- `--a` / `--all` includes dotfile entries; without it, names beginning with `.` are skipped at every depth so reported sizes/counts ignore hidden subtrees.
- `--n N` truncates the printed rows to the first `N` after sorting. When `--o` is omitted, `--n` implies descending size order so the largest entries survive truncation; the `TOTAL` summary still reflects every visited entry.
- `--json` emits NDJSON: one `{"type":..,"name":..,"size":..}` object per row and a final `{"summary":{..,"duration_ns":..}}` line. Table header and `TOTAL` text are suppressed in this mode.
- Invalid `--o` values exit non-zero and print `error: --o requires asc or desc` to stderr; invalid `--n` values exit non-zero with `error: --n requires a positive integer`.
- Access-denied nested directories are skipped with stderr warnings; elevated terminal may reduce warnings on Windows.
- Final `TOTAL` summary counts every entry visited by the recursive scan (direct entries plus everything under each direct directory), not just direct children. `entries` stays equal to `files + dirs + other`.

## TUI behavior (`rll tui`)

- Launches a full-screen interactive list. Selection starts on the first row as soon as the first entry streams in; the scan continues in the background and pushes rows into the list via an `mpsc` channel.
- Layout: `title | header | list | footer`. Title shows `rll  <current path>` and either `scanning…` or the final entry count. Footer shows the `TOTAL` summary once the scan completes plus the keybinding hint.
- Keys: `↑/k` up, `↓/j` down, `Home/g` first, `End/G` last, `PgUp/u` page up, `PgDn/d` page down, `Enter/l` open selected directory, `Backspace/h` parent directory, `r` reload current directory, `c` settings, `q`/`Esc`/`Ctrl-C` quit. Press events only (avoids duplicate moves on Windows key-release).
- Settings screen: `Enter`/`Space` cycles values, `s` saves, `Esc` cancels; supports hidden files plus sort field `unsorted|name|size|type` and direction `asc|desc`.
- TUI settings persist in `%APPDATA%\rll\config` on Windows, `$XDG_CONFIG_HOME/rll/config` or `$HOME/.config/rll/config` elsewhere; `rll tui --a` overrides hidden files for that session only.
- Parent navigation may leave the launch directory; crossing above the initial root shows an in-TUI confirmation modal (`y`/`Enter` confirm, `n`/`Esc` cancel).
- Completed directory scans are cached by path for instant revisits; `r` drops the current directory cache entry and scans again.
- Honors `--a`/`--all` for hidden entries; other CLI flags (`--o`, `--n`, `--json`) are not consumed by the TUI path in this phase.
- Restores raw mode and leaves the alternate screen on normal exit, on `Err`, and on panic (panic hook installed before terminal setup).
- Warnings collected during the scan are buffered and printed to stderr after the TUI exits, so they never garble the alternate screen.
- Render is capped to ~30fps and is event-driven: redraw fires only on new scan rows, key/resize events, or scan completion.

## Architecture

- `src/main.rs`: process entrypoint; maps library exit code to `ExitCode`.
- `src/lib.rs`: crate root. Holds `run_stdio`/`run`/`run_with_args` entrypoints, declares the modules, and dispatches `Mode::Cli` → `output::run_path` vs `Mode::Tui` → `tui::run`. Re-exports `format_size` as the public API.
- `src/cli.rs`: arg parsing — `Options`, `SortOrder`, `Mode`, `Options::parse` (including the `tui` subcommand token), `buffer_rows`/`effective_order` policy.
- `src/config.rs`: std-only persisted config for TUI settings; line-based key/value format and platform config path resolution.
- `src/scan.rs`: scan engine — `EntryItem`, `DirectoryResult`, `Summary`, `NestedCounts`, `ParallelScan`, `is_hidden`, and the work-stealing `scan_directories_parallel` plus its `ScanState`/`worker_loop`/`scan_one_level`/`worker_count` internals. Std-only; no crate deps.
- `src/format.rs`: human-readable `format_size` (public) and `format_duration` (`pub(crate)`).
- `src/output.rs`: CLI rendering — `run_path`, `write_entries`, table/NDJSON writers, row buffering/sorting/truncation, summary line, JSON string escaper. Owns the CLI-path unit tests.
- Cross-module reuse: `cli`, `scan`, and `format` items the TUI and output paths share are `pub(crate)`; the TUI submodules import the scan + format layer directly via `crate::scan::*` / `crate::format::*`.
- `src/tui/`: ratatui + crossterm interactive mode, split into submodules:
  - `mod.rs`: `pub(crate) run`, terminal lifecycle (raw mode, alternate screen, panic hook), channel setup, and initial root capture.
  - `app.rs`: `App` UI state, `Row`, cached directory snapshots, confirm-modal state, and list-selection navigation (`move_*`/`page_*`). No ratatui drawing.
  - `render.rs`: `render(frame, app)` — the `title | header | list | footer` ratatui layout plus confirmation modal overlay.
  - `event.rs`: `event_loop` (scan-event drain, ~30fps throttle), scan spawning, stale `scan_id` filtering, directory navigation, reload, and key mapping.
  - `scan.rs`: `ScanEvent` protocol and path-aware `scan_into_channel` background thread bridging `crate::scan` into the UI.
- `build.rs`: writes empty `ar` stub archives for gnullvm-missing import libs (see Toolchain).
- `tests/cli.rs`: integration tests through compiled `rll` binary; use temp dirs for deterministic file sizes and ordering assertions.
- `scripts/check_perf.rs`: std-only wall-time and binary-size check.
- `scripts/measure_windows.ps1`: Windows peak working-set measurement.

## Maintenance

- Before committing, check whether recent code changes require syncing `CLAUDE.md`.

## Constraints

- Runtime dependencies (CLI path): none — std-only.
- Runtime dependencies (TUI path): `ratatui` (with the `crossterm` feature, default features off). No other crate deps; the TUI module reuses the std-only scan layer.
- Scan only `.`; no path argument in MVP.
- List only direct entries; never print nested entries.
- Compute directory size recursively by summing nested file metadata sizes.
- Do not sort by default; parallel directory results may print out of filesystem order.
- Preserve fast streaming when no sort option is used; only buffer rows when sorting is requested.
- When sorted output buffers rows, flush the header before scanning so stderr warnings cannot appear before the table header.
- Use std-only worker threads for direct directory sizing.
- Default worker count is `thread::available_parallelism()`, minimum `1` (directory traversal is I/O-bound, so the full hint overlaps read latency better than half). Override with `RLL_WORKERS` env var (positive integer); invalid values fall back to default.
- Cache per-file size via `DirEntry::metadata().len()` during the top-level scan (stored on `EntryItem.size_hint`); never re-stat the path afterwards. Avoids a redundant syscall on Windows where the size is already in the `FindNextFile` data.
- Use a single shared work-stealing queue (`Mutex<Vec<ScanTask>>` + `Condvar`) for recursive directory sizing; workers scan one directory level at a time and push discovered subdirs back to the shared queue so peers can steal them. Avoids per-worker DFS stacks where one giant subtree starves the pool.
- Use `DirEntry::file_type()` for `FILE`/`DIR`/`OTHER`.
- Use metadata for size; never read file contents.
- Keep stdout parseable: table rows plus final `TOTAL ... in ...` summary.
- Recursive `TOTAL` counts are aggregated in `ScanState` via per-type `AtomicU64` counters (`nested_files`, `nested_dirs`, `nested_others`) updated with `Relaxed` ordering by workers, then folded into the top-level `Summary` after `scan_directories_parallel` returns. Nested entries whose `DirEntry::file_type()` fails emit a warning and are skipped (not counted), matching pre-existing nested error handling; top-level filetype failures still count as `OTHER`.
- Use stderr only for warnings/errors.
- Treat stdout `BrokenPipe` as success exit code `0`.

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **rll** (284 symbols, 694 relationships, 24 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/rll/context` | Codebase overview, check index freshness |
| `gitnexus://repo/rll/clusters` | All functional areas |
| `gitnexus://repo/rll/processes` | All execution flows |
| `gitnexus://repo/rll/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
