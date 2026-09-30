//! Session prune op: remove exited endpoints.
//!
//! Split out of `session_ops.rs` so each file stays under the repo line
//! gate; behavior is unchanged.

use std::path::Path;

use super::super::{
    DaemonOp, OpError, PtyOwner, SessionBackend, SessionStatus, checked_endpoint_path,
    classify_owner, read_endpoint, runtime_dir, status, transact, validate_session_name,
};
use super::list::session_list;
use super::stop::{pty_owner_pid, remove_orphan};

/// Remove endpoints whose session already exited. Returns the pruned names.
/// Delete paths come from validated listing stems re-checked here, never
/// from untrusted payload fields. PTY orphans kill their lingering child
/// through the validated pid path before the record goes; a failed kill
/// preserves the endpoint.
///
/// # Errors
///
/// Returns [`OpError`] when the session list, an endpoint removal, or an
/// orphan kill fails.
pub fn session_prune() -> Result<Vec<String>, OpError> {
    let dir = runtime_dir()?;
    let mut pruned = Vec::new();
    let mut pty_live = Vec::new();
    let st = status()?;
    for info in session_list()? {
        if info.status != SessionStatus::Exited {
            continue;
        }
        validate_session_name(&info.name)?;
        if info.backend != SessionBackend::Pty {
            std::fs::remove_file(checked_endpoint_path(&dir, &info.name)?)
                .map_err(|e| OpError::new("io", format!("prune {}: {e}", info.name)))?;
            pruned.push(info.name);
            continue;
        }
        prune_one_pty(&dir, &info.name, &st, &mut pty_live, &mut pruned)?;
    }
    pruned.extend(prune_live_pty(&pty_live)?);
    Ok(pruned)
}

/// Prune one exited PTY session: orphans die by the validated pid path
/// here; live-owned names batch into the daemon prune below. A record
/// that changed backend under the listing (a start won the race) is left
/// for the next prune, never deleted blind.
fn prune_one_pty(
    dir: &Path,
    name: &str,
    st: &super::super::DaemonStatus,
    pty_live: &mut Vec<String>,
    pruned: &mut Vec<String>,
) -> Result<(), OpError> {
    let Some(ep) = read_endpoint(dir, name)? else {
        return Ok(());
    };
    if ep.backend != SessionBackend::Pty {
        return Ok(());
    }
    match classify_owner(pty_owner_pid(&ep)?, st)? {
        PtyOwner::Live => {
            pty_live.push(name.to_string());
            Ok(())
        }
        PtyOwner::Orphan => {
            remove_orphan(dir, name, &ep)?;
            pruned.push(name.to_string());
            Ok(())
        }
    }
}

/// Prune the live-owned names through the daemon. A daemon lost on the
/// way re-classifies per name: newly orphaned sessions prune by the
/// validated pid path, a still-live owner errors.
fn prune_live_pty(names: &[String]) -> Result<Vec<String>, OpError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    match prune_via_daemon(names) {
        Ok(pruned) => Ok(pruned),
        Err(e) if e.code == "io" || e.code == "timeout" => prune_pty_fallback(names, e),
        Err(e) => Err(e),
    }
}

/// One daemon prune round trip, unwrapping the pruned names.
pub(super) fn prune_via_daemon(names: &[String]) -> Result<Vec<String>, OpError> {
    let value = transact(&DaemonOp::Prune {
        names: names.to_vec(),
    })?;
    let pruned = value
        .get("pruned")
        .ok_or_else(|| OpError::new("internal", "daemon prune without pruned"))?;
    serde_json::from_value::<Vec<String>>(pruned.clone())
        .map_err(|e| OpError::new("internal", format!("bad daemon prune: {e}")))
}

/// The daemon died mid-prune: re-classify each name once and prune the
/// newly orphaned by the validated pid path.
fn prune_pty_fallback(names: &[String], err: OpError) -> Result<Vec<String>, OpError> {
    let dir = runtime_dir()?;
    let st = status()?;
    let mut pruned = Vec::new();
    for name in names {
        validate_session_name(name)?;
        let Some(ep) = read_endpoint(&dir, name)? else {
            continue;
        };
        if ep.backend != SessionBackend::Pty {
            continue;
        }
        match classify_owner(pty_owner_pid(&ep)?, &st)? {
            PtyOwner::Orphan => {
                remove_orphan(&dir, name, &ep)?;
                pruned.push(name.clone());
            }
            PtyOwner::Live => return Err(err),
        }
    }
    Ok(pruned)
}
