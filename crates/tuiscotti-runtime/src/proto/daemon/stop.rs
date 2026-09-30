//! Server transactions: daemon-owned PTY stop/prune paths.
//!
//! Split out of `daemon.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

#[cfg(all(unix, feature = "pty"))]
use std::path::Path;

#[cfg(all(unix, feature = "pty"))]
use super::super::{
    OpError, OpResult, SessionBackend, SessionInfo, SessionStatus, checked_endpoint_path,
    pid_alive, read_endpoint, stop_pid,
};
#[cfg(all(unix, feature = "pty"))]
use super::serve::result_value;
#[cfg(all(unix, feature = "pty"))]
use super::start::guard_single_owner;

/// Stop a session and remove its endpoint. Piped names delegate to the
/// piped stop; PTY names stop through the owned handle (TERM, grace,
/// kill+reap) with the entry AND the endpoint preserved on any failure.
/// An endpoint without a registry entry is already-exited: kill a
/// lingering child through the validated pid path, remove the record.
#[cfg(all(unix, feature = "pty"))]
pub(super) fn daemon_stop(dir: &Path, name: &str) -> Result<serde_json::Value, OpError> {
    let Some(ep) = read_endpoint(dir, name)? else {
        // No record: drop any leaked entry, then report truthfully.
        super::super::pty_registry::drop_session(name);
        return Err(OpError::new("not-found", name));
    };
    if ep.backend == SessionBackend::Process {
        return result_value(&OpResult::Session {
            session: super::super::session_stop(name)?,
        });
    }
    guard_single_owner(&ep)?;
    match super::super::pty_registry::stop(name) {
        Ok(()) => {}
        Err(e) if e.code == "not-found" => {
            if pid_alive(ep.pid) {
                stop_pid(ep.pid)?;
            }
        }
        Err(e) => return Err(e),
    }
    remove_endpoint(dir, name)?;
    result_value(&OpResult::Session {
        session: SessionInfo {
            name: ep.name,
            pid: ep.pid,
            argv: ep.argv,
            backend: SessionBackend::Pty,
            status: SessionStatus::Exited,
            started_unix: ep.started_unix,
        },
    })
}

/// Prune the named PTY sessions: drop each registry entry that is already
/// exited (a running entry is NEVER pruned — a start racing the prune
/// must not lose its session), then remove its endpoint record. Piped
/// names are ignored here (the CLI prunes those locally); tampered
/// records abort the prune instead of deleting around them.
#[cfg(all(unix, feature = "pty"))]
pub(super) fn daemon_prune(dir: &Path, names: &[String]) -> Result<serde_json::Value, OpError> {
    let mut pruned = Vec::new();
    for name in names {
        if !super::super::pty_registry::drop_exited(name) {
            continue;
        }
        match read_endpoint(dir, name)? {
            None => {}
            Some(ep) if ep.backend != SessionBackend::Pty => {}
            Some(_) => {
                remove_endpoint(dir, name)?;
                pruned.push(name.clone());
            }
        }
    }
    Ok(serde_json::json!({"pruned": pruned}))
}

/// Remove one endpoint file by its validated listing stem — never by an
/// untrusted payload field (the stem was validated before this call).
#[cfg(all(unix, feature = "pty"))]
pub(super) fn remove_endpoint(dir: &Path, name: &str) -> Result<(), OpError> {
    std::fs::remove_file(checked_endpoint_path(dir, name)?)
        .map_err(|e| OpError::new("io", format!("remove {name}: {e}")))?;
    Ok(())
}
