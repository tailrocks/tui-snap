use std::path::{Path, PathBuf};

use super::{
    DaemonOp, NameReservation, ObservationView, OpError, OpResult, PtyOwner,
    SESSION_ENDPOINT_VERSION, SessionBackend, SessionEndpoint, SessionInfo, SessionStatus,
    base64_encode, checked_aux_path, checked_endpoint_path, classify_owner, current_uid,
    ensure_live, now_unix, pid_alive, read_endpoint, runtime_dir, status, stop_pid, transact,
    validate_session_name, write_endpoint,
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
fn remove_orphan(dir: &Path, name: &str, ep: &SessionEndpoint) -> Result<(), OpError> {
    if pid_alive(ep.pid) {
        stop_pid(ep.pid)?;
    }
    remove_endpoint_file(dir, name)
}

/// A validated `Pty` endpoint always names its owner; anything else is an
/// internal inconsistency, never a signal target.
fn pty_owner_pid(ep: &SessionEndpoint) -> Result<u32, OpError> {
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
fn session_result(value: serde_json::Value) -> Result<SessionInfo, OpError> {
    match serde_json::from_value::<OpResult>(value) {
        Ok(OpResult::Session { session }) => Ok(session),
        Ok(_) => Err(OpError::new("internal", "daemon returned the wrong result")),
        Err(e) => Err(OpError::new("internal", format!("bad daemon result: {e}"))),
    }
}

/// Send input to a PTY session: exactly one of `text`, `chord`, or
/// `bytes_b64`. Piped sessions have no input transport (`unsupported`);
/// orphaned sessions have no owner left (`not-found`). Never autostarts.
///
/// # Errors
///
/// Returns [`OpError`] for a bad name, a missing endpoint, a wrong
/// backend, a silent owner, or a refused/failed delivery.
pub fn session_input(
    name: &str,
    text: Option<String>,
    chord: Option<String>,
    bytes_b64: Option<String>,
) -> Result<(), OpError> {
    validate_session_name(name)?;
    let set = [text.is_some(), chord.is_some(), bytes_b64.is_some()]
        .into_iter()
        .filter(|b| *b)
        .count();
    if set != 1 {
        return Err(OpError::new(
            "invalid-input",
            "stdin needs exactly one of text|chord|bytes_b64",
        ));
    }
    pty_transact(
        name,
        &DaemonOp::Input {
            name: name.to_string(),
            text,
            chord,
            bytes_b64,
        },
    )?;
    Ok(())
}

/// [`session_input`] for raw bytes (the attach loop's stdin forwarding).
///
/// # Errors
///
/// Returns [`OpError`] for empty bytes or any [`session_input`] failure.
pub fn session_input_bytes(name: &str, bytes: &[u8]) -> Result<(), OpError> {
    if bytes.is_empty() {
        return Err(OpError::new("invalid-input", "bytes must not be empty"));
    }
    session_input(name, None, None, Some(base64_encode(bytes)))
}

/// Read a PTY session's current observation projection. Piped sessions
/// have no screen (`unsupported`); orphaned sessions have no owner left
/// (`not-found`). Never autostarts.
///
/// # Errors
///
/// Returns [`OpError`] for a bad name, a missing endpoint, a wrong
/// backend, a silent owner, or an unreadable session.
pub fn session_observe(name: &str) -> Result<ObservationView, OpError> {
    validate_session_name(name)?;
    let value = pty_transact(
        name,
        &DaemonOp::Observe {
            name: name.to_string(),
        },
    )?;
    match serde_json::from_value::<OpResult>(value) {
        Ok(OpResult::Observation { observation }) => Ok(observation),
        Ok(_) => Err(OpError::new("internal", "daemon returned the wrong result")),
        Err(e) => Err(OpError::new("internal", format!("bad daemon result: {e}"))),
    }
}

/// Run one op against a PTY session's live owner: validate the endpoint
/// read first (tamper fails here, before any daemon contact), refuse
/// piped backends (no transport exists), refuse orphans (no owner left).
/// A daemon lost mid-op re-classifies once; a newly orphaned session
/// reports `not-found`, a still-live owner reports the transport error.
fn pty_transact(name: &str, op: &DaemonOp) -> Result<serde_json::Value, OpError> {
    let dir = runtime_dir()?;
    let ep = read_endpoint(&dir, name)?.ok_or_else(|| OpError::new("not-found", name))?;
    if ep.backend != SessionBackend::Pty {
        return Err(OpError::new(
            "unsupported",
            format!("{name} is a piped session; this op needs --pty"),
        ));
    }
    let recorded = pty_owner_pid(&ep)?;
    match classify_owner(recorded, &status()?)? {
        PtyOwner::Live => match transact(op) {
            Ok(value) => Ok(value),
            Err(e) if e.code == "io" || e.code == "timeout" => {
                match classify_owner(recorded, &status()?)? {
                    PtyOwner::Orphan => Err(orphaned(name)),
                    PtyOwner::Live => Err(e),
                }
            }
            Err(e) => Err(e),
        },
        PtyOwner::Orphan => Err(orphaned(name)),
    }
}

/// The owner-is-down verdict for transport ops (input/observe): the
/// child may linger, but without its owner there is no transport.
fn orphaned(name: &str) -> OpError {
    OpError::new(
        "not-found",
        format!("{name}: owner daemon is down; session orphaned"),
    )
}

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
    st: &super::DaemonStatus,
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
    st: &super::DaemonStatus,
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
fn prune_via_daemon(names: &[String]) -> Result<Vec<String>, OpError> {
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

/// Containment-checked session log path for `session attach`.
///
/// # Errors
///
/// Returns [`OpError`] for a bad name or an unusable runtime dir.
pub fn session_log_path(name: &str) -> Result<PathBuf, OpError> {
    let dir = runtime_dir()?;
    checked_aux_path(&dir, name, "log")
}
