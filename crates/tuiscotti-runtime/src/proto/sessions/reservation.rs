//! Exclusive same-name start reservations.
//!
//! Split out of `sessions.rs` so each file stays under the repo line gate.
//!
//! MUTUAL EXCLUSION IS THE OS FILE LOCK (`flock` / `LockFileEx` via `fs2`),
//! held on an open FD for the guard's lifetime — never path existence.
//! The lock file is a persistent tombstone holding `"<pid> <unix-secs>\n"`;
//! it is NEVER unlinked: unlink-then-recreate lets two processes each
//! exclusively lock a different inode (old-unlinked + new) and reintroduces
//! double-hold. Check (read the tombstone) and act (rewrite our claim) both
//! run UNDER the flock, which closes the stale-takeover TOCTOU where two
//! racing starters each judged a stale seed, then one deleted the other's
//! fresh lock and both proceeded to spawn.
//!
//! Crash recovery falls out of the OS: death drops the flock, and the
//! tombstone keeps the dead owner's pid, which the next acquirer judges
//! stale. Clean release rewrites the tombstone to the free marker
//! (`"0 1\n"`, pid 0 never probes alive) so the next acquire takes over
//! immediately instead of waiting out the 30 s takeover window.
//!
//! Residuals: racing a pre-flock starter (path existence, no flock) during
//! an upgrade can still double-hold. A new-version daemon loser exits
//! silently on bind conflict and the winner serves; an old-version loser
//! still records `daemon.err` (upgrade-mid-race only, self-heals on retry).
//! A piped session double-start publishes twice (last wins) and may strand
//! the first child until it exits on its own. A parsed live owner while we
//! hold the flock means pid reuse after a crash (fail closed, busy) or a
//! mixed-version holder (polite busy).

use std::fs::File;
use std::io::{Read as _, Seek as _, Write as _};
use std::path::Path;

use fs2::FileExt as _;

#[cfg(all(unix, feature = "pty"))]
use super::super::checked_daemon_path;
use super::super::{OpError, checked_aux_path, now_unix, pid_alive, read_endpoint};
use super::types::RESERVATION_TAKEOVER_SECS;

/// Tombstone content meaning "free": pid 0 never probes alive, stamp 1 is
/// ancient. Written on clean release/drop so the next acquire takes over
/// immediately.
const FREE_MARKER: &str = "0 1\n";

/// Exclusive same-name start reservation: a flock-held open FD on the
/// `{name}.lock` tombstone. The guard frees the tombstone on drop, so every
/// early return releases the name; call [`NameReservation::release`] on the
/// success path. A tombstone whose owner is dead is crash residue and is
/// taken over.
#[derive(Debug)]
pub(crate) struct NameReservation {
    file: File,
    released: bool,
}

impl NameReservation {
    pub(crate) fn acquire(dir: &Path, name: &str) -> Result<Self, OpError> {
        let lock_path = checked_aux_path(dir, name, "lock")?;
        Self::acquire_at(dir, name, &lock_path)
    }

    /// Single-flight daemon autostart lock (`daemon.lock`). Bypasses
    /// [`validate_session_name`](super::super::validate_session_name) (which reserves `daemon`
    /// for exactly the daemon's files); flock exclusivity, stale takeover,
    /// and the freeing drop guard are identical to session reservations.
    #[cfg(all(unix, feature = "pty"))]
    pub(crate) fn acquire_daemon(dir: &Path) -> Result<Self, OpError> {
        let lock_path = checked_daemon_path(dir, "lock")?;
        Self::acquire_at(dir, "daemon", &lock_path)
    }

    /// Open-or-create, flock non-blocking, judge the tombstone under the
    /// lock, rewrite our claim in place. Single attempt: contention means
    /// busy, and retry-with-backoff belongs to the caller (`ensure_live`
    /// polls; single-shot starts report `session-exists`).
    fn acquire_at(dir: &Path, name: &str, lock_path: &Path) -> Result<Self, OpError> {
        // Refuse symlink lock paths (fail closed like the socket/pidfile):
        // opening would follow the link onto a file we must never claim.
        if std::fs::symlink_metadata(lock_path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(OpError::new(
                "invalid-input",
                format!("{} is a symlink; refusing to lock", lock_path.display()),
            ));
        }
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|e| OpError::new("io", format!("reserve {name}: {e}")))?;
        match file.try_lock_exclusive() {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(lock_busy_error(dir, name)?);
            }
            Err(e) => {
                return Err(OpError::new("io", format!("flock {name}: {e}")));
            }
        }
        let mut content = String::new();
        if file.read_to_string(&mut content).is_err() {
            unlock_best_effort(&file);
            return Err(OpError::new(
                "io",
                format!("read {}: tombstone unreadable", lock_path.display()),
            ));
        }
        if !lock_content_is_stale(&content) {
            unlock_best_effort(&file);
            return Err(lock_busy_error(dir, name)?);
        }
        if claim_in_place(&mut file).is_err() {
            unlock_best_effort(&file);
            return Err(OpError::new("io", format!("claim {}", lock_path.display())));
        }
        Ok(Self {
            file,
            released: false,
        })
    }

    /// Publish succeeded: mark free, unlock, close. The tombstone persists
    /// by design (see module docs); the next acquire takes it immediately.
    pub(crate) fn release(mut self) {
        self.released = true;
        free_in_place(&mut self.file);
        unlock_best_effort(&self.file);
    }
}

impl Drop for NameReservation {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        // Early return (not a crash — crashes never run this): free the
        // tombstone so the next acquire takes over without the 30 s wait.
        // No pid-tag check: the flock we hold IS the ownership proof, and
        // re-reading-then-deciding would reintroduce the check-then-act
        // race this protocol exists to close.
        free_in_place(&mut self.file);
        unlock_best_effort(&self.file);
    }
}

/// Best-effort unlock (close releases the lock regardless; every caller is
/// already on a release path).
fn unlock_best_effort(file: &File) {
    if file.unlock().is_err() {
        // The FD close below still releases the flock.
    }
}

/// Rewrite our claim over the tombstone in place (same inode — never
/// unlink; see module docs). No fsync: if we crash, the tombstone either
/// keeps our pid (dead owner: stale) or an older stale marker (stale).
fn claim_in_place(file: &mut File) -> std::io::Result<()> {
    file.rewind()?;
    file.set_len(0)?;
    file.write_all(format!("{} {}\n", std::process::id(), now_unix()).as_bytes())
}

/// Best-effort free-marker rewrite (release/drop paths never fail). Even a
/// failed rewrite degrades promptly: an emptied tombstone reads stale.
fn free_in_place(file: &mut File) {
    if file.rewind().is_err() {
        return;
    }
    if file.set_len(0).is_err() {
        return;
    }
    if file.write_all(FREE_MARKER.as_bytes()).is_err() {
        // Best-effort release mark; the worst case is an emptied
        // tombstone, which reads stale (see below).
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

/// Judge tombstone content read UNDER the flock. Unparsable (empty from
/// a fresh create, torn by a crashed writer) is always takeable: the only
/// process that could still be writing holds the flock, and that is us. A
/// parsed live owner means pid reuse after a crash or a mixed-version
/// holder: fail closed (busy). The 30 s dead-owner window is unchanged from
/// the path protocol.
fn lock_content_is_stale(content: &str) -> bool {
    let mut parts = content.split_whitespace();
    let (Some(pid), Some(created)) = (parts.next(), parts.next()) else {
        return true;
    };
    let (Ok(pid), Ok(created)) = (pid.parse::<u32>(), created.parse::<u64>()) else {
        return true;
    };
    let old = created.saturating_add(RESERVATION_TAKEOVER_SECS) < now_unix();
    old && !pid_alive(pid)
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
        // The tombstone persists by design (never unlink under flock);
        // drop freed it, so reacquire takes over immediately.
        assert!(dir.join("n.lock").exists(), "tombstone persists");
        let again = NameReservation::acquire(&dir, "n").expect("reacquire");
        again.release();
        assert!(dir.join("n.lock").exists(), "release keeps tombstone");
        let third = NameReservation::acquire(&dir, "n").expect("takeover after release");
        third.release();
        cleanup(&dir);
    }

    #[test]
    fn reservation_racing_takeover_single_holder() {
        use std::sync::{
            Arc, Barrier,
            atomic::{AtomicUsize, Ordering},
        };
        // Barrier-aligned racers on a seeded-stale lock: exactly one holder
        // at any instant, every round. Fails reliably on the pre-flock
        // check-then-act protocol (both judge the stale seed, one deletes
        // the other's fresh lock, both proceed).
        let dir = test_dir("racestale");
        let holders = Arc::new(AtomicUsize::new(0));
        let max = Arc::new(AtomicUsize::new(0));
        for _ in 0..50 {
            std::fs::write(dir.join("r.lock"), format!("{} 1\n", u32::MAX)).expect("seed");
            let barrier = Arc::new(Barrier::new(2));
            let mut threads = Vec::new();
            for _ in 0..2 {
                let (dir, barrier, holders, max) = (
                    dir.clone(),
                    Arc::clone(&barrier),
                    Arc::clone(&holders),
                    Arc::clone(&max),
                );
                threads.push(std::thread::spawn(move || {
                    barrier.wait();
                    match NameReservation::acquire(&dir, "r") {
                        Ok(guard) => {
                            let n = holders.fetch_add(1, Ordering::SeqCst) + 1;
                            max.fetch_max(n, Ordering::SeqCst);
                            std::thread::sleep(std::time::Duration::from_millis(1));
                            holders.fetch_sub(1, Ordering::SeqCst);
                            guard.release();
                        }
                        Err(e) => assert_eq!(e.code, "session-exists"),
                    }
                }));
            }
            for t in threads {
                t.join().expect("racer");
            }
            assert_eq!(holders.load(Ordering::SeqCst), 0, "balanced holds");
        }
        assert_eq!(
            max.load(Ordering::SeqCst),
            1,
            "exactly one holder at a time"
        );
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

    #[cfg(unix)]
    #[test]
    fn reservation_refuses_symlink_lock() {
        let dir = test_dir("symlinklock");
        let target = dir.join("victim.txt");
        std::fs::write(&target, b"precious").expect("seed");
        std::os::unix::fs::symlink(&target, dir.join("s.lock")).expect("symlink");
        let e = NameReservation::acquire(&dir, "s").expect_err("symlink lock taken");
        assert_eq!(e.code, "invalid-input");
        assert_eq!(
            std::fs::read(&target).expect("victim intact"),
            b"precious",
            "claim must never write through the link"
        );
        cleanup(&dir);
    }
}
