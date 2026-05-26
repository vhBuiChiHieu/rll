# rll

`rll` is a small Rust CLI for listing direct entries in the current directory with file sizes, recursive directory sizes, and a final total.

## Features

- Lists only direct entries in the current directory.
- Computes directory sizes recursively.
- Streams output by default for fast feedback.
- Supports size sorting with `--o asc` and `--o desc`.
- Uses only Rust standard library dependencies.

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
rll
rll --o asc
rll --o desc
```

Invalid ordering values exit with a non-zero status and print:

```text
error: --o requires asc or desc
```

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
