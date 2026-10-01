//! Session stop ops: piped pid stops + PTY owner/orphan stops.
//!
//! Split out of `session_ops.rs` so each file stays under the repo line
//! gate; behavior is unchanged.

use std::path::Path;

#[cfg(unix)]
use super::super::{
    DaemonOp, PtyOwner, SessionBackend, SessionStatus, classify_owner, read_endpoint, status,
    transact,
};
use super::super::{
    OpError, OpResult, SessionEndpoint, SessionInfo, checked_endpoint_path, pid_alive, runtime_dir,
    stop_pid, validate_session_name,
};

/// Stop a named session and remove its endpoint. Piped sessions stop by
/// pid (SIGTERM, grace, SIGKILL, verify); PTY sessions stop through the
/// owning daemon (owned handle: TERM, grace, kill+reap), or — when the
/// owner is dead — through the validated pid path as an orphan. A stop
/// that cannot kill keeps the endpoint and reports failure; success
/// always reports `Exited`.
///
/// # Errors
///
/// Returns [`OpError`] for a bad name, a missing endpoint, a kill failure
/// (endpoint preserved), a removal failure, or a live-but-silent owner.
pub fn session_stop(name: &str) -> Result<SessionInfo, OpError> {
    validate_session_name(name)?;
    #[cfg(not(unix))]
    {
        runtime_dir()?;
        return Err(OpError::new("unsupported", "session stop needs Unix"));
    }
    #[cfg(unix)]
    {
        let dir = runtime_dir()?;
        let ep = read_endpoint(&dir, name)?.ok_or_else(|| OpError::new("not-found", name))?;
        match ep.backend {
            SessionBackend::Process => {
                if pid_alive(ep.pid) {
                    stop_pid(ep.pid)?;
                }
                remove_endpoint_file(&dir, name)?;
                Ok(exited_info(&ep))
            }
            SessionBackend::Pty => session_stop_pty(&dir, name, &ep),
        }
    }
}

/// Stop a PTY session: through the live owner, or as an orphan when the
/// owner is dead. A daemon lost mid-stop re-classifies once so an
/// orphaned stop still kills the child instead of dropping its record.
#[cfg(unix)]
fn session_stop_pty(dir: &Path, name: &str, ep: &SessionEndpoint) -> Result<SessionInfo, OpError> {
    let recorded = pty_owner_pid(ep)?;
    match classify_owner(recorded, &status()?)? {
        PtyOwner::Live => match transact(&DaemonOp::Stop {
            name: name.to_string(),
        }) {
            Ok(value) => session_result(value),
            Err(e) if e.code == "io" || e.code == "timeout" => {
                match classify_owner(recorded, &status()?)? {
                    PtyOwner::Orphan => stop_orphan(dir, name, ep),
                    PtyOwner::Live => Err(e),
                }
            }
            Err(e) => Err(e),
        },
        PtyOwner::Orphan => stop_orphan(dir, name, ep),
    }
}

/// Stop an orphaned PTY session: kill the lingering child (when any)
/// through the validated pid path, then remove the record. A failed kill
/// preserves the endpoint, like every other stop.
#[cfg(unix)]
fn stop_orphan(dir: &Path, name: &str, ep: &SessionEndpoint) -> Result<SessionInfo, OpError> {
    remove_orphan(dir, name, ep)?;
    Ok(exited_info(ep))
}

/// Kill an orphan's lingering child (when any) through the validated pid
/// path, then remove its record. Shared by stop and prune (and portable:
/// off-Unix every pid reads dead, so this only removes the record).
pub(super) fn remove_orphan(dir: &Path, name: &str, ep: &SessionEndpoint) -> Result<(), OpError> {
    if pid_alive(ep.pid) {
        stop_pid(ep.pid)?;
    }
    remove_endpoint_file(dir, name)
}

/// A validated `Pty` endpoint always names its owner; anything else is an
/// internal inconsistency, never a signal target.
pub(super) fn pty_owner_pid(ep: &SessionEndpoint) -> Result<u32, OpError> {
    ep.daemon_pid
        .ok_or_else(|| OpError::new("internal", "validated Pty endpoint lacks its owner"))
}

/// The `Exited` report for a removed endpoint.
#[cfg(unix)]
fn exited_info(ep: &SessionEndpoint) -> SessionInfo {
    SessionInfo {
        name: ep.name.clone(),
        pid: ep.pid,
        argv: ep.argv.clone(),
        backend: ep.backend.clone(),
        status: SessionStatus::Exited,
        started_unix: ep.started_unix,
    }
}

/// Remove one endpoint file by its validated listing stem.
fn remove_endpoint_file(dir: &Path, name: &str) -> Result<(), OpError> {
    std::fs::remove_file(checked_endpoint_path(dir, name)?)
        .map_err(|e| OpError::new("io", format!("remove {name}: {e}")))
}

/// Unwrap a daemon `Session` result value.
pub(super) fn session_result(value: serde_json::Value) -> Result<SessionInfo, OpError> {
    match serde_json::from_value::<OpResult>(value) {
        Ok(OpResult::Session { session }) => Ok(session),
        Ok(_) => Err(OpError::new("internal", "daemon returned the wrong result")),
        Err(e) => Err(OpError::new("internal", format!("bad daemon result: {e}"))),
    }
}
