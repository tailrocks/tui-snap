//! Exclusive same-name start reservations.
//!
//! Split out of `sessions.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use std::path::{Path, PathBuf};

#[cfg(all(unix, feature = "pty"))]
use super::super::checked_daemon_path;
use super::super::{OpError, checked_aux_path, now_unix, pid_alive, read_endpoint};
use super::types::{RESERVATION_CORRUPT_STALE_SECS, RESERVATION_TAKEOVER_SECS};

/// Exclusive same-name start reservation (`{name}.lock`, `create_new`). The
/// guard removes our lock on drop, so every early return releases the name;
/// call [`NameReservation::release`] on the success path. A lock whose owner
/// is dead (or is corrupt and old) is crash residue and is taken over.
#[derive(Debug)]
pub(crate) struct NameReservation {
    lock_path: PathBuf,
    released: bool,
}

impl NameReservation {
    pub(crate) fn acquire(dir: &Path, name: &str) -> Result<Self, OpError> {
        let lock_path = checked_aux_path(dir, name, "lock")?;
        Self::acquire_at(dir, name, lock_path)
    }

    /// Single-flight daemon autostart lock (`daemon.lock`). Bypasses
    /// [`validate_session_name`](super::super::validate_session_name) (which reserves `daemon`
    /// for exactly the daemon's files); exclusivity, stale takeover, and
    /// the pid-tagged drop guard are identical to session reservations.
    #[cfg(all(unix, feature = "pty"))]
    pub(crate) fn acquire_daemon(dir: &Path) -> Result<Self, OpError> {
        let lock_path = checked_daemon_path(dir, "lock")?;
        Self::acquire_at(dir, "daemon", lock_path)
    }

    fn acquire_at(dir: &Path, name: &str, lock_path: PathBuf) -> Result<Self, OpError> {
        for _ in 0..4 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut f) => {
                    if let Err(e) = std::io::Write::write_all(
                        &mut f,
                        format!("{} {}\n", std::process::id(), now_unix()).as_bytes(),
                    ) {
                        drop(f);
                        if std::fs::remove_file(&lock_path).is_err() {
                            // Best-effort cleanup of our own unwritten lock.
                        }
                        return Err(OpError::new(
                            "io",
                            format!("write {}: {e}", lock_path.display()),
                        ));
                    }
                    return Ok(Self {
                        lock_path,
                        released: false,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&lock_path)? {
                        if std::fs::remove_file(&lock_path).is_err() {
                            // Someone else took over first; re-probe below.
                        }
                        continue;
                    }
                    return Err(lock_busy_error(dir, name)?);
                }
                Err(e) => {
                    return Err(OpError::new("io", format!("reserve {name}: {e}")));
                }
            }
        }
        Err(OpError::new(
            "session-exists",
            format!("{name} is starting elsewhere"),
        ))
    }

    /// Publish succeeded: remove our lock now (drop is the backstop).
    pub(crate) fn release(mut self) {
        self.released = true;
        if std::fs::remove_file(&self.lock_path).is_err() {
            // Best-effort release of our own lock.
        }
    }
}

impl Drop for NameReservation {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        // Only remove a lock we still own (same-process pid tag): never
        // delete a lock that a successor already recreated.
        let tag = format!("{} ", std::process::id());
        let owned = std::fs::read_to_string(&self.lock_path).is_ok_and(|c| c.starts_with(&tag));
        if owned && std::fs::remove_file(&self.lock_path).is_err() {
            // Best-effort release of our own lock.
        }
    }
}

/// A live same-name starter reports the running pid; otherwise the name is
/// briefly busy. Both are `session-exists`: the caller never spawns. The
/// daemon lock (`daemon`) has no endpoint by construction (`daemon` is a
/// reserved name), so it skips the probe — probing would fail name
/// validation and mask the `session-exists` the starter retries on.
fn lock_busy_error(dir: &Path, name: &str) -> Result<OpError, OpError> {
    if name != "daemon"
        && let Some(ep) = read_endpoint(dir, name)?
        && pid_alive(ep.pid)
    {
        return Ok(OpError::new(
            "session-exists",
            format!("{name} already running (pid {})", ep.pid),
        ));
    }
    Ok(OpError::new(
        "session-exists",
        format!("{name} is starting elsewhere"),
    ))
}

/// Crash-residue probe: a parsed lock goes stale with a dead owner; an
/// unparsable one only by age (its writer may still be mid-write).
fn lock_is_stale(lock_path: &Path) -> Result<bool, OpError> {
    let content = match std::fs::read_to_string(lock_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => {
            return Err(OpError::new(
                "io",
                format!("read {}: {e}", lock_path.display()),
            ));
        }
    };
    let mut parts = content.split_whitespace();
    if let (Some(pid), Some(created)) = (parts.next(), parts.next())
        && let (Ok(pid), Ok(created)) = (pid.parse::<u32>(), created.parse::<u64>())
    {
        let old = created.saturating_add(RESERVATION_TAKEOVER_SECS) < now_unix();
        return Ok(old && !pid_alive(pid));
    }
    let age = std::fs::symlink_metadata(lock_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok());
    Ok(age.is_some_and(|a| a.as_secs() > RESERVATION_CORRUPT_STALE_SECS))
}

#[cfg(test)]
mod tests {
    use super::super::write_endpoint;
    use super::super::{
        SESSION_ENDPOINT_VERSION, cleanup, now_unix, sample_endpoint, sample_owner,
        sample_pty_endpoint, test_dir,
    };
    use super::*;

    #[test]
    fn reservation_is_exclusive() {
        let dir = test_dir("reserve");
        let first = NameReservation::acquire(&dir, "n").expect("first");
        let e = NameReservation::acquire(&dir, "n").expect_err("double acquire");
        assert_eq!(e.code, "session-exists");
        drop(first);
        assert!(!dir.join("n.lock").exists(), "drop releases");
        let again = NameReservation::acquire(&dir, "n").expect("reacquire");
        again.release();
        assert!(!dir.join("n.lock").exists(), "release removes");
        cleanup(&dir);
    }

    #[test]
    fn endpoint_daemon_pid_required_iff_pty() {
        let dir = test_dir("daemonpid");
        let me = std::process::id();
        // Pty with a live daemon pid round-trips.
        let ep = sample_pty_endpoint("p", me, sample_owner(), Some(me));
        write_endpoint(&dir, &ep).expect("write");
        let back = read_endpoint(&dir, "p").expect("read").expect("some");
        assert_eq!(back.daemon_pid, Some(me));
        // Pty without one is corrupt.
        let ep = sample_pty_endpoint("p", me, sample_owner(), None);
        write_endpoint(&dir, &ep).expect("write");
        assert_eq!(
            read_endpoint(&dir, "p")
                .expect_err("pidless Pty accepted")
                .code,
            "invalid-input"
        );
        // Pty with pid 0 is corrupt (never a signal target).
        let ep = sample_pty_endpoint("p", me, sample_owner(), Some(0));
        write_endpoint(&dir, &ep).expect("write");
        assert_eq!(
            read_endpoint(&dir, "p")
                .expect_err("pid-0 daemon accepted")
                .code,
            "invalid-input"
        );
        // Process carrying one is corrupt.
        let mut ep = sample_endpoint("q", me, sample_owner());
        ep.daemon_pid = Some(me);
        write_endpoint(&dir, &ep).expect("write");
        assert_eq!(
            read_endpoint(&dir, "q")
                .expect_err("daemon pid on Process accepted")
                .code,
            "invalid-input"
        );
        // Pre-F2 records (no daemon_pid field) still parse as Process.
        std::fs::write(
            dir.join("old.json"),
            format!(
                r#"{{"version":{SESSION_ENDPOINT_VERSION},"name":"old","pid":{me},"argv":["sleep"],"backend":"process","started_unix":{},"owner":{}}}"#,
                now_unix(),
                sample_owner()
            ),
        )
        .expect("seed");
        let back = read_endpoint(&dir, "old").expect("read").expect("some");
        assert_eq!(back.daemon_pid, None);
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn reservation_takes_over_stale_locks() {
        let dir = test_dir("stale");
        // Dead owner (invalid pid never probes alive) + ancient stamp.
        std::fs::write(dir.join("s.lock"), format!("{} 1\n", u32::MAX)).expect("seed");
        let taken = NameReservation::acquire(&dir, "s").expect("takeover");
        taken.release();
        // Live owner (ourselves) + fresh stamp stays busy.
        std::fs::write(
            dir.join("b.lock"),
            format!("{} {}\n", std::process::id(), now_unix()),
        )
        .expect("seed");
        let e = NameReservation::acquire(&dir, "b").expect_err("busy lock taken");
        assert_eq!(e.code, "session-exists");
        cleanup(&dir);
    }
}
