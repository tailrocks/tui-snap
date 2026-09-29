use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;
use serde::{Deserialize, Serialize};

/// Start a named session: spawn `argv` detached (output to the session log),
/// publish the endpoint. A live same-name session is a `session-exists` error
/// unless `force` stops it first.
pub fn session_start(name: &str, argv: &[String], force: bool) -> Result<SessionInfo, OpError> {
    let owned: Vec<std::ffi::OsString> = argv.iter().map(|a| std::ffi::OsString::from(a)).collect();
    session_start_os(name, &owned, force)
}

/// [`session_start`] with native [`OsString`](std::ffi::OsString) argv: the
/// child spawns byte-exact. The endpoint record keeps a lossy UTF-8
/// projection (`argv_display`) because endpoint JSON and the machine-protocol
/// schema are UTF-8; the record is diagnostic, never re-spawned.
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
    if let Some(ep) = read_endpoint(&dir, name)? {
        if pid_alive(ep.pid) {
            if !force {
                return Err(OpError::new(
                    "session-exists",
                    format!("{name} already running (pid {})", ep.pid),
                ));
            }
            session_stop(name)?;
        } else {
            std::fs::remove_file(endpoint_path(&dir, name))
                .map_err(|e| OpError::new("io", format!("remove stale {name}: {e}")))?;
        }
    }
    let log_path = dir.join(format!("{name}.log"));
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
    let mut child = cmd.spawn().map_err(|e| {
        OpError::new(
            "spawn-failed",
            format!("{}: {e}", argv_display.first().cloned().unwrap_or_default()),
        )
    })?;
    let ep = SessionEndpoint {
        version: SESSION_ENDPOINT_VERSION,
        name: name.to_string(),
        pid: child.id(),
        argv: argv_display,
        backend: SessionBackend::Process,
        started_unix: now_unix(),
        owner: Some(current_uid()),
    };
    write_endpoint(&dir, &ep)?;
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

/// Stop a named session: SIGTERM the recorded pid (best effort when already
/// dead), remove the endpoint. Returns the last known info.
pub fn session_stop(name: &str) -> Result<SessionInfo, OpError> {
    validate_session_name(name)?;
    let dir = runtime_dir()?;
    let ep = read_endpoint(&dir, name)?.ok_or_else(|| OpError::new("not-found", name))?;
    let alive = pid_alive(ep.pid);
    if alive {
        kill_pid(ep.pid)?;
        // Brief grace, then SIGKILL via the same helper path.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while pid_alive(ep.pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        #[cfg(all(unix, feature = "pty"))]
        if pid_alive(ep.pid) {
            // No libc: best-effort `kill -KILL` (the workspace forbids
            // `unsafe`). Endpoint removal below proceeds regardless: a
            // surviving pid simply reappears as live on the next list.
            let killed = std::process::Command::new("kill")
                .arg("-KILL")
                .arg(ep.pid.to_string())
                .status()
                .is_ok_and(|s| s.success());
            if !killed {
                // SIGKILL delivery failed; the endpoint still goes away.
            }
        }
    }
    std::fs::remove_file(endpoint_path(&dir, name))
        .map_err(|e| OpError::new("io", format!("remove {name}: {e}")))?;
    Ok(SessionInfo {
        name: ep.name,
        pid: ep.pid,
        argv: ep.argv,
        backend: ep.backend,
        status: if alive {
            SessionStatus::Running
        } else {
            SessionStatus::Exited
        },
        started_unix: ep.started_unix,
    })
}

/// List all valid endpoints with liveness. Corrupt files are skipped only via
/// [`session_prune`]'s report; here a corrupt file is an error.
pub fn session_list() -> Result<Vec<SessionInfo>, OpError> {
    let dir = runtime_dir()?;
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| OpError::new("io", format!("list {}: {e}", dir.display())))?
        .collect::<Result<_, _>>()
        .map_err(|e| OpError::new("io", format!("list {}: {e}", dir.display())))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        if stem.contains('.') || entry.file_type().map(|t| !t.is_file()).unwrap_or(true) {
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

/// Remove endpoints whose pid is dead. Returns the pruned names.
pub fn session_prune() -> Result<Vec<String>, OpError> {
    let dir = runtime_dir()?;
    let mut pruned = Vec::new();
    for info in session_list()? {
        if info.status == SessionStatus::Exited {
            std::fs::remove_file(endpoint_path(&dir, &info.name))
                .map_err(|e| OpError::new("io", format!("prune {}: {e}", info.name)))?;
            pruned.push(info.name);
        }
    }
    Ok(pruned)
}
