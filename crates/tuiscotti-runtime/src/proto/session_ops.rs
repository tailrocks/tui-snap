use std::path::{Path, PathBuf};

use super::{
    NameReservation, OpError, SESSION_ENDPOINT_VERSION, SessionBackend, SessionEndpoint,
    SessionInfo, SessionStatus, checked_aux_path, checked_endpoint_path, current_uid, kill_pid,
    kill9_pid, now_unix, pid_alive, read_endpoint, runtime_dir, validate_session_name,
    write_endpoint,
};

/// Start a named session: reserve the name, spawn `argv` detached (output to
/// the session log), publish the endpoint atomically. A live same-name
/// session is a `session-exists` error unless `force` stops it first; a
/// corrupt same-name record blocks with its validation error (fail-closed:
/// tamper evidence is never silently replaced). The reservation is held from
/// before the spawn until after the publish, so a losing concurrent starter
/// never spawns and no untracked process escapes.
///
/// # Errors
///
/// Returns [`OpError`] for a bad name, a live same-name session, or spawn
/// failure. A failed publish kills and reaps only the new child.
pub fn session_start(name: &str, argv: &[String], force: bool) -> Result<SessionInfo, OpError> {
    let owned: Vec<std::ffi::OsString> = argv.iter().map(std::ffi::OsString::from).collect();
    session_start_os(name, &owned, force)
}

/// [`session_start`] with native [`OsString`](std::ffi::OsString) argv: the
/// child spawns byte-exact. The endpoint record keeps a lossy UTF-8
/// projection (`argv_display`) because endpoint JSON and the machine-protocol
/// schema are UTF-8; the record is diagnostic, never re-spawned.
///
/// # Errors
///
/// Returns [`OpError`] for a bad name, a live same-name session, or spawn
/// failure. A failed publish kills and reaps only the new child.
pub fn session_start_os(
    name: &str,
    argv: &[std::ffi::OsString],
    force: bool,
) -> Result<SessionInfo, OpError> {
    validate_session_name(name)?;
    if argv.is_empty() {
        return Err(OpError::new("invalid-input", "session start needs argv"));
    }
    let argv_display: Vec<String> = argv
        .iter()
        .map(|a| a.as_os_str().to_string_lossy().into_owned())
        .collect();
    let dir = runtime_dir()?;
    let reservation = NameReservation::acquire(&dir, name)?;
    clear_existing_endpoint(&dir, name, force)?;
    let child = spawn_session_child(&dir, name, argv, &argv_display)?;
    let info = publish_new_endpoint(&dir, name, &argv_display, child)?;
    reservation.release();
    Ok(info)
}

/// Under our reservation: a live entry needs `force` (stop it) or fails;
/// a dead entry's stale file is removed.
fn clear_existing_endpoint(dir: &Path, name: &str, force: bool) -> Result<(), OpError> {
    let Some(ep) = read_endpoint(dir, name)? else {
        return Ok(());
    };
    if pid_alive(ep.pid) {
        if !force {
            return Err(OpError::new(
                "session-exists",
                format!("{name} already running (pid {})", ep.pid),
            ));
        }
        session_stop(name)?;
    } else {
        std::fs::remove_file(checked_endpoint_path(dir, name)?)
            .map_err(|e| OpError::new("io", format!("remove stale {name}: {e}")))?;
    }
    Ok(())
}

/// Spawn the session child with piped output to the session log. The log path
/// is containment-checked and never created through a symlink.
fn spawn_session_child(
    dir: &Path,
    name: &str,
    argv: &[std::ffi::OsString],
    argv_display: &[String],
) -> Result<std::process::Child, OpError> {
    let log_path = checked_aux_path(dir, name, "log")?;
    match std::fs::symlink_metadata(&log_path) {
        Ok(m) if m.file_type().is_symlink() => {
            return Err(OpError::new(
                "invalid-input",
                format!("log {} is a symlink; refusing", log_path.display()),
            ));
        }
        Ok(m) if m.file_type().is_dir() => {
            return Err(OpError::new(
                "io",
                format!("log {} is a directory", log_path.display()),
            ));
        }
        _ => {}
    }
    let log = std::fs::File::create(&log_path)
        .map_err(|e| OpError::new("io", format!("log {}: {e}", log_path.display())))?;
    let mut cmd = std::process::Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stdout(
            log.try_clone()
                .map_err(|e| OpError::new("io", e.to_string()))?,
        )
        .stderr(log);
    // NOTE (wave 1): the session child is no longer detached via setsid(2).
    // The workspace forbids `unsafe`, and `Command::pre_exec` is an unsafe
    // fn with no safe equivalent. The child still outlives the `session
    // start` process in the common case (no SIGHUP unless its terminal
    // closes). Restoring a real detach needs a policy exception or a safe
    // wrapper; recorded as known debt.
    cmd.spawn().map_err(|e| {
        OpError::new(
            "spawn-failed",
            format!("{}: {e}", argv_display.first().cloned().unwrap_or_default()),
        )
    })
}

/// Publish the new endpoint. On publish failure the owned [`Child`](std::process::Child)
/// handle — and only it — is killed and reaped, so no untracked process
/// escapes a failed registration.
fn publish_new_endpoint(
    dir: &Path,
    name: &str,
    argv_display: &[String],
    mut child: std::process::Child,
) -> Result<SessionInfo, OpError> {
    let ep = SessionEndpoint {
        version: SESSION_ENDPOINT_VERSION,
        name: name.to_string(),
        pid: child.id(),
        argv: argv_display.to_vec(),
        backend: SessionBackend::Process,
        started_unix: now_unix(),
        owner: current_uid()?,
    };
    if let Err(e) = write_endpoint(dir, &ep) {
        if child.kill().is_err() {
            // Best effort: the child may have exited already.
        }
        if child.wait().is_err() {
            // Best effort: reap our own child, never another pid.
        }
        return Err(e);
    }
    // Reaper thread: the child runs detached (own session), but until this
    // process exits it is still ours — without a wait it would linger as a
    // zombie and `pid_alive` would misreport it. The thread only reaps.
    std::thread::spawn(move || {
        // Reap only: the detached child's exit status is unobserved by
        // design (liveness comes from `pid_alive`), so a wait failure
        // changes nothing.
        if child.wait().is_err() {
            // Reap failed; the zombie (if any) outlives us.
        }
    });
    Ok(SessionInfo {
        name: ep.name,
        pid: ep.pid,
        argv: ep.argv,
        backend: ep.backend,
        status: SessionStatus::Running,
        started_unix: ep.started_unix,
    })
}

/// Stop a named session: SIGTERM the recorded pid, escalate to SIGKILL past a
/// grace, then remove the endpoint. A stop that cannot kill the pid keeps the
/// endpoint and reports failure; success always reports `Exited`.
///
/// # Errors
///
/// Returns [`OpError`] for a bad name, a missing endpoint, a kill failure
/// (endpoint preserved), or a removal failure.
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
        if pid_alive(ep.pid) {
            stop_live_pid(ep.pid)?;
        }
        std::fs::remove_file(checked_endpoint_path(&dir, name)?)
            .map_err(|e| OpError::new("io", format!("remove {name}: {e}")))?;
        Ok(SessionInfo {
            name: ep.name,
            pid: ep.pid,
            argv: ep.argv,
            backend: ep.backend,
            status: SessionStatus::Exited,
            started_unix: ep.started_unix,
        })
    }
}

/// SIGTERM, grace, SIGKILL, then verify dead. Any failure returns before the
/// caller removes the endpoint, so a failed stop preserves registry state.
#[cfg(unix)]
fn stop_live_pid(pid: u32) -> Result<(), OpError> {
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

#[cfg(unix)]
fn wait_until_dead(pid: u32, timeout: std::time::Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while pid_alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// List all valid endpoints with liveness. Dotted names are sessions like any
/// other (name validation permits dots); foreign files, symlinks, and
/// directories are skipped. A corrupt file under a valid session name is an
/// error, not a silent skip.
///
/// # Errors
///
/// Returns [`OpError`] when the runtime dir cannot be listed or a session
/// record is corrupt.
pub fn session_list() -> Result<Vec<SessionInfo>, OpError> {
    let dir = runtime_dir()?;
    let mut out = Vec::new();
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
    Ok(out)
}

/// Remove endpoints whose pid is dead. Returns the pruned names. Delete paths
/// come from validated listing stems re-checked here, never from untrusted
/// payload fields.
///
/// # Errors
///
/// Returns [`OpError`] when the session list or an endpoint removal fails.
pub fn session_prune() -> Result<Vec<String>, OpError> {
    let dir = runtime_dir()?;
    let mut pruned = Vec::new();
    for info in session_list()? {
        if info.status == SessionStatus::Exited {
            validate_session_name(&info.name)?;
            std::fs::remove_file(checked_endpoint_path(&dir, &info.name)?)
                .map_err(|e| OpError::new("io", format!("prune {}: {e}", info.name)))?;
            pruned.push(info.name);
        }
    }
    Ok(pruned)
}

/// Containment-checked session log path for `session attach`.
///
/// # Errors
///
/// Returns [`OpError`] for a bad name or an unusable runtime dir.
pub fn session_log_path(name: &str) -> Result<PathBuf, OpError> {
    let dir = runtime_dir()?;
    checked_aux_path(&dir, name, "log")
}
