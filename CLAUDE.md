# CLAUDE.md

## Project

`rll` is a std-only Rust CLI that lists direct entries in the current directory. It computes recursive sizes for direct directories and prioritizes fast output.

## Commands

```bash
cargo fmt --check
cargo test
cargo build --release
./target/release/rll.exe
./target/release/rll.exe --o asc
./target/release/rll.exe --o desc
```

Perf checks:

```bash
rustc scripts/check_perf.rs -O -o target/check_perf.exe
./target/check_perf.exe ./target/release/rll.exe 10000 5
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/measure_windows.ps1 -Binary target/release/rll.exe -Entries 10000 -Runs 5
```

## CLI behavior

- Default command scans current directory only and prints direct entries plus final summary.
- `--o asc|desc` sorts direct entries by computed size ascending/descending.
- Invalid `--o` values exit non-zero and print `error: --o requires asc or desc` to stderr.
- Access-denied nested directories are skipped with stderr warnings; elevated terminal may reduce warnings on Windows.

## Architecture

- `src/main.rs`: process entrypoint; maps library exit code to `ExitCode`.
- `src/lib.rs`: arg parsing, direct entry scan, parallel top-level directory sizing, output formatting, summary line, core unit tests.
- `tests/cli.rs`: integration tests through compiled `rll` binary; use temp dirs for deterministic file sizes and ordering assertions.
- `scripts/check_perf.rs`: std-only wall-time and binary-size check.
- `scripts/measure_windows.ps1`: Windows peak working-set measurement.

## Constraints

- Runtime dependencies: none.
- Scan only `.`; no path argument in MVP.
- List only direct entries; never print nested entries.
- Compute directory size recursively by summing nested file metadata sizes.
- Do not sort by default; parallel directory results may print out of filesystem order.
- Preserve fast streaming when no sort option is used; only buffer rows when sorting is requested.
- When sorted output buffers rows, flush the header before scanning so stderr warnings cannot appear before the table header.
- Use std-only worker threads for direct directory sizing.
- Default worker count is half of `thread::available_parallelism()`, minimum `1`.
- Use explicit DFS stack for recursive directory sizing; do not use recursive function calls.
- Use `DirEntry::file_type()` for `FILE`/`DIR`/`OTHER`.
- Use metadata for size; never read file contents.
- Keep stdout parseable: table rows plus final `TOTAL ... in ...` summary.
- Use stderr only for warnings/errors.
- Treat stdout `BrokenPipe` as success exit code `0`.
