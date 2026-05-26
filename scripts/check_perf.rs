use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_ENTRIES: usize = 10_000;
const DEFAULT_RUNS: usize = 5;
const MAX_AVG_PER_ENTRY_NS: u128 = 200_000;

fn main() -> io::Result<()> {
    let mut args = env::args().skip(1);
    let binary = args.next().unwrap_or_else(default_binary_path);
    let entries = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_ENTRIES);
    let runs = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_RUNS);

    let binary_path = fs::canonicalize(binary)?;
    let binary_size = fs::metadata(&binary_path)?.len();
    let bench_dir = create_bench_dir(entries)?;

    let mut durations = Vec::with_capacity(runs);
    let mut output_bytes = 0_u64;

    for _ in 0..runs {
        let start = Instant::now();
        let output = Command::new(&binary_path)
            .current_dir(&bench_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?;
        let elapsed = start.elapsed();

        if !output.status.success() {
            eprintln!("rll failed with status {:?}", output.status.code());
            eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
            fs::remove_dir_all(&bench_dir)?;
            std::process::exit(1);
        }

        output_bytes = output.stdout.len() as u64;
        durations.push(elapsed);
    }

    let total_nanos: u128 = durations.iter().map(Duration::as_nanos).sum();
    let avg_nanos = total_nanos / runs as u128;
    let avg_per_entry = avg_nanos / entries.max(1) as u128;

    println!("entries: {entries}");
    println!("runs: {runs}");
    println!("avg_wall_time_ms: {:.3}", avg_nanos as f64 / 1_000_000.0);
    println!("avg_per_entry_ns: {avg_per_entry}");
    println!("output_bytes: {output_bytes}");
    println!("binary_size_bytes: {binary_size}");
    println!("peak_memory: {}", memory_note());

    fs::remove_dir_all(&bench_dir)?;

    if avg_per_entry > MAX_AVG_PER_ENTRY_NS {
        eprintln!("avg_per_entry_ns {avg_per_entry} exceeds conservative limit {MAX_AVG_PER_ENTRY_NS}");
        std::process::exit(1);
    }

    Ok(())
}

fn default_binary_path() -> String {
    if cfg!(windows) {
        "target/release/rll.exe".to_owned()
    } else {
        "target/release/rll".to_owned()
    }
}

fn create_bench_dir(entries: usize) -> io::Result<PathBuf> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = env::temp_dir().join(format!("rll-perf-{}-{nonce}", std::process::id()));
    fs::create_dir(&dir)?;

    for index in 0..entries {
        fs::write(dir.join(format!("file-{index:06}.txt")), b"0123456789abcdef")?;
    }

    Ok(dir)
}

fn memory_note() -> &'static str {
    if cfg!(target_os = "linux") {
        "run `/usr/bin/time -v target/release/rll >/dev/null` in benchmark dir for Maximum resident set size"
    } else if cfg!(target_os = "macos") {
        "run `/usr/bin/time -l target/release/rll >/dev/null` in benchmark dir for maximum resident set size"
    } else if cfg!(windows) {
        "not sampled by std-only script on Windows; use PowerShell process counters for peak working set"
    } else {
        "unsupported by std-only script on this OS"
    }
}
