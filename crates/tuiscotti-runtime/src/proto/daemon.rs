// ---------------------------------------------------------------------------
// Retained-session daemon (F08-F2): one owner per runtime dir
// ---------------------------------------------------------------------------
//
// A named PTY session outlives any single CLI invocation, so something
// long-lived must hold its handle. That something is this daemon: one per
// runtime dir, auto-started by the first `session start --pty`, serving
// newline-delimited JSON over a `0600` Unix socket (`daemon.sock`).
//
// SUBSTRATE (read before touching): the daemon holds `tui::Session`,
// whose backend is termpane =0.1.0 from crates.io. The daemon reuses
// termpane's OWNER SEMANTICS contract: an owned handle per session,
// kill+reap through the handle (never raw pid signaling while the
// handle lives), and never signal after reap (`poll_exit()` is checked
// before every signal; `Session::signal` itself refuses `ChildExited`).
// This file's IPC, autostart, and endpoint logic never names the
// session type.
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
//
// Split into one module per area so each file stays under the repo line
// gate; behavior is unchanged.

use super::EXIT_OP_ERROR;
#[cfg(all(unix, feature = "pty"))]
use super::OpError;
#[cfg(all(unix, feature = "pty"))]
use super::{checked_daemon_path, runtime_dir};
#[cfg(all(unix, feature = "pty"))]
use serve::serve_runtime_dir;

mod ensure;
mod serve;
mod start;
mod status;
mod stop;
mod transact;

pub(crate) use ensure::ensure_live;
pub(crate) use status::{DaemonStatus, PtyOwner, classify_owner, status};
pub(crate) use transact::transact;

/// Largest boot-error record the daemon leaves in `daemon.err`.
#[cfg(all(unix, feature = "pty"))]
const MAX_ERR_HINT: usize = 4096;

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

/// Best-effort fatal-error record for autostart diagnostics: one capped
/// line in `daemon.err`, never a panic, never a second failure mode. A lost
/// autostart race is NOT a boot error: `bind_or_conflict` is the sole
/// `session-exists` producer at boot, and it fires only when another daemon
/// already serves (the boot succeeded — this process is redundant), so
/// recording it would leave a bogus failure for a healthy runtime dir.
#[cfg(all(unix, feature = "pty"))]
fn note_boot_error(e: &OpError) {
    if e.code == "session-exists" {
        return;
    }
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
