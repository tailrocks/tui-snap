//! Server transactions: daemon-owned PTY start path.
//!
//! Split out of `daemon.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

#[cfg(all(unix, feature = "pty"))]
use std::collections::HashMap;
#[cfg(all(unix, feature = "pty"))]
use std::path::{Path, PathBuf};

#[cfg(all(unix, feature = "pty"))]
use super::super::{
    NameReservation, OpError, OpResult, SESSION_ENDPOINT_VERSION, SessionBackend, SessionEndpoint,
    SessionInfo, SessionStatus, base64_decode, checked_endpoint_path, current_uid, now_unix,
    pid_alive, read_endpoint, stop_pid, write_endpoint,
};
#[cfg(all(unix, feature = "pty"))]
use super::serve::result_value;
#[cfg(all(unix, feature = "pty"))]
use super::stop::remove_endpoint;

// ---------------------------------------------------------------------------
// Server transactions: the daemon owns the Pty endpoint lifecycle
// ---------------------------------------------------------------------------

/// Start a retained PTY session: reserve the name, clear any stale entry,
/// spawn through the registry, publish the endpoint. Mirrors the piped
/// start's ordering (reservation held from before the spawn until after
/// the publish), so a losing concurrent starter never spawns and a failed
/// publish kills and reaps only the new child.
/// The `Start` op's members beyond the session name (one struct keeps
/// the transaction entry under the argument-count lint).
#[cfg(all(unix, feature = "pty"))]
pub(super) struct StartArgs<'a> {
    pub(super) argv_b64: &'a [String],
    pub(super) cwd: Option<&'a str>,
    pub(super) cols: Option<u16>,
    pub(super) rows: Option<u16>,
    pub(super) force: bool,
}

#[cfg(all(unix, feature = "pty"))]
pub(super) fn daemon_start(
    dir: &Path,
    name: &str,
    args: &StartArgs<'_>,
) -> Result<serde_json::Value, OpError> {
    let argv_os = decode_argv(args.argv_b64)?;
    let cwd = check_start_cwd(args.cwd)?;
    let reservation = NameReservation::acquire(dir, name)?;
    if let Err(e) = clear_existing_for_start(dir, name, args.force) {
        drop(reservation);
        return Err(e);
    }
    let Some(child_pid) = spawn_start_child(name, &argv_os, args.cols, args.rows, cwd)? else {
        super::super::pty_registry::drop_session(name);
        drop(reservation);
        return Err(OpError::new(
            "unsupported",
            "spawned session reports no pid on this platform",
        ));
    };
    let argv_display: Vec<String> = argv_os
        .iter()
        .map(|a| a.as_os_str().to_string_lossy().into_owned())
        .collect();
    let ep = SessionEndpoint {
        version: SESSION_ENDPOINT_VERSION,
        name: name.to_string(),
        pid: child_pid,
        argv: argv_display.clone(),
        backend: SessionBackend::Pty,
        started_unix: now_unix(),
        owner: current_uid()?,
        daemon_pid: Some(std::process::id()),
    };
    if let Err(e) = write_endpoint(dir, &ep) {
        // Publish failed: kill and reap only the child we just spawned.
        super::super::pty_registry::drop_session(name);
        drop(reservation);
        return Err(e);
    }
    reservation.release();
    result_value(&OpResult::Session {
        session: SessionInfo {
            name: ep.name,
            pid: ep.pid,
            argv: argv_display,
            backend: SessionBackend::Pty,
            status: SessionStatus::Running,
            started_unix: ep.started_unix,
        },
    })
}

/// Decode the base64 argv into byte-exact spawn arguments.
#[cfg(all(unix, feature = "pty"))]
fn decode_argv(argv_b64: &[String]) -> Result<Vec<std::ffi::OsString>, OpError> {
    use std::os::unix::ffi::OsStringExt;
    if argv_b64.is_empty() {
        return Err(OpError::new("invalid-input", "session start needs argv"));
    }
    let mut argv_os = Vec::with_capacity(argv_b64.len());
    for arg in argv_b64 {
        let bytes = base64_decode(arg)
            .map_err(|e| OpError::new("invalid-input", format!("bad argv entry: {e}")))?;
        argv_os.push(std::ffi::OsString::from_vec(bytes));
    }
    Ok(argv_os)
}

/// A start `cwd` must be absolute when present (relative would resolve
/// against the daemon's directory — surprising and unstable).
#[cfg(all(unix, feature = "pty"))]
fn check_start_cwd(cwd: Option<&str>) -> Result<Option<PathBuf>, OpError> {
    match cwd {
        None => Ok(None),
        Some(c) => {
            let path = PathBuf::from(c);
            if path.is_absolute() {
                Ok(Some(path))
            } else {
                Err(OpError::new(
                    "invalid-input",
                    "session start cwd must be absolute",
                ))
            }
        }
    }
}

/// Spawn under the reservation through the shared registry (empty child
/// env: PTY children inherit the daemon's environment — documented
/// inherited-env behavior, matching termpane's overrides-only PTY path).
#[cfg(all(unix, feature = "pty"))]
fn spawn_start_child(
    name: &str,
    argv_os: &[std::ffi::OsString],
    cols: Option<u16>,
    rows: Option<u16>,
    cwd: Option<PathBuf>,
) -> Result<Option<u32>, OpError> {
    match super::super::pty_registry::spawn_os(
        argv_os,
        Some(name.to_string()),
        cols,
        rows,
        cwd,
        &HashMap::new(),
    )? {
        OpResult::Spawned { pid, .. } => Ok(pid),
        _ => Err(OpError::new("internal", "spawn returned the wrong result")),
    }
}

/// Under our reservation: a live entry needs `force` (stop it) or fails;
/// a stale entry's child (if any lingers) is killed through the validated
/// pid path and its file removed. Piped entries follow the F08-F1 rules
/// verbatim; PTY entries consult the registry, never `pid_alive`, for
/// liveness (no pid-reuse window on the owned path).
#[cfg(all(unix, feature = "pty"))]
fn clear_existing_for_start(dir: &Path, name: &str, force: bool) -> Result<(), OpError> {
    let Some(ep) = read_endpoint(dir, name)? else {
        return Ok(());
    };
    match ep.backend {
        SessionBackend::Process => clear_process_for_start(dir, name, &ep, force),
        SessionBackend::Pty => clear_pty_for_start(dir, name, &ep, force),
    }
}

/// F08-F1 start-over-piped-entry rules, verbatim.
#[cfg(all(unix, feature = "pty"))]
fn clear_process_for_start(
    dir: &Path,
    name: &str,
    ep: &SessionEndpoint,
    force: bool,
) -> Result<(), OpError> {
    if pid_alive(ep.pid) {
        if !force {
            return Err(OpError::new(
                "session-exists",
                format!("{name} already running (pid {})", ep.pid),
            ));
        }
        return super::super::session_stop(name).map(|_| ());
    }
    std::fs::remove_file(checked_endpoint_path(dir, name)?)
        .map_err(|e| OpError::new("io", format!("remove stale {name}: {e}")))?;
    Ok(())
}

/// Start-over-PTY-entry rules: registry liveness, never pid probes.
#[cfg(all(unix, feature = "pty"))]
fn clear_pty_for_start(
    dir: &Path,
    name: &str,
    ep: &SessionEndpoint,
    force: bool,
) -> Result<(), OpError> {
    guard_single_owner(ep)?;
    let live = super::super::pty_registry::sessions_status()
        .iter()
        .any(|s| s.name == name && s.running);
    if live {
        if !force {
            return Err(OpError::new(
                "session-exists",
                format!("{name} already running (pid {})", ep.pid),
            ));
        }
        super::super::pty_registry::stop(name)?;
        return remove_endpoint(dir, name);
    }
    // Stale record: a lingering child (orphan of a dead daemon, or our own
    // lost entry) is killed through the validated pid path before its last
    // pid record is removed — dropping the record first would leak it.
    if pid_alive(ep.pid) {
        stop_pid(ep.pid)?;
    }
    remove_endpoint(dir, name)
}

/// Refuse to touch a session whose recorded owner is another LIVE daemon:
/// two serving daemons is split brain (the lock failed), and adopting or
/// killing the other's session would corrupt it. Fail closed instead.
#[cfg(all(unix, feature = "pty"))]
pub(super) fn guard_single_owner(ep: &SessionEndpoint) -> Result<(), OpError> {
    if let Some(owner) = ep.daemon_pid
        && owner != std::process::id()
        && pid_alive(owner)
    {
        return Err(OpError::new(
            "op-failed",
            format!(
                "{} is owned by another live daemon (pid {owner}); refusing",
                ep.name
            ),
        ));
    }
    Ok(())
}
