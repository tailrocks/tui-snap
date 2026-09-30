//! Pid liveness + signaling via a trusted `kill(1)`.
//!
//! Split out of `sessions.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use super::super::OpError;
#[cfg(unix)]
use super::super::validate_pid;
#[cfg(unix)]
use super::types::KILL_BINARIES;

/// Trusted `kill(1)` without `PATH`: the first absolute candidate that is a
/// regular file. Final wiring is `termpane::process::{pid_alive, signal}`.
#[cfg(unix)]
fn kill_binary() -> Result<&'static str, OpError> {
    for cand in KILL_BINARIES {
        if std::fs::symlink_metadata(cand).is_ok_and(|m| m.file_type().is_file()) {
            return Ok(cand);
        }
    }
    Err(OpError::new(
        "io",
        "no trusted kill binary (/bin/kill, /usr/bin/kill)",
    ))
}

/// Best-effort liveness of a validated pid. Invalid pids (0, out of range)
/// and missing kill binaries report dead without executing anything.
#[cfg(unix)]
pub(crate) fn pid_alive(pid: u32) -> bool {
    if validate_pid(pid).is_err() {
        return false;
    }
    let Ok(bin) = kill_binary() else {
        return false;
    };
    std::process::Command::new(bin)
        .arg("-0")
        .arg(pid.to_string())
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Non-Unix builds cannot probe liveness; every pid reports dead.
#[cfg(not(unix))]
pub(crate) fn pid_alive(pid: u32) -> bool {
    let _ = pid;
    false
}

/// Deliver `SIGTERM` to a validated pid. An already-dead pid still succeeds;
/// a failure against a live pid is an error (the caller keeps the endpoint).
pub(crate) fn kill_pid(pid: u32) -> Result<(), OpError> {
    signal_pid(pid, "-TERM")
}

/// Deliver `SIGKILL` to a validated pid, with the same dead-pid semantics as
/// [`kill_pid`].
pub(crate) fn kill9_pid(pid: u32) -> Result<(), OpError> {
    signal_pid(pid, "-KILL")
}

#[cfg(unix)]
fn signal_pid(pid: u32, sig: &str) -> Result<(), OpError> {
    validate_pid(pid)?;
    let bin = kill_binary()?;
    let status = std::process::Command::new(bin)
        .arg(sig)
        .arg(pid.to_string())
        .status()
        .map_err(|e| OpError::new("io", format!("{bin} {sig} {pid}: {e}")))?;
    if status.success() {
        return Ok(());
    }
    if pid_alive(pid) {
        return Err(OpError::new(
            "io",
            format!("{bin} {sig} {pid} failed against a live pid"),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn signal_pid(pid: u32, sig: &str) -> Result<(), OpError> {
    let _ = (pid, sig);
    Err(OpError::new("unsupported", "session stop needs Unix"))
}

/// SIGTERM a validated pid, grace, SIGKILL, then verify dead. Any failure
/// returns before the caller removes state, so a failed stop preserves the
/// endpoint (piped sessions) or the endpoint plus the daemon entry (PTY
/// orphans). The single pid-stop implementation for both backends.
#[cfg(unix)]
pub(crate) fn stop_pid(pid: u32) -> Result<(), OpError> {
    kill_pid(pid)?;
    wait_until_dead(pid, std::time::Duration::from_millis(500));
    if pid_alive(pid) {
        kill9_pid(pid)?;
        wait_until_dead(pid, std::time::Duration::from_millis(500));
    }
    if pid_alive(pid) {
        return Err(OpError::new(
            "op-failed",
            format!("pid {pid} survived SIGKILL; endpoint preserved"),
        ));
    }
    Ok(())
}

/// Non-Unix builds cannot stop pids; every pid stop fails closed.
#[cfg(not(unix))]
pub(crate) fn stop_pid(pid: u32) -> Result<(), OpError> {
    let _ = pid;
    Err(OpError::new("unsupported", "session stop needs Unix"))
}

#[cfg(unix)]
fn wait_until_dead(pid: u32, timeout: std::time::Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while pid_alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::super::types::PID_MAX;
    use super::super::validate_pid;
    use super::*;

    #[test]
    fn pid_validation_rejects_group_selection() {
        assert!(validate_pid(0).is_err());
        assert!(validate_pid(PID_MAX + 1).is_err());
        assert!(validate_pid(u32::MAX).is_err());
        validate_pid(1).expect("pid 1");
        validate_pid(PID_MAX).expect("pid max");
        assert!(!pid_alive(0));
        assert!(!pid_alive(u32::MAX));
    }

    #[cfg(unix)]
    #[test]
    fn kill_probe_uses_absolute_binary() {
        let bin = super::kill_binary().expect("kill binary");
        assert!(bin.starts_with('/'), "{bin}");
        assert!(std::fs::symlink_metadata(bin).expect("stat").is_file());
    }
}
