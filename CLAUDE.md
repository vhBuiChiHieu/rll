# CLAUDE.md

## Project

`rll` is a std-only Rust CLI that lists direct entries in the current directory. It computes recursive sizes for direct directories and prioritizes fast output.

## Toolchain

- Default toolchain: `stable-x86_64-pc-windows-gnullvm` (self-contained; no Visual Studio Build Tools or MinGW required on Windows).
- `.cargo/config.toml` pins `linker = "rust-lld"` and `rustflags = ["-C", "target-feature=+crt-static"]` so the release binary is statically linked and has no runtime DLL dependencies.
- First-time setup on a new machine: `rustup toolchain install stable-x86_64-pc-windows-gnullvm && rustup default stable-x86_64-pc-windows-gnullvm`.

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

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **rll** (133 symbols, 290 relationships, 20 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

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
