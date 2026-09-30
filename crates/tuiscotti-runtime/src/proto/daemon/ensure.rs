//! Daemon autostart: exactly one daemon per runtime dir, on demand.
//!
//! Split out of `daemon.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

#[cfg(all(unix, feature = "pty"))]
use std::path::Path;

use super::super::OpError;
#[cfg(all(unix, feature = "pty"))]
use super::super::{NameReservation, checked_daemon_path, runtime_dir};
#[cfg(all(unix, feature = "pty"))]
use super::status::{DaemonStatus, socket_live, status};

/// Autostart waits this long for the lock and, separately, for readiness.
#[cfg(all(unix, feature = "pty"))]
const ENSURE_ATTEMPTS: u32 = 50;
/// Poll cadence inside the autostart waits.
#[cfg(all(unix, feature = "pty"))]
const ENSURE_POLL: std::time::Duration = std::time::Duration::from_millis(100);

// ---------------------------------------------------------------------------
// Autostart: exactly one daemon per runtime dir, on demand
// ---------------------------------------------------------------------------

/// Ensure a daemon serves this runtime dir, starting one under the
/// `daemon.lock` single-flight when down. Only `session start --pty` calls
/// this: every other op must NOT resurrect a daemon (a fresh daemon owns
/// nothing, so autostart there would only mask orphans).
#[cfg(all(unix, feature = "pty"))]
pub(crate) fn ensure_live() -> Result<(), OpError> {
    let dir = runtime_dir()?;
    for _ in 0..ENSURE_ATTEMPTS {
        match status()? {
            DaemonStatus::Live { .. } => return Ok(()),
            DaemonStatus::Unreachable { pid } => {
                return Err(OpError::new(
                    "op-failed",
                    format!(
                        "daemon (pid {pid}) is alive but not serving; refusing to start a second"
                    ),
                ));
            }
            DaemonStatus::Down => match NameReservation::acquire_daemon(&dir) {
                Ok(res) => return start_under_lock(&dir, res),
                Err(e) if e.code == "session-exists" => {
                    // A racing starter holds the lock; its daemon should
                    // appear on the next probe.
                    std::thread::sleep(ENSURE_POLL);
                }
                Err(e) => return Err(e),
            },
        }
    }
    Err(OpError::new("io", "timed out waiting for the daemon lock"))
}

/// Without a server there is nothing to start: fail closed with the
/// reason instead of spawning a daemon that cannot serve.
#[cfg(not(all(unix, feature = "pty")))]
pub(crate) fn ensure_live() -> Result<(), OpError> {
    Err(OpError::new(
        "unsupported",
        "PTY sessions need Unix with the `pty` feature",
    ))
}

/// Holding the single-flight lock: re-probe (a racing starter may have
/// won), sweep stale files, spawn, and wait for readiness. The lock
/// releases on every path; the daemon never holds it (socket liveness is
/// the ownership signal once it serves).
#[cfg(all(unix, feature = "pty"))]
fn start_under_lock(dir: &Path, res: NameReservation) -> Result<(), OpError> {
    match status()? {
        DaemonStatus::Live { .. } => {
            res.release();
            return Ok(());
        }
        DaemonStatus::Unreachable { pid } => {
            drop(res);
            return Err(OpError::new(
                "op-failed",
                format!("daemon (pid {pid}) is alive but not serving; refusing to start a second"),
            ));
        }
        DaemonStatus::Down => {}
    }
    if let Err(e) = cleanup_stale_daemon_files(dir) {
        drop(res);
        return Err(e);
    }
    if let Err(e) = spawn_daemon() {
        drop(res);
        return Err(e);
    }
    let sock = checked_daemon_path(dir, "sock")?;
    for _ in 0..ENSURE_ATTEMPTS {
        if socket_live(&sock)? {
            res.release();
            return Ok(());
        }
        std::thread::sleep(ENSURE_POLL);
    }
    drop(res);
    Err(OpError::new(
        "io",
        format!("daemon did not become ready{}", boot_hint(dir)),
    ))
}

/// Remove stale socket/pidfile/err under the single-flight lock. Symlinks
/// are never removed or followed (hard error); missing files are fine.
#[cfg(all(unix, feature = "pty"))]
fn cleanup_stale_daemon_files(dir: &Path) -> Result<(), OpError> {
    for suffix in ["sock", "pid", "err"] {
        remove_unless_symlink(&checked_daemon_path(dir, suffix)?)?;
    }
    Ok(())
}

/// Remove `path` unless it is a symlink (refuse) or missing (fine).
#[cfg(all(unix, feature = "pty"))]
pub(super) fn remove_unless_symlink(path: &Path) -> Result<(), OpError> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(OpError::new("io", format!("stat {}: {e}", path.display()))),
        Ok(meta) if meta.file_type().is_symlink() => Err(OpError::new(
            "invalid-input",
            format!("{} is a symlink; refusing to remove", path.display()),
        )),
        Ok(_) => match std::fs::remove_file(path) {
            Ok(()) | Err(_) => Ok(()),
        },
    }
}

/// Spawn ourselves as the daemon: same exe, hidden subcommand, inherited
/// environment (runtime dir + idle override), detached stdio.
#[cfg(all(unix, feature = "pty"))]
fn spawn_daemon() -> Result<(), OpError> {
    let exe = std::env::current_exe().map_err(|e| OpError::new("io", format!("own exe: {e}")))?;
    std::process::Command::new(exe)
        .arg("__daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| OpError::new("io", format!("spawn daemon: {e}")))?;
    Ok(())
}

/// Best-effort `daemon.err` tail for autostart failure messages.
#[cfg(all(unix, feature = "pty"))]
fn boot_hint(dir: &Path) -> String {
    let path = checked_daemon_path(dir, "err");
    let text = path
        .as_ref()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let tail: String = text.trim().chars().take(300).collect();
    if tail.is_empty() {
        String::new()
    } else {
        format!(" (daemon: {tail})")
    }
}
