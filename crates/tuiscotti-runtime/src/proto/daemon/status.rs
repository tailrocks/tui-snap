//! Daemon status probe + owner classification.
//!
//! Split out of `daemon.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::Path;

use super::super::OpError;
#[cfg(unix)]
use super::super::{checked_daemon_path, current_uid, pid_alive, runtime_dir, validate_pid};

// ---------------------------------------------------------------------------
// Status: is a daemon serving this runtime dir?
// ---------------------------------------------------------------------------

/// What the socket + pidfile probe found. `Live` means a connect
/// succeeded (truth); the pid is the pidfile hint, if it parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DaemonStatus {
    /// A daemon answered the socket.
    #[cfg(unix)]
    Live {
        /// Pidfile hint, when present and sane.
        pid: Option<u32>,
    },
    /// No daemon (nothing answered, recorded owner dead or absent).
    Down,
    /// A recorded owner is alive but the socket is dead: never guess.
    #[cfg(unix)]
    Unreachable {
        /// The alive-but-silent pid.
        pid: u32,
    },
}

/// A `Pty` endpoint's owner verdict: the live daemon owns it, or the
/// recorded owner is dead and the endpoint is an orphan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PtyOwner {
    /// The serving daemon owns this session: talk to it.
    Live,
    /// The recorded owner is dead: orphan rules apply (list `Exited`,
    /// validated-path kill on prune/force/stop, no transport).
    Orphan,
}

#[cfg(unix)]
pub(crate) fn status() -> Result<DaemonStatus, OpError> {
    let dir = runtime_dir()?;
    let sock = checked_daemon_path(&dir, "sock")?;
    if socket_live(&sock)? {
        return Ok(DaemonStatus::Live {
            pid: read_daemon_pid(&dir)?,
        });
    }
    match read_daemon_pid(&dir)? {
        Some(pid) if pid_alive(pid) => Ok(DaemonStatus::Unreachable { pid }),
        Some(_) | None => Ok(DaemonStatus::Down),
    }
}

/// Non-Unix builds never serve: every `Pty` endpoint is foreign, and
/// foreign endpoints list as orphans rather than failing the list.
#[cfg(not(unix))]
pub(crate) fn status() -> Result<DaemonStatus, OpError> {
    Ok(DaemonStatus::Down)
}

/// True when a daemon answers `sock`. A symlink is never followed or
/// connected to (hard error); a missing path is simply not live.
#[cfg(unix)]
pub(super) fn socket_live(sock: &Path) -> Result<bool, OpError> {
    match std::fs::symlink_metadata(sock) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(OpError::new("io", format!("stat {}: {e}", sock.display())));
        }
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(OpError::new(
                    "invalid-input",
                    format!("{} is a symlink; refusing to connect", sock.display()),
                ));
            }
            check_socket_owner(sock, &meta)?;
        }
    }
    Ok(UnixStream::connect(sock).is_ok())
}

/// The socket must be ours: a foreign-owned socket in our runtime dir is
/// tamper evidence, never a server.
#[cfg(unix)]
pub(super) fn check_socket_owner(sock: &Path, meta: &std::fs::Metadata) -> Result<(), OpError> {
    use std::os::unix::fs::MetadataExt;
    let me = current_uid()?;
    if meta.uid() != me {
        return Err(OpError::new(
            "owner-mismatch",
            format!("{} belongs to uid {}, not {me}", sock.display(), meta.uid()),
        ));
    }
    Ok(())
}

/// Read the pidfile hint: missing or corrupt reads as `None` (down +
/// cleanup), while symlinks, foreign owners, and oversize files are hard
/// errors. A zero pid is corrupt, never a signal target.
#[cfg(unix)]
pub(super) fn read_daemon_pid(dir: &Path) -> Result<Option<u32>, OpError> {
    let path = checked_daemon_path(dir, "pid")?;
    let meta = match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(OpError::new("io", format!("stat {}: {e}", path.display())));
        }
        Ok(m) => m,
    };
    if meta.file_type().is_symlink() {
        return Err(OpError::new(
            "invalid-input",
            format!("{} is a symlink; refusing to read", path.display()),
        ));
    }
    if !meta.file_type().is_file() {
        return Err(OpError::new(
            "invalid-input",
            format!("{} is not a regular file", path.display()),
        ));
    }
    check_socket_owner(&path, &meta)?;
    if meta.len() > 1024 {
        return Err(OpError::new(
            "bound-exceeded",
            format!("{} exceeds 1024 bytes", path.display()),
        ));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| OpError::new("io", format!("read {}: {e}", path.display())))?;
    let pid: u32 = match text.trim().parse() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    if validate_pid(pid).is_err() {
        return Ok(None);
    }
    Ok(Some(pid))
}

/// Classify one `Pty` endpoint's recorded owner against a fresh
/// [`status`]: owned by the serving daemon, or orphaned by a dead owner.
/// A recorded pid that is alive but not the serving daemon (restarted
/// daemon, recycled pid, split brain) fails closed — never guessed.
pub(crate) fn classify_owner(recorded: u32, status: &DaemonStatus) -> Result<PtyOwner, OpError> {
    #[cfg(not(unix))]
    let _ = recorded;
    match status {
        #[cfg(unix)]
        DaemonStatus::Live { pid } if *pid == Some(recorded) => Ok(PtyOwner::Live),
        #[cfg(unix)]
        DaemonStatus::Live { .. } | DaemonStatus::Down => {
            if pid_alive(recorded) {
                Err(OpError::new(
                    "op-failed",
                    format!(
                        "owner daemon (pid {recorded}) is alive but not serving; refusing to guess"
                    ),
                ))
            } else {
                Ok(PtyOwner::Orphan)
            }
        }
        #[cfg(not(unix))]
        DaemonStatus::Down => Ok(PtyOwner::Orphan),
        #[cfg(unix)]
        DaemonStatus::Unreachable { pid } => Err(OpError::new(
            "op-failed",
            format!("owner daemon (pid {pid}) is alive but not serving; refusing to guess"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn owner_live_match_serves() {
        let me = std::process::id();
        let st = DaemonStatus::Live { pid: Some(me) };
        assert_eq!(classify_owner(me, &st).expect("own daemon"), PtyOwner::Live);
    }

    #[cfg(unix)]
    #[test]
    fn owner_dead_recorded_is_orphan() {
        // `u32::MAX` is never a live pid: every status agrees orphan.
        let dead = u32::MAX;
        assert!(!pid_alive(dead));
        for st in [
            DaemonStatus::Live { pid: None },
            DaemonStatus::Live { pid: Some(1) },
            DaemonStatus::Down,
        ] {
            assert_eq!(classify_owner(dead, &st).expect("orphan"), PtyOwner::Orphan);
        }
    }

    #[cfg(unix)]
    #[test]
    fn owner_alive_foreign_never_guessed() {
        // Our own pid is alive but is not the serving daemon in any of
        // these shapes: every one fails closed with the endpoint kept.
        let me = std::process::id();
        assert!(pid_alive(me));
        for st in [
            DaemonStatus::Live { pid: None },
            DaemonStatus::Live { pid: Some(me ^ 1) },
            DaemonStatus::Down,
            DaemonStatus::Unreachable { pid: me },
        ] {
            let e = classify_owner(me, &st).expect_err("must not guess");
            assert_eq!(e.code, "op-failed");
        }
        let e = classify_owner(u32::MAX, &DaemonStatus::Unreachable { pid: 1 })
            .expect_err("unreachable guesses nothing");
        assert_eq!(e.code, "op-failed");
    }
}
