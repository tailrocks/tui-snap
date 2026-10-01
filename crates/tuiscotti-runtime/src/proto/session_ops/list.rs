//! Session list op: endpoint scan + PTY liveness merge.
//!
//! Split out of `session_ops.rs` so each file stays under the repo line
//! gate; behavior is unchanged.

use super::super::{
    DaemonOp, OpError, PtyOwner, SessionBackend, SessionEndpoint, SessionInfo, SessionStatus,
    classify_owner, pid_alive, read_endpoint, runtime_dir, status, transact, validate_session_name,
};
use super::stop::pty_owner_pid;

/// List all valid endpoints with liveness. Dotted names are sessions like any
/// other (name validation permits dots); foreign files, symlinks, and
/// directories are skipped. A corrupt file under a valid session name is an
/// error, not a silent skip. Piped sessions resolve liveness by pid;
/// PTY sessions ask the owning daemon (authoritative while it lives), and
/// orphans of a dead owner list as `Exited`.
///
/// # Errors
///
/// Returns [`OpError`] when the runtime dir cannot be listed, a session
/// record is corrupt, or a live owner cannot be reached.
pub fn session_list() -> Result<Vec<SessionInfo>, OpError> {
    let dir = runtime_dir()?;
    let mut out = Vec::new();
    let mut pty = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| OpError::new("io", format!("list {}: {e}", dir.display())))?
        .collect::<Result<_, _>>()
        .map_err(|e| OpError::new("io", format!("list {}: {e}", dir.display())))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        if validate_session_name(stem).is_err() {
            continue;
        }
        let is_file = entry
            .file_type()
            .map_err(|e| OpError::new("io", format!("stat {name}: {e}")))?
            .is_file();
        if !is_file {
            continue;
        }
        if let Some(ep) = read_endpoint(&dir, stem)? {
            if ep.backend == SessionBackend::Pty {
                pty.push(ep);
            } else {
                out.push(SessionInfo {
                    name: ep.name,
                    pid: ep.pid,
                    argv: ep.argv,
                    backend: ep.backend,
                    status: if pid_alive(ep.pid) {
                        SessionStatus::Running
                    } else {
                        SessionStatus::Exited
                    },
                    started_unix: ep.started_unix,
                });
            }
        }
    }
    out.extend(pty_infos(&pty)?);
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Resolve liveness for the PTY endpoints: one daemon probe, one registry
/// fetch when any endpoint is live-owned. Pure-piped lists never touch
/// the daemon. A daemon lost between probe and fetch re-classifies once;
/// newly orphaned endpoints list `Exited`, a still-live owner errors.
fn pty_infos(endpoints: &[SessionEndpoint]) -> Result<Vec<SessionInfo>, OpError> {
    if endpoints.is_empty() {
        return Ok(Vec::new());
    }
    let owners = pty_owners(endpoints, &status()?)?;
    if !owners.iter().any(|(_, o)| *o == PtyOwner::Live) {
        return Ok(infos_with_registry(&owners, &[]));
    }
    match daemon_registry() {
        Ok(registry) => Ok(infos_with_registry(&owners, &registry)),
        Err(e) if e.code == "io" || e.code == "timeout" => {
            let owners = pty_owners(endpoints, &status()?)?;
            if owners.iter().any(|(_, o)| *o == PtyOwner::Live) {
                Err(e)
            } else {
                Ok(infos_with_registry(&owners, &[]))
            }
        }
        Err(e) => Err(e),
    }
}

/// Classify each endpoint's recorded owner against one daemon probe.
fn pty_owners<'a>(
    endpoints: &'a [SessionEndpoint],
    st: &super::super::DaemonStatus,
) -> Result<Vec<(&'a SessionEndpoint, PtyOwner)>, OpError> {
    let mut owners = Vec::with_capacity(endpoints.len());
    for ep in endpoints {
        owners.push((ep, classify_owner(pty_owner_pid(ep)?, st)?));
    }
    Ok(owners)
}

/// Build the reports: live-owned sessions consult the registry snapshot
/// (an entry missing from the registry already exited); orphans are
/// `Exited` by definition.
fn infos_with_registry(
    owners: &[(&SessionEndpoint, PtyOwner)],
    registry: &[DaemonSession],
) -> Vec<SessionInfo> {
    owners
        .iter()
        .map(|(ep, owner)| {
            let running =
                *owner == PtyOwner::Live && registry.iter().any(|s| s.name == ep.name && s.running);
            SessionInfo {
                name: ep.name.clone(),
                pid: ep.pid,
                argv: ep.argv.clone(),
                backend: ep.backend.clone(),
                status: if running {
                    SessionStatus::Running
                } else {
                    SessionStatus::Exited
                },
                started_unix: ep.started_unix,
            }
        })
        .collect()
}

/// One daemon registry snapshot for the list merge.
#[derive(Debug, Clone, serde::Deserialize)]
struct DaemonSession {
    /// Registry key (= session name).
    name: String,
    /// True while the child has not been reaped.
    running: bool,
}

/// Fetch the daemon's registry snapshot.
fn daemon_registry() -> Result<Vec<DaemonSession>, OpError> {
    let value = transact(&DaemonOp::List)?;
    let sessions = value
        .get("sessions")
        .ok_or_else(|| OpError::new("internal", "daemon list without sessions"))?;
    serde_json::from_value::<Vec<DaemonSession>>(sessions.clone())
        .map_err(|e| OpError::new("internal", format!("bad daemon list: {e}")))
}
