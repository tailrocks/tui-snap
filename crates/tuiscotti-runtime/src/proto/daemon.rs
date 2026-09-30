// ---------------------------------------------------------------------------
// Retained-session daemon (F08-F2): one owner per runtime dir
// ---------------------------------------------------------------------------
//
// A named PTY session outlives any single CLI invocation, so something
// long-lived must hold its handle. That something is this daemon: one per
// runtime dir, auto-started by the first `session start --pty`, serving
// newline-delimited JSON over a `0600` Unix socket (`daemon.sock`).
//
// INTERIM SUBSTRATE (read before touching): the daemon holds
// `tui::Session` (the crate's own portable-pty session), NOT
// `termpane::PtySession` — termpane is unreleased and workspace policy
// bars path/git imports, so no termpane import exists anywhere here. What
// the daemon reuses from termpane is the OWNER SEMANTICS contract: an
// owned handle per session, kill+reap through the handle (never raw pid
// signaling while the handle lives), and never signal after reap
// (`poll_exit()` is checked before every signal; `Session::signal`
// itself refuses `ChildExited`). MECHANICAL MIGRATION when termpane
// releases: swap the map value to `Mutex<termpane::PtySession>`
// (termpane's `close` takes `&mut self`, tui's takes `&self`), map
// `write_stdin`/`observe`/`signal`/`finish` onto the same registry
// functions, and keep this file's IPC, autostart, and endpoint logic
// untouched — none of it names the session type.
//
// Trust shape: the socket lives in the 0o700 runtime dir, is itself 0600,
// and serves only local same-uid clients (no TCP, ever — that would need
// auth, out of scope). Endpoint files stay untrusted metadata: the CLI
// validates its endpoint read before every Pty op, and the daemon
// re-validates every request name with `validate_session_name`.
//
// Files: `daemon.lock` (autostart single-flight via `NameReservation`,
// stale takeover when the owner is dead), `daemon.pid` (a hint, not
// authority — socket connectability is truth), `daemon.sock`, `daemon.err`
// (last fatal boot error, best-effort, for autostart diagnostics).
// `daemon` is a reserved session name so no session collides with these.
//
// Liveness: the daemon is authoritative for PTY sessions (`poll_exit` on
// the owned handle — no pid-reuse window while it lives). A `Pty`
// endpoint whose recorded `daemon_pid` is dead is an ORPHAN: it lists as
// `Exited`, and its child is killed only through the validated pid path
// (`stop_pid`: absolute `/bin/kill`, no PATH) on prune/force/stop — never
// by trusting the pid alone. A recorded owner that is alive but not
// serving is never guessed about (error, endpoint preserved).
//
// Residuals (documented, bounded): a daemon crash between spawn and
// endpoint publish leaks one child (no record exists yet); pid reuse can
// misdirect an orphan kill exactly as it can a piped stop (F08-F1's
// accepted residual); a recycled `daemon_pid` fails closed to "alive but
// not serving" until the pid dies. A multithreaded parent that spawns
// `tuiscotti` while concurrently creating pipes can leak fds into the
// daemon on platforms without atomic-CLOEXEC pipes (proven on macOS:
// `pipe()` + `fcntl()` races `fork()`); single-threaded parents
// (shells) are immune. Scrubbing unknown fds needs `pre_exec`/unsafe
// (forbidden here) — the real fix awaits an unsafe-policy exception —
// so the test binary instead serializes its own spawns (see
// `SPAWN_LOCK` in `tuiscotti-cli/tests/cli.rs`).

#[cfg(unix)]
use std::path::Path;
#[cfg(all(unix, feature = "pty"))]
use std::path::PathBuf;

use super::daemon_proto::DaemonOp;
#[cfg(unix)]
use super::daemon_proto::{
    DaemonRequest, DaemonResponse, IPC_TIMEOUT, MAX_RESPONSE_BYTES, read_line_capped,
};
#[cfg(all(unix, feature = "pty"))]
use super::daemon_proto::{MAX_REQUEST_BYTES, MAX_WAIT_MS, parse_request, render_response};
use super::{EXIT_OP_ERROR, OpError};
#[cfg(all(unix, feature = "pty"))]
use super::{
    NameReservation, OpResult, SessionBackend, SessionEndpoint, SessionInfo, SessionStatus,
    base64_decode, checked_endpoint_path, now_unix, read_endpoint, stop_pid, validate_session_name,
    write_endpoint,
};
#[cfg(unix)]
use super::{checked_daemon_path, current_uid, pid_alive, runtime_dir};
#[cfg(all(unix, feature = "pty"))]
use std::collections::HashMap;
#[cfg(all(unix, feature = "pty"))]
use std::os::unix::net::UnixListener;
#[cfg(unix)]
use std::os::unix::net::UnixStream;

/// Largest boot-error record the daemon leaves in `daemon.err`.
#[cfg(all(unix, feature = "pty"))]
const MAX_ERR_HINT: usize = 4096;

/// Cap on concurrent connections: one wedged child (blocking input) may
/// stall its own conn thread, never the daemon; past this the daemon
/// sheds load with a clean error instead of spawning threads forever.
#[cfg(all(unix, feature = "pty"))]
const MAX_CONNS: usize = 32;

/// Autostart waits this long for the lock and, separately, for readiness.
#[cfg(all(unix, feature = "pty"))]
const ENSURE_ATTEMPTS: u32 = 50;
/// Poll cadence inside the autostart waits.
#[cfg(all(unix, feature = "pty"))]
const ENSURE_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// Daemon entry point (`tuiscotti __daemon`, hidden): serve this runtime
/// dir until idle, then exit 0. Fatal boot errors land on stderr and in
/// `daemon.err` (best-effort) with exit 3.
#[cfg(all(unix, feature = "pty"))]
#[must_use]
pub fn daemon_main() -> i32 {
    match serve_runtime_dir() {
        Ok(()) => 0,
        Err(e) => {
            note_boot_error(&e);
            eprintln!("error: {e}");
            EXIT_OP_ERROR
        }
    }
}

/// Non-server builds fail the hidden subcommand closed instead of
/// pretending to serve.
#[cfg(not(all(unix, feature = "pty")))]
#[must_use]
pub fn daemon_main() -> i32 {
    eprintln!("error: [unsupported] the session daemon needs Unix with the `pty` feature");
    EXIT_OP_ERROR
}

// ---------------------------------------------------------------------------
// Status: is a daemon serving this runtime dir?
// ---------------------------------------------------------------------------

/// What the socket + pidfile probe found. `Live` means a connect
/// succeeded (truth); the pid is the pidfile hint, if it parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DaemonStatus {
    /// A daemon answered the socket.
    #[cfg(unix)]
    Live {
        /// Pidfile hint, when present and sane.
        pid: Option<u32>,
    },
    /// No daemon (nothing answered, recorded owner dead or absent).
    Down,
    /// A recorded owner is alive but the socket is dead: never guess.
    #[cfg(unix)]
    Unreachable {
        /// The alive-but-silent pid.
        pid: u32,
    },
}

/// A `Pty` endpoint's owner verdict: the live daemon owns it, or the
/// recorded owner is dead and the endpoint is an orphan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PtyOwner {
    /// The serving daemon owns this session: talk to it.
    Live,
    /// The recorded owner is dead: orphan rules apply (list `Exited`,
    /// validated-path kill on prune/force/stop, no transport).
    Orphan,
}

#[cfg(unix)]
pub(crate) fn status() -> Result<DaemonStatus, OpError> {
    let dir = runtime_dir()?;
    let sock = checked_daemon_path(&dir, "sock")?;
    if socket_live(&sock)? {
        return Ok(DaemonStatus::Live {
            pid: read_daemon_pid(&dir)?,
        });
    }
    match read_daemon_pid(&dir)? {
        Some(pid) if pid_alive(pid) => Ok(DaemonStatus::Unreachable { pid }),
        Some(_) | None => Ok(DaemonStatus::Down),
    }
}

/// Non-Unix builds never serve: every `Pty` endpoint is foreign, and
/// foreign endpoints list as orphans rather than failing the list.
#[cfg(not(unix))]
pub(crate) fn status() -> Result<DaemonStatus, OpError> {
    Ok(DaemonStatus::Down)
}

/// True when a daemon answers `sock`. A symlink is never followed or
/// connected to (hard error); a missing path is simply not live.
#[cfg(unix)]
fn socket_live(sock: &Path) -> Result<bool, OpError> {
    match std::fs::symlink_metadata(sock) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(OpError::new("io", format!("stat {}: {e}", sock.display())));
        }
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(OpError::new(
                    "invalid-input",
                    format!("{} is a symlink; refusing to connect", sock.display()),
                ));
            }
            check_socket_owner(sock, &meta)?;
        }
    }
    Ok(UnixStream::connect(sock).is_ok())
}

/// The socket must be ours: a foreign-owned socket in our runtime dir is
/// tamper evidence, never a server.
#[cfg(unix)]
fn check_socket_owner(sock: &Path, meta: &std::fs::Metadata) -> Result<(), OpError> {
    use std::os::unix::fs::MetadataExt;
    let me = current_uid()?;
    if meta.uid() != me {
        return Err(OpError::new(
            "owner-mismatch",
            format!("{} belongs to uid {}, not {me}", sock.display(), meta.uid()),
        ));
    }
    Ok(())
}

/// Read the pidfile hint: missing or corrupt reads as `None` (down +
/// cleanup), while symlinks, foreign owners, and oversize files are hard
/// errors. A zero pid is corrupt, never a signal target.
#[cfg(unix)]
fn read_daemon_pid(dir: &Path) -> Result<Option<u32>, OpError> {
    let path = checked_daemon_path(dir, "pid")?;
    let meta = match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(OpError::new("io", format!("stat {}: {e}", path.display())));
        }
        Ok(m) => m,
    };
    if meta.file_type().is_symlink() {
        return Err(OpError::new(
            "invalid-input",
            format!("{} is a symlink; refusing to read", path.display()),
        ));
    }
    if !meta.file_type().is_file() {
        return Err(OpError::new(
            "invalid-input",
            format!("{} is not a regular file", path.display()),
        ));
    }
    check_socket_owner(&path, &meta)?;
    if meta.len() > 1024 {
        return Err(OpError::new(
            "bound-exceeded",
            format!("{} exceeds 1024 bytes", path.display()),
        ));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| OpError::new("io", format!("read {}: {e}", path.display())))?;
    let pid: u32 = match text.trim().parse() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    if super::validate_pid(pid).is_err() {
        return Ok(None);
    }
    Ok(Some(pid))
}

/// Classify one `Pty` endpoint's recorded owner against a fresh
/// [`status`]: owned by the serving daemon, or orphaned by a dead owner.
/// A recorded pid that is alive but not the serving daemon (restarted
/// daemon, recycled pid, split brain) fails closed — never guessed.
pub(crate) fn classify_owner(recorded: u32, status: &DaemonStatus) -> Result<PtyOwner, OpError> {
    #[cfg(not(unix))]
    let _ = recorded;
    match status {
        #[cfg(unix)]
        DaemonStatus::Live { pid } if *pid == Some(recorded) => Ok(PtyOwner::Live),
        #[cfg(unix)]
        DaemonStatus::Live { .. } | DaemonStatus::Down => {
            if pid_alive(recorded) {
                Err(OpError::new(
                    "op-failed",
                    format!(
                        "owner daemon (pid {recorded}) is alive but not serving; refusing to guess"
                    ),
                ))
            } else {
                Ok(PtyOwner::Orphan)
            }
        }
        #[cfg(not(unix))]
        DaemonStatus::Down => Ok(PtyOwner::Orphan),
        #[cfg(unix)]
        DaemonStatus::Unreachable { pid } => Err(OpError::new(
            "op-failed",
            format!("owner daemon (pid {pid}) is alive but not serving; refusing to guess"),
        )),
    }
}

// ---------------------------------------------------------------------------
// Autostart: exactly one daemon per runtime dir, on demand
// ---------------------------------------------------------------------------

/// Ensure a daemon serves this runtime dir, starting one under the
/// `daemon.lock` single-flight when down. Only `session start --pty` calls
/// this: every other op must NOT resurrect a daemon (a fresh daemon owns
/// nothing, so autostart there would only mask orphans).
#[cfg(all(unix, feature = "pty"))]
pub(crate) fn ensure_live() -> Result<(), OpError> {
    let dir = runtime_dir()?;
    for _ in 0..ENSURE_ATTEMPTS {
        match status()? {
            DaemonStatus::Live { .. } => return Ok(()),
            DaemonStatus::Unreachable { pid } => {
                return Err(OpError::new(
                    "op-failed",
                    format!(
                        "daemon (pid {pid}) is alive but not serving; refusing to start a second"
                    ),
                ));
            }
            DaemonStatus::Down => match NameReservation::acquire_daemon(&dir) {
                Ok(res) => return start_under_lock(&dir, res),
                Err(e) if e.code == "session-exists" => {
                    // A racing starter holds the lock; its daemon should
                    // appear on the next probe.
                    std::thread::sleep(ENSURE_POLL);
                }
                Err(e) => return Err(e),
            },
        }
    }
    Err(OpError::new("io", "timed out waiting for the daemon lock"))
}

/// Without a server there is nothing to start: fail closed with the
/// reason instead of spawning a daemon that cannot serve.
#[cfg(not(all(unix, feature = "pty")))]
pub(crate) fn ensure_live() -> Result<(), OpError> {
    Err(OpError::new(
        "unsupported",
        "PTY sessions need Unix with the `pty` feature",
    ))
}

/// Holding the single-flight lock: re-probe (a racing starter may have
/// won), sweep stale files, spawn, and wait for readiness. The lock
/// releases on every path; the daemon never holds it (socket liveness is
/// the ownership signal once it serves).
#[cfg(all(unix, feature = "pty"))]
fn start_under_lock(dir: &Path, res: NameReservation) -> Result<(), OpError> {
    match status()? {
        DaemonStatus::Live { .. } => {
            res.release();
            return Ok(());
        }
        DaemonStatus::Unreachable { pid } => {
            drop(res);
            return Err(OpError::new(
                "op-failed",
                format!("daemon (pid {pid}) is alive but not serving; refusing to start a second"),
            ));
        }
        DaemonStatus::Down => {}
    }
    if let Err(e) = cleanup_stale_daemon_files(dir) {
        drop(res);
        return Err(e);
    }
    if let Err(e) = spawn_daemon() {
        drop(res);
        return Err(e);
    }
    let sock = checked_daemon_path(dir, "sock")?;
    for _ in 0..ENSURE_ATTEMPTS {
        if socket_live(&sock)? {
            res.release();
            return Ok(());
        }
        std::thread::sleep(ENSURE_POLL);
    }
    drop(res);
    Err(OpError::new(
        "io",
        format!("daemon did not become ready{}", boot_hint(dir)),
    ))
}

/// Remove stale socket/pidfile/err under the single-flight lock. Symlinks
/// are never removed or followed (hard error); missing files are fine.
#[cfg(all(unix, feature = "pty"))]
fn cleanup_stale_daemon_files(dir: &Path) -> Result<(), OpError> {
    for suffix in ["sock", "pid", "err"] {
        remove_unless_symlink(&checked_daemon_path(dir, suffix)?)?;
    }
    Ok(())
}

/// Remove `path` unless it is a symlink (refuse) or missing (fine).
#[cfg(all(unix, feature = "pty"))]
fn remove_unless_symlink(path: &Path) -> Result<(), OpError> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(OpError::new("io", format!("stat {}: {e}", path.display()))),
        Ok(meta) if meta.file_type().is_symlink() => Err(OpError::new(
            "invalid-input",
            format!("{} is a symlink; refusing to remove", path.display()),
        )),
        Ok(_) => match std::fs::remove_file(path) {
            Ok(()) | Err(_) => Ok(()),
        },
    }
}

/// Spawn ourselves as the daemon: same exe, hidden subcommand, inherited
/// environment (runtime dir + idle override), detached stdio.
#[cfg(all(unix, feature = "pty"))]
fn spawn_daemon() -> Result<(), OpError> {
    let exe = std::env::current_exe().map_err(|e| OpError::new("io", format!("own exe: {e}")))?;
    std::process::Command::new(exe)
        .arg("__daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| OpError::new("io", format!("spawn daemon: {e}")))?;
    Ok(())
}

/// Best-effort `daemon.err` tail for autostart failure messages.
#[cfg(all(unix, feature = "pty"))]
fn boot_hint(dir: &Path) -> String {
    let path = checked_daemon_path(dir, "err");
    let text = path
        .as_ref()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let tail: String = text.trim().chars().take(300).collect();
    if tail.is_empty() {
        String::new()
    } else {
        format!(" (daemon: {tail})")
    }
}

// ---------------------------------------------------------------------------
// Client: one request per connection
// ---------------------------------------------------------------------------

/// Connection correlation ids.
#[cfg(unix)]
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Run one op against the serving daemon (no autostart, no retry: the
/// caller owns both policies). Verifies the echoed id and unwraps the
/// envelope into the result value or the daemon's own [`OpError`].
#[cfg(unix)]
pub(crate) fn transact(op: &DaemonOp) -> Result<serde_json::Value, OpError> {
    let dir = runtime_dir()?;
    let sock = checked_daemon_path(&dir, "sock")?;
    refuse_unusable_socket(&sock)?;
    let mut stream = UnixStream::connect(&sock)
        .map_err(|e| OpError::new("io", format!("connect {}: {e}", sock.display())))?;
    transact_stream(&mut stream, op)
}

/// Transact over an already-connected stream (the unit-test seam:
/// `UnixStream::pair` stands in for a daemon).
#[cfg(unix)]
fn transact_stream(stream: &mut UnixStream, op: &DaemonOp) -> Result<serde_json::Value, OpError> {
    stream
        .set_read_timeout(Some(IPC_TIMEOUT))
        .map_err(|e| OpError::new("io", format!("socket timeout: {e}")))?;
    stream
        .set_write_timeout(Some(IPC_TIMEOUT))
        .map_err(|e| OpError::new("io", format!("socket timeout: {e}")))?;
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let req = serde_json::to_vec(&DaemonRequest { id, op: op.clone() })
        .map_err(|e| OpError::new("internal", format!("encode IPC request: {e}")))?;
    write_line(stream, &req)?;
    let line = read_line_capped(stream, MAX_RESPONSE_BYTES)?
        .ok_or_else(|| OpError::new("io", "daemon closed the connection"))?;
    let res: DaemonResponse = serde_json::from_slice(&line)
        .map_err(|e| OpError::new("io", format!("bad daemon response: {e}")))?;
    if res.id != id {
        return Err(OpError::new("io", "daemon response id mismatch"));
    }
    if res.ok {
        res.result
            .ok_or_else(|| OpError::new("internal", "daemon ok without a result"))
    } else {
        Err(res
            .error
            .unwrap_or_else(|| OpError::new("internal", "daemon failed without an error")))
    }
}

/// The socket must exist, must not be a symlink, and must be ours.
#[cfg(unix)]
fn refuse_unusable_socket(sock: &Path) -> Result<(), OpError> {
    let meta = std::fs::symlink_metadata(sock).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            OpError::new("io", format!("no daemon socket at {}", sock.display()))
        } else {
            OpError::new("io", format!("stat {}: {e}", sock.display()))
        }
    })?;
    if meta.file_type().is_symlink() {
        return Err(OpError::new(
            "invalid-input",
            format!("{} is a symlink; refusing to connect", sock.display()),
        ));
    }
    check_socket_owner(sock, &meta)
}

/// Write one `\n`-terminated line.
#[cfg(unix)]
fn write_line(stream: &mut UnixStream, bytes: &[u8]) -> Result<(), OpError> {
    use std::io::Write as _;
    stream
        .write_all(bytes)
        .and_then(|()| stream.write_all(b"\n"))
        .and_then(|()| stream.flush())
        .map_err(|e| OpError::new("io", format!("IPC write: {e}")))
}

/// Non-Unix builds have no socket to transact on.
#[cfg(not(unix))]
pub(crate) fn transact(op: &DaemonOp) -> Result<serde_json::Value, OpError> {
    let _ = op;
    Err(OpError::new("unsupported", "the session daemon needs Unix"))
}

// ---------------------------------------------------------------------------
// Server: bind, serve, idle out
// ---------------------------------------------------------------------------

/// Bind the runtime dir's socket and serve until the registry sits empty
/// past the idle limit. Every connection gets one thread (capped);
/// registry ops run through the shared `pty_registry` functions.
#[cfg(all(unix, feature = "pty"))]
fn serve_runtime_dir() -> Result<(), OpError> {
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
                    super::pty_registry::close_all();
                    break;
                }
                if !super::pty_registry::is_empty() {
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
    use super::pty_registry;
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
fn result_value(result: &OpResult) -> Result<serde_json::Value, OpError> {
    serde_json::to_value(result)
        .map_err(|e| OpError::new("internal", format!("encode result: {e}")))
}

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
struct StartArgs<'a> {
    argv_b64: &'a [String],
    cwd: Option<&'a str>,
    cols: Option<u16>,
    rows: Option<u16>,
    force: bool,
}

#[cfg(all(unix, feature = "pty"))]
fn daemon_start(
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
        super::pty_registry::drop_session(name);
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
        version: super::SESSION_ENDPOINT_VERSION,
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
        super::pty_registry::drop_session(name);
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
    match super::pty_registry::spawn_os(
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
        return super::session_stop(name).map(|_| ());
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
    let live = super::pty_registry::sessions_status()
        .iter()
        .any(|s| s.name == name && s.running);
    if live {
        if !force {
            return Err(OpError::new(
                "session-exists",
                format!("{name} already running (pid {})", ep.pid),
            ));
        }
        super::pty_registry::stop(name)?;
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
fn guard_single_owner(ep: &SessionEndpoint) -> Result<(), OpError> {
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

/// Stop a session and remove its endpoint. Piped names delegate to the
/// piped stop; PTY names stop through the owned handle (TERM, grace,
/// kill+reap) with the entry AND the endpoint preserved on any failure.
/// An endpoint without a registry entry is already-exited: kill a
/// lingering child through the validated pid path, remove the record.
#[cfg(all(unix, feature = "pty"))]
fn daemon_stop(dir: &Path, name: &str) -> Result<serde_json::Value, OpError> {
    let Some(ep) = read_endpoint(dir, name)? else {
        // No record: drop any leaked entry, then report truthfully.
        super::pty_registry::drop_session(name);
        return Err(OpError::new("not-found", name));
    };
    if ep.backend == SessionBackend::Process {
        return result_value(&OpResult::Session {
            session: super::session_stop(name)?,
        });
    }
    guard_single_owner(&ep)?;
    match super::pty_registry::stop(name) {
        Ok(()) => {}
        Err(e) if e.code == "not-found" => {
            if pid_alive(ep.pid) {
                stop_pid(ep.pid)?;
            }
        }
        Err(e) => return Err(e),
    }
    remove_endpoint(dir, name)?;
    result_value(&OpResult::Session {
        session: SessionInfo {
            name: ep.name,
            pid: ep.pid,
            argv: ep.argv,
            backend: SessionBackend::Pty,
            status: SessionStatus::Exited,
            started_unix: ep.started_unix,
        },
    })
}

/// Prune the named PTY sessions: drop each registry entry that is already
/// exited (a running entry is NEVER pruned — a start racing the prune
/// must not lose its session), then remove its endpoint record. Piped
/// names are ignored here (the CLI prunes those locally); tampered
/// records abort the prune instead of deleting around them.
#[cfg(all(unix, feature = "pty"))]
fn daemon_prune(dir: &Path, names: &[String]) -> Result<serde_json::Value, OpError> {
    let mut pruned = Vec::new();
    for name in names {
        if !super::pty_registry::drop_exited(name) {
            continue;
        }
        match read_endpoint(dir, name)? {
            None => {}
            Some(ep) if ep.backend != SessionBackend::Pty => {}
            Some(_) => {
                remove_endpoint(dir, name)?;
                pruned.push(name.clone());
            }
        }
    }
    Ok(serde_json::json!({"pruned": pruned}))
}

/// Remove one endpoint file by its validated listing stem — never by an
/// untrusted payload field (the stem was validated before this call).
#[cfg(all(unix, feature = "pty"))]
fn remove_endpoint(dir: &Path, name: &str) -> Result<(), OpError> {
    std::fs::remove_file(checked_endpoint_path(dir, name)?)
        .map_err(|e| OpError::new("io", format!("remove {name}: {e}")))?;
    Ok(())
}

/// Best-effort fatal-error record for autostart diagnostics: one capped
/// line in `daemon.err`, never a panic, never a second failure mode.
#[cfg(all(unix, feature = "pty"))]
fn note_boot_error(e: &OpError) {
    let Ok(dir) = runtime_dir() else {
        return;
    };
    let Ok(path) = checked_daemon_path(&dir, "err") else {
        return;
    };
    let line: String = e.to_string().chars().take(MAX_ERR_HINT).collect();
    if std::fs::write(&path, line).is_err() {
        // Best-effort diagnostics; stderr already carries the error.
    }
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

    #[cfg(unix)]
    #[test]
    fn owner_live_match_serves() {
        let me = std::process::id();
        let st = DaemonStatus::Live { pid: Some(me) };
        assert_eq!(classify_owner(me, &st).expect("own daemon"), PtyOwner::Live);
    }

    #[cfg(unix)]
    #[test]
    fn owner_dead_recorded_is_orphan() {
        // `u32::MAX` is never a live pid: every status agrees orphan.
        let dead = u32::MAX;
        assert!(!pid_alive(dead));
        for st in [
            DaemonStatus::Live { pid: None },
            DaemonStatus::Live { pid: Some(1) },
            DaemonStatus::Down,
        ] {
            assert_eq!(classify_owner(dead, &st).expect("orphan"), PtyOwner::Orphan);
        }
    }

    #[cfg(unix)]
    #[test]
    fn owner_alive_foreign_never_guessed() {
        // Our own pid is alive but is not the serving daemon in any of
        // these shapes: every one fails closed with the endpoint kept.
        let me = std::process::id();
        assert!(pid_alive(me));
        for st in [
            DaemonStatus::Live { pid: None },
            DaemonStatus::Live { pid: Some(me ^ 1) },
            DaemonStatus::Down,
            DaemonStatus::Unreachable { pid: me },
        ] {
            let e = classify_owner(me, &st).expect_err("must not guess");
            assert_eq!(e.code, "op-failed");
        }
        let e = classify_owner(u32::MAX, &DaemonStatus::Unreachable { pid: 1 })
            .expect_err("unreachable guesses nothing");
        assert_eq!(e.code, "op-failed");
    }

    #[cfg(unix)]
    #[test]
    fn transact_stream_round_trip_and_errors() {
        // Fake daemon over a socket pair: speaks the real envelope.
        let (mut cli, mut srv) = UnixStream::pair().expect("socket pair");
        let worker = std::thread::spawn(move || {
            use std::io::{Read as _, Write as _};
            let mut buf = Vec::new();
            let mut one = [0u8; 1];
            loop {
                srv.read_exact(&mut one).expect("request byte");
                if one[0] == b'\n' {
                    break;
                }
                buf.push(one[0]);
            }
            let req: DaemonRequest = serde_json::from_slice(&buf).expect("request json");
            let res = match req.op {
                DaemonOp::List => DaemonResponse::ok(req.id, serde_json::json!({"sessions": []})),
                DaemonOp::Stop { .. } => {
                    DaemonResponse::err(req.id, OpError::new("not-found", "gone"))
                }
                _ => DaemonResponse::err(req.id + 1, OpError::new("internal", "crossed")),
            };
            let mut out = serde_json::to_vec(&res).expect("response json");
            out.push(b'\n');
            srv.write_all(&out).expect("reply");
        });
        let value = transact_stream(&mut cli, &DaemonOp::List).expect("list round trip");
        assert_eq!(value, serde_json::json!({"sessions": []}));
        worker.join().expect("fake daemon");
        // Daemon errors surface verbatim with their code.
        let (mut cli, mut srv) = UnixStream::pair().expect("socket pair");
        let worker = std::thread::spawn(move || {
            use std::io::{Read as _, Write as _};
            let mut buf = Vec::new();
            let mut one = [0u8; 1];
            loop {
                srv.read_exact(&mut one).expect("request byte");
                if one[0] == b'\n' {
                    break;
                }
                buf.push(one[0]);
            }
            let req: DaemonRequest = serde_json::from_slice(&buf).expect("request json");
            let res = DaemonResponse::err(req.id, OpError::new("not-found", "gone"));
            let mut out = serde_json::to_vec(&res).expect("response json");
            out.push(b'\n');
            srv.write_all(&out).expect("reply");
        });
        let e = transact_stream(
            &mut cli,
            &DaemonOp::Stop {
                name: "ghost".to_string(),
            },
        )
        .expect_err("daemon error must surface");
        assert_eq!(e.code, "not-found");
        worker.join().expect("fake daemon");
    }

    #[cfg(unix)]
    #[test]
    fn transact_stream_rejects_crossed_id() {
        let (mut cli, mut srv) = UnixStream::pair().expect("socket pair");
        let worker = std::thread::spawn(move || {
            use std::io::{Read as _, Write as _};
            let mut buf = Vec::new();
            let mut one = [0u8; 1];
            loop {
                srv.read_exact(&mut one).expect("request byte");
                if one[0] == b'\n' {
                    break;
                }
                buf.push(one[0]);
            }
            let req: DaemonRequest = serde_json::from_slice(&buf).expect("request json");
            let res = DaemonResponse::ok(req.id + 1, serde_json::json!(null));
            let mut out = serde_json::to_vec(&res).expect("response json");
            out.push(b'\n');
            srv.write_all(&out).expect("reply");
        });
        let e = transact_stream(&mut cli, &DaemonOp::List).expect_err("crossed id accepted");
        assert_eq!(e.code, "io");
        worker.join().expect("fake daemon");
    }
}
