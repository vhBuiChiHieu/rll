// Human-readable size and duration formatting.

use std::fmt;
use std::time::Duration;

pub fn format_size(bytes: u64) -> String {
    Size(bytes).to_string()
}

struct Size(u64);

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const KIB: u64 = 1024;
        const MIB: u64 = KIB * 1024;
        const GIB: u64 = MIB * 1024;

        match self.0 {
            0..=1023 => write!(f, "{} B", self.0),
            KIB..=1_048_575 => write!(f, "{:.1} KiB", self.0 as f64 / KIB as f64),
            MIB..=1_073_741_823 => write!(f, "{:.1} MiB", self.0 as f64 / MIB as f64),
            _ => write!(f, "{:.1} GiB", self.0 as f64 / GIB as f64),
        }
    }
}

pub(crate) fn format_duration(duration: Duration) -> String {
    let nanos = duration.as_nanos();

    if nanos < 1_000 {
        format!("{nanos} ns")
    } else if nanos < 1_000_000 {
        format!("{:.3} µs", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.3} ms", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.3} s", nanos as f64 / 1_000_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::format_size;

    #[test]
    fn formats_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(912), "912 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn formats_kib_mib_gib() {
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(20 * 1024 * 1024 + 314_572), "20.3 MiB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }
}
