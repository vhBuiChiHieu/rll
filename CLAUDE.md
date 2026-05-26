# CLAUDE.md

## Project

`rll` is a std-only Rust CLI that lists direct entries in the current directory. It computes recursive sizes for direct directories and prioritizes fast output.

## Commands

```bash
cargo fmt --check
cargo test
cargo build --release
./target/release/rll.exe
```

Perf checks:

```bash
rustc scripts/check_perf.rs -O -o target/check_perf.exe
./target/check_perf.exe ./target/release/rll.exe 10000 5
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/measure_windows.ps1 -Binary target/release/rll.exe -Entries 10000 -Runs 5
```

## Architecture

- `src/main.rs`: process entrypoint; maps library exit code to `ExitCode`.
- `src/lib.rs`: direct entry scan, parallel top-level directory sizing, output formatting, summary line, tests for core behavior.
- `tests/cli.rs`: integration tests through compiled `rll` binary.
- `scripts/check_perf.rs`: std-only wall-time and binary-size check.
- `scripts/measure_windows.ps1`: Windows peak working-set measurement.

## Constraints

- Runtime dependencies: none.
- Scan only `.`; no path argument in MVP.
- List only direct entries; never print nested entries.
- Compute directory size recursively by summing nested file metadata sizes.
- Do not sort by default; parallel directory results may print out of filesystem order.
- Use std-only worker threads for direct directory sizing.
- Default worker count is half of `thread::available_parallelism()`, minimum `1`.
- Use explicit DFS stack for recursive directory sizing; do not use recursive function calls.
- Use `DirEntry::file_type()` for `FILE`/`DIR`/`OTHER`.
- Use metadata for size; never read file contents.
- Keep stdout parseable: table rows plus final `TOTAL ... in ...` summary.
- Use stderr only for warnings/errors.
- Treat stdout `BrokenPipe` as success exit code `0`.
