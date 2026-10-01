//! Daemon server: bind, serve, idle out.
//!
//! Split out of `daemon.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

#[cfg(all(unix, feature = "pty"))]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(all(unix, feature = "pty"))]
use std::path::Path;

#[cfg(all(unix, feature = "pty"))]
use super::super::{
    DaemonOp, DaemonResponse, IPC_TIMEOUT, MAX_REQUEST_BYTES, MAX_WAIT_MS, OpError, OpResult,
    checked_daemon_path, parse_request, read_line_capped, render_response, runtime_dir,
    validate_session_name,
};
#[cfg(all(unix, feature = "pty"))]
use super::ensure::remove_unless_symlink;
#[cfg(all(unix, feature = "pty"))]
use super::start::{StartArgs, daemon_start};
#[cfg(all(unix, feature = "pty"))]
use super::status::socket_live;
#[cfg(all(unix, feature = "pty"))]
use super::stop::{daemon_prune, daemon_stop};

/// Cap on concurrent connections: one wedged child (blocking input) may
/// stall its own conn thread, never the daemon; past this the daemon
/// sheds load with a clean error instead of spawning threads forever.
#[cfg(all(unix, feature = "pty"))]
const MAX_CONNS: usize = 32;

// ---------------------------------------------------------------------------
// Server: bind, serve, idle out
// ---------------------------------------------------------------------------

/// Bind the runtime dir's socket and serve until the registry sits empty
/// past the idle limit. Every connection gets one thread (capped);
/// registry ops run through the shared `pty_registry` functions.
#[cfg(all(unix, feature = "pty"))]
pub(super) fn serve_runtime_dir() -> Result<(), OpError> {
    use std::os::unix::fs::PermissionsExt;
    let dir = runtime_dir()?;
    let sock = checked_daemon_path(&dir, "sock")?;
    refuse_bind_symlink(&sock)?;
    let listener = bind_or_conflict(&sock)?;
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| OpError::new("io", format!("chmod 600 {}: {e}", sock.display())))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| OpError::new("io", format!("socket nonblocking: {e}")))?;
    write_pidfile(&dir)?;
    serve(&listener, &dir, daemon_idle_secs());
    // Idle exit with an empty registry: sweep our files best-effort (a
    // racing starter re-probes the socket either way).
    if remove_unless_symlink(&sock).is_err() {
        // Best-effort sweep of our own socket.
    }
    if std::fs::remove_file(checked_daemon_path(&dir, "pid")?).is_err() {
        // Best-effort sweep of our own pidfile.
    }
    Ok(())
}

/// `bind(2)` on a symlink path must never happen: refuse first.
#[cfg(all(unix, feature = "pty"))]
fn refuse_bind_symlink(sock: &Path) -> Result<(), OpError> {
    if std::fs::symlink_metadata(sock).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(OpError::new(
            "invalid-input",
            format!("{} is a symlink; refusing to bind", sock.display()),
        ));
    }
    Ok(())
}

/// Bind, naming the live-owner conflict precisely when the address is
/// taken by something that answers.
#[cfg(all(unix, feature = "pty"))]
fn bind_or_conflict(sock: &Path) -> Result<UnixListener, OpError> {
    match UnixListener::bind(sock) {
        Ok(l) => Ok(l),
        Err(e) => {
            if socket_live(sock).unwrap_or(false) {
                return Err(OpError::new(
                    "session-exists",
                    format!("another daemon already serves {}", sock.display()),
                ));
            }
            Err(OpError::new("io", format!("bind {}: {e}", sock.display())))
        }
    }
}

/// Atomically publish our pid (tmp + rename, mirroring endpoint
/// discipline; the file is a hint, but a torn one helps nobody).
#[cfg(all(unix, feature = "pty"))]
fn write_pidfile(dir: &Path) -> Result<(), OpError> {
    use std::os::unix::fs::PermissionsExt;
    let path = checked_daemon_path(dir, "pid")?;
    let tmp = checked_daemon_path(dir, &format!("pid.tmp.{}", std::process::id()))?;
    std::fs::write(&tmp, format!("{}\n", std::process::id()))
        .map_err(|e| OpError::new("io", format!("write {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| OpError::new("io", format!("publish {}: {e}", path.display())))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| OpError::new("io", format!("chmod 600 {}: {e}", path.display())))?;
    Ok(())
}

/// Idle seconds before an empty daemon exits: 60 s, or
/// `TUISCOTTI_DAEMON_IDLE_SECS` when it parses to 1..=3600 (the test
/// suite uses 1 s so no daemon outlives its test).
#[cfg(all(unix, feature = "pty"))]
fn daemon_idle_secs() -> u64 {
    parse_idle_secs(std::env::var("TUISCOTTI_DAEMON_IDLE_SECS").ok().as_deref())
}

/// Pure half of `daemon_idle_secs`: out-of-range and unparsable values
/// fall back to the default (our own env var; never a fatal error).
#[cfg(any(test, all(unix, feature = "pty")))]
fn parse_idle_secs(val: Option<&str>) -> u64 {
    val.and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| (1..=3600).contains(s))
        .unwrap_or(60)
}

/// Accept loop: one bounded thread per connection, idle exit when the
/// registry sits empty past `idle_secs`. Accept errors sleep and retry
/// (a transient EMFILE must not kill the owner of live sessions).
#[cfg(all(unix, feature = "pty"))]
fn serve(listener: &UnixListener, dir: &Path, idle_secs: u64) {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    let active = Arc::new(AtomicUsize::new(0));
    let mut idle_since = std::time::Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                idle_since = std::time::Instant::now();
                serve_conn(&active, dir, stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if !dir.exists() {
                    // The runtime dir is gone (tempdir-style embedding
                    // cleaned up around us, or the operator deleted it):
                    // our records are gone, so close the orphaned
                    // sessions and exit instead of lingering pointless.
                    super::super::pty_registry::close_all();
                    break;
                }
                if !super::super::pty_registry::is_empty() {
                    idle_since = std::time::Instant::now();
                } else if idle_since.elapsed().as_secs() >= idle_secs {
                    // Re-probe before quitting: a start may have landed in
                    // the backlog after the last check.
                    match listener.accept() {
                        Ok((stream, _)) => {
                            idle_since = std::time::Instant::now();
                            serve_conn(&active, dir, stream);
                        }
                        Err(_) => break,
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}

/// Serve one accepted connection on a counted thread, shedding load past
/// the cap with a clean error instead of queueing unbounded work.
#[cfg(all(unix, feature = "pty"))]
fn serve_conn(
    active: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
    dir: &Path,
    stream: UnixStream,
) {
    use std::sync::atomic::Ordering;
    if active.load(Ordering::SeqCst) >= MAX_CONNS {
        shed_conn(stream);
        return;
    }
    let active = std::sync::Arc::clone(active);
    active.fetch_add(1, Ordering::SeqCst);
    let dir = dir.to_path_buf();
    std::thread::spawn(move || {
        handle_conn(stream, &dir);
        active.fetch_sub(1, Ordering::SeqCst);
    });
}

/// Over the connection cap: answer this one with a clean error instead
/// of queueing unbounded work.
#[cfg(all(unix, feature = "pty"))]
fn shed_conn(mut stream: UnixStream) {
    use std::io::Write as _;
    // Accepted sockets inherit the listener's nonblocking flag on some
    // platforms (macOS): force blocking mode back, or reads race EAGAIN
    // and timeouts never apply.
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    stream.set_read_timeout(Some(IPC_TIMEOUT)).unwrap_or(());
    stream.set_write_timeout(Some(IPC_TIMEOUT)).unwrap_or(());
    let id = read_line_capped(&mut stream, MAX_REQUEST_BYTES)
        .ok()
        .flatten()
        .and_then(|l| parse_request(&l).ok())
        .map_or(0, |r| r.id);
    let res = DaemonResponse::err(
        id,
        OpError::new("bound-exceeded", "daemon is at its connection cap"),
    );
    if let Ok(bytes) = render_response(&res)
        && stream.write_all(&bytes).is_ok()
        && stream.write_all(b"\n").is_err()
    {
        // Best-effort shed reply; the peer is going away.
    }
}

/// Serve one connection: read one bounded request, dispatch, reply once.
#[cfg(all(unix, feature = "pty"))]
fn handle_conn(mut stream: UnixStream, dir: &Path) {
    // Accepted sockets inherit the listener's nonblocking flag on some
    // platforms (macOS): force blocking mode back, or reads race EAGAIN
    // and timeouts never apply.
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    stream.set_read_timeout(Some(IPC_TIMEOUT)).unwrap_or(());
    stream.set_write_timeout(Some(IPC_TIMEOUT)).unwrap_or(());
    let line = match read_line_capped(&mut stream, MAX_REQUEST_BYTES) {
        Ok(Some(l)) => l,
        Ok(None) => return,
        Err(e) => {
            respond(&mut stream, &DaemonResponse::err(0, e));
            return;
        }
    };
    let req = match parse_request(&line) {
        Ok(r) => r,
        Err(e) => {
            respond(&mut stream, &DaemonResponse::err(0, e));
            return;
        }
    };
    match dispatch(dir, &req.op) {
        Ok(value) => respond(&mut stream, &DaemonResponse::ok(req.id, value)),
        Err(e) => respond(&mut stream, &DaemonResponse::err(req.id, e)),
    }
}

/// Best-effort reply: by now the peer may be gone, which is fine.
#[cfg(all(unix, feature = "pty"))]
fn respond(stream: &mut UnixStream, res: &DaemonResponse) {
    use std::io::Write as _;
    if let Ok(bytes) = render_response(res)
        && stream.write_all(&bytes).is_ok()
        && stream.write_all(b"\n").is_ok()
        && stream.flush().is_err()
    {
        // Peer gone mid-reply; nothing left to do.
    }
}

/// Dispatch one validated op. Every targeted name is re-validated here
/// (64-char session rule); wait timeouts are capped so a disconnected
/// client cannot park a conn thread past [`MAX_WAIT_MS`].
#[cfg(all(unix, feature = "pty"))]
fn dispatch(dir: &Path, op: &DaemonOp) -> Result<serde_json::Value, OpError> {
    use super::super::pty_registry;
    if let Some(name) = op.target_name() {
        validate_session_name(name)?;
    }
    match op {
        DaemonOp::Start {
            name,
            argv_b64,
            cwd,
            cols,
            rows,
            force,
        } => daemon_start(
            dir,
            name,
            &StartArgs {
                argv_b64,
                cwd: cwd.as_deref(),
                cols: *cols,
                rows: *rows,
                force: *force,
            },
        ),
        DaemonOp::Input {
            name,
            text,
            chord,
            bytes_b64,
        } => result_value(&pty_registry::stdin(
            name,
            text.clone(),
            chord.clone(),
            bytes_b64.clone(),
        )?),
        DaemonOp::Observe { name } => result_value(&pty_registry::observe(name)?),
        DaemonOp::Snapshot { name } => result_value(&pty_registry::snapshot(name)?),
        DaemonOp::Wait {
            name,
            kind,
            needle,
            quiet_ms,
            timeout_ms,
        } => result_value(&pty_registry::wait(
            name,
            kind,
            needle.as_deref(),
            *quiet_ms,
            (*timeout_ms).min(MAX_WAIT_MS),
        )?),
        DaemonOp::Resize { name, cols, rows } => {
            pty_registry::resize(name, *cols, *rows)?;
            Ok(serde_json::json!({"resized": name}))
        }
        DaemonOp::Signal { name, sig } => {
            pty_registry::signal(name, sig)?;
            Ok(serde_json::json!({"signaled": name}))
        }
        DaemonOp::Stop { name } => daemon_stop(dir, name),
        DaemonOp::List => Ok(serde_json::json!({"sessions": pty_registry::sessions_status()})),
        DaemonOp::Prune { names } => {
            for name in names {
                validate_session_name(name)?;
            }
            daemon_prune(dir, names)
        }
    }
}

/// Serialize an [`OpResult`] into a response value.
#[cfg(all(unix, feature = "pty"))]
pub(super) fn result_value(result: &OpResult) -> Result<serde_json::Value, OpError> {
    serde_json::to_value(result)
        .map_err(|e| OpError::new("internal", format!("encode result: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_secs_default_and_bounds() {
        assert_eq!(parse_idle_secs(None), 60);
        assert_eq!(parse_idle_secs(Some("1")), 1);
        assert_eq!(parse_idle_secs(Some("3600")), 3600);
        for bad in ["", "0", "3601", "99999", "ten", "-5", "  "] {
            assert_eq!(parse_idle_secs(Some(bad)), 60, "{bad:?}");
        }
    }
}
