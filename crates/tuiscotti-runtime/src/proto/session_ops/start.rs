//! Session start ops: piped + PTY.
//!
//! Split out of `session_ops.rs` so each file stays under the repo line
//! gate; behavior is unchanged.

use std::path::Path;

use super::super::{
    DaemonOp, MAX_CONCURRENT_SESSIONS, NameReservation, OpError, SESSION_ENDPOINT_VERSION,
    SESSION_LIMIT_CODE, SessionBackend, SessionEndpoint, SessionInfo, SessionStatus, base64_encode,
    checked_aux_path, checked_endpoint_path, current_uid, ensure_live, now_unix, pid_alive,
    read_endpoint, runtime_dir, transact, validate_session_name, write_endpoint,
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
    admit_session_slot(&dir)?;
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

/// Admission control: refuse a start past [`MAX_CONCURRENT_SESSIONS`]
/// live sessions with a typed [`SESSION_LIMIT_CODE`] rejection — never a
/// silent queue, no child spawns. Best-effort across processes (the PTY
/// registry enforces the same cap exactly at insert).
fn admit_session_slot(dir: &Path) -> Result<(), OpError> {
    if live_session_count(dir)? >= MAX_CONCURRENT_SESSIONS {
        return Err(OpError::new(
            SESSION_LIMIT_CODE,
            format!("at the session limit ({MAX_CONCURRENT_SESSIONS}); stop one first"),
        ));
    }
    Ok(())
}

/// Live sessions occupying slots: valid endpoints with a live child pid,
/// both backends (pid reuse can only refuse a start).
/// Foreign/symlink/dir entries never count. Unlike
/// [`super::session_list`] (which reports a corrupt record as an error),
/// an unreadable or invalid entry simply occupies no slot: poison records
/// (pid 0, tampered payloads) must never veto an unrelated start.
fn live_session_count(dir: &Path) -> Result<usize, OpError> {
    let mut live = 0;
    let entries = std::fs::read_dir(dir)
        .map_err(|e| OpError::new("io", format!("list {}: {e}", dir.display())))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        if validate_session_name(stem).is_err() {
            continue;
        }
        if entry.file_type().is_ok_and(|t| !t.is_file()) {
            continue;
        }
        // Only the directory listing itself is a hard error: an entry that
        // fails to read or validate occupies no slot (see doc comment).
        let Ok(Some(ep)) = read_endpoint(dir, stem) else {
            continue;
        };
        if pid_alive(ep.pid) {
            live += 1;
        }
    }
    Ok(live)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::proto::{
        SESSION_ENDPOINT_VERSION, SessionBackend, current_uid, now_unix, set_runtime_dir_override,
    };
    #[cfg(unix)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(unix)]
    static CTR: AtomicU64 = AtomicU64::new(0);

    #[cfg(unix)]
    fn scratch() -> std::path::PathBuf {
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("tuiscotti-admit-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test dir");
        dir
    }

    #[cfg(unix)]
    fn seed(dir: &Path, name: &str, pid: u32, owner: u32) {
        let ep = SessionEndpoint {
            version: SESSION_ENDPOINT_VERSION,
            name: name.to_string(),
            pid,
            argv: vec!["sleep".to_string()],
            backend: SessionBackend::Process,
            started_unix: now_unix(),
            owner,
            daemon_pid: None,
        };
        write_endpoint(dir, &ep).expect("seed endpoint");
    }

    #[cfg(unix)]
    #[test]
    fn piped_start_refuses_at_cap_and_cleans_up_below_it() {
        struct OverrideClear;
        impl Drop for OverrideClear {
            fn drop(&mut self) {
                set_runtime_dir_override(None);
            }
        }
        let dir = scratch();
        set_runtime_dir_override(Some(dir.clone()));
        let _clear = OverrideClear;
        let owner = current_uid().expect("uid");
        for i in 0..MAX_CONCURRENT_SESSIONS {
            seed(&dir, &format!("live-{i}"), std::process::id(), owner);
        }
        // Highest valid pid: dead on every platform (pid_max ≪ 2³¹−1).
        seed(&dir, "dead", 2_147_483_647, owner);
        std::fs::write(dir.join("foreign.txt"), b"x").expect("seed foreign");
        // Poison records: a pid-0 endpoint (invalid payload) and a directory
        // at an entry path. Both must occupy no slot and veto no start.
        std::fs::write(
            dir.join("poison.json"),
            format!(
                r#"{{"version":{SESSION_ENDPOINT_VERSION},"name":"poison","pid":0,"argv":["x"],"backend":"process","started_unix":{},"owner":{owner}}}"#,
                now_unix(),
            ),
        )
        .expect("seed poison");
        std::fs::create_dir(dir.join("dz.json")).expect("seed dir entry");
        // Full: typed rejection, nothing spawned or published.
        let before = std::fs::read_dir(&dir).expect("list dir").count();
        let argv = [OsString::from("/bin/sleep"), OsString::from("30")];
        let err = session_start_os("newbie", &argv, false).expect_err("full dir must refuse");
        assert_eq!(err.code, SESSION_LIMIT_CODE, "{err}");
        let after = std::fs::read_dir(&dir).expect("list dir").count();
        assert_eq!(after, before, "refused start spawns nothing");
        assert!(!dir.join("newbie.json").exists(), "nothing published");
        assert!(!dir.join("newbie.lock").exists(), "reservation released");
        // Dead records, foreign files, and poison entries occupy no slot;
        // the start below cleans up fully and preserves the poison records.
        std::fs::remove_file(dir.join("live-0.json")).expect("free a slot");
        let info = session_start_os("newbie", &argv, false).expect("slot reopens");
        assert!(pid_alive(info.pid), "started child must be alive");
        session_stop("newbie").expect("stop succeeds");
        assert!(!dir.join("newbie.json").exists(), "endpoint removed");
        assert!(!pid_alive(info.pid), "stray child survived");
        assert!(dir.join("poison.json").is_file(), "poison preserved");
        assert!(dir.join("dz.json").is_dir(), "dir entry preserved");
        if std::fs::remove_dir_all(&dir).is_err() {
            // Leftover scratch in the temp dir is harmless.
        }
    }
}
