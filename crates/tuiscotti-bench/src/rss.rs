//! Peak-RSS sampling without syscalls or foreign dependencies.
//!
//! Linux reads `VmHWM` (peak resident set, kibibytes) from
//! `/proc/self/status` with plain file I/O. Everywhere else there is no
//! portable std-only peak-RSS source, so sampling reports unavailable and
//! samples record `rss=0` with `rss_units="unknown"` — an honest marker,
//! never a guess.

/// Peak resident set size of this process plus its unit.
///
/// Linux only: parsed from `VmHWM` in `/proc/self/status` (kibibytes).
/// `None` when unavailable (non-Linux, unreadable, or unparsable).
#[must_use]
pub fn peak_rss() -> Option<(u64, &'static str)> {
    #[cfg(target_os = "linux")]
    {
        linux_peak_rss()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Parse `VmHWM` out of `/proc/self/status`.
#[cfg(target_os = "linux")]
fn linux_peak_rss() -> Option<(u64, &'static str)> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = text.lines().find(|l| l.starts_with("VmHWM:"))?;
    let value = line
        .strip_prefix("VmHWM:")?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    Some((value, "kibibytes"))
}
