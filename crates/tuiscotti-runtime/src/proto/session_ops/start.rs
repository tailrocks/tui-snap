//! Session start ops: piped + PTY.
//!
//! Split out of `session_ops.rs` so each file stays under the repo line
//! gate; behavior is unchanged.

use std::path::Path;

use super::super::{
    DaemonOp, NameReservation, OpError, SESSION_ENDPOINT_VERSION, SessionBackend, SessionEndpoint,
    SessionInfo, SessionStatus, base64_encode, checked_aux_path, checked_endpoint_path,
    current_uid, ensure_live, now_unix, pid_alive, read_endpoint, runtime_dir, transact,
    validate_session_name, write_endpoint,
};
use super::prune::prune_via_daemon;
use super::stop::{session_result, session_stop};

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

/// Start a named PTY session: autostart the daemon when down, then run
/// the whole start transaction there (reserve, clear, spawn, publish).
/// The child spawns byte-exact; the endpoint keeps a lossy UTF-8
/// projection (diagnostic, never re-spawned). `cols`/`rows` pair or are
/// both absent (default 80x24); the child inherits the caller's working
/// directory and the daemon's environment (documented F08-F2 behavior).
///
/// # Errors
///
/// Returns [`OpError`] for a bad name, unpaired geometry, a live
/// same-name session, spawn failure, or a dead owner mid-start.
pub fn session_start_pty(
    name: &str,
    argv: &[std::ffi::OsString],
    force: bool,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<SessionInfo, OpError> {
    validate_session_name(name)?;
    if argv.is_empty() {
        return Err(OpError::new("invalid-input", "session start needs argv"));
    }
    if cols.is_some() != rows.is_some() {
        return Err(OpError::new(
            "invalid-input",
            "cols and rows must be given together",
        ));
    }
    ensure_live()?;
    let op = start_op(name, argv, cols, rows, force);
    match transact(&op) {
        Ok(value) => session_result(value),
        Err(e) if e.code == "io" || e.code == "timeout" => {
            // The daemon may have idled out between ensure and transact:
            // one resurrection + retry, then the error stands as returned
            // (a `session-exists` here stays an error — fail closed).
            ensure_live()?;
            session_result(transact(&op)?)
        }
        Err(e) => Err(e),
    }
}

/// Build the daemon `Start` op: byte-exact argv as base64, the caller's
/// working directory when it is representable.
fn start_op(
    name: &str,
    argv: &[std::ffi::OsString],
    cols: Option<u16>,
    rows: Option<u16>,
    force: bool,
) -> DaemonOp {
    DaemonOp::Start {
        name: name.to_string(),
        argv_b64: argv_b64(argv),
        cwd: start_cwd(),
        cols,
        rows,
        force,
    }
}

/// Base64 argv for the wire (byte-exact spawn through a UTF-8 protocol).
fn argv_b64(argv: &[std::ffi::OsString]) -> Vec<String> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        argv.iter()
            .map(|a| base64_encode(a.as_os_str().as_bytes()))
            .collect()
    }
    #[cfg(not(unix))]
    {
        argv.iter()
            .map(|a| base64_encode(a.to_string_lossy().as_bytes()))
            .collect()
    }
}

/// The caller's working directory for the child (omitted when unknown
/// or unrepresentable — then the daemon's directory applies).
fn start_cwd() -> Option<String> {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
}

/// Under our reservation: a live entry needs `force` (stop it) or fails;
/// a dead entry's stale file is removed. (Conservative for PTY entries:
/// a live recorded pid always blocks, so pid reuse can only refuse a
/// start, never steal one; force/stale paths delegate to the backend.)
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
        if ep.backend == SessionBackend::Pty && prune_via_daemon(&[name.to_string()]).is_err() {
            // Best-effort drop of the daemon's lingering exited entry so a
            // replaced PTY session cannot pin the daemon past idle exit.
            // Sound to ignore: entries exist only in a reachable daemon,
            // and an unreachable daemon holds nothing to drop.
        }
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
        daemon_pid: None,
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
