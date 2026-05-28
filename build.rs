// Stub import-library shim for the x86_64-pc-windows-gnullvm target.
//
// The self-contained gnullvm sysroot ships only a minimal set of MinGW import libs
// (kernel32, user32, ws2_32, dbghelp, msvcrt, ntdll, userenv, mingw32, mingwex, unwind).
// Transitive dependencies such as `winapi` reference additional system libraries via
// `#[link(name = "...")]`, which forces rust-lld to resolve `-l<name>` against an
// import library that does not exist in the sysroot. The link then aborts with
// "unable to find library -l<name>".
//
// crossterm/winapi never actually call into these DLLs from the code paths we use,
// so the references are dead at the COFF level. Providing an empty `ar` archive for
// each missing name satisfies rust-lld's library lookup without contributing any
// real symbols. If a future dependency does require a symbol from one of these
// DLLs the link will fail with a clean "undefined symbol" error, signaling that we
// need real import libs (e.g. via mingw-w64 dlltool).
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("windows-gnullvm") {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR set by cargo"));
    let stub_dir = out_dir.join("gnullvm-link-stubs");
    fs::create_dir_all(&stub_dir).expect("create gnullvm-link-stubs dir");

    // Eight-byte signature is a valid empty ar archive accepted by rust-lld.
    const EMPTY_AR: &[u8] = b"!<arch>\n";
    const MISSING: &[&str] = &[
        "advapi32",
        "cfgmgr32",
        "gdi32",
        "msimg32",
        "opengl32",
        "synchronization",
        "winspool",
    ];

    for lib in MISSING {
        let path = stub_dir.join(format!("lib{lib}.a"));
        if !path.exists() {
            fs::write(&path, EMPTY_AR).expect("write empty ar stub");
        }
    }

    println!("cargo:rustc-link-search=native={}", stub_dir.display());
}
