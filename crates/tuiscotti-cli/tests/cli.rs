//! CLI core + typed op protocol tests (A01, A02-partial, A04-partial).
//!
//! Covers [`tuiscotti::proto::execute`] for every op (happy + error paths), the
//! `machine` JSON-lines shape, every CLI subcommand round trip in temp dirs,
//! exit codes, and the inspect/import never-executes guarantee.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tuiscotti::proto;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tuiscotti"))
}

/// Serializes process spawning across this test binary. Platforms
/// without atomic-CLOEXEC pipes (macOS `pipe()` + `fcntl()`) race: a
/// `fork()` landing between another thread's `pipe()` and its CLOEXEC
/// setup inherits the sibling's pipe fds into the child. Harmless for
/// short-lived children, fatal for the retained-session daemon — it pins
/// the victim's pipes forever and the victim's `wait_with_output` hangs
/// on a zombie child. The lock covers `spawn()` only (pipe creation,
/// fork, and CLOEXEC setup all happen inside it), so waits still run in
/// parallel and the suite stays fast. Every spawn in this binary must go
/// through here, including one-shot probes.
static SPAWN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn spawn_locked(cmd: &mut Command) -> std::io::Result<std::process::Child> {
    let _guard = SPAWN_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cmd.spawn()
}

fn run_cli(
    args: &[&str],
    env_extra: &[(&str, &str)],
    stdin: Option<&str>,
) -> std::io::Result<Output> {
    let mut cmd = Command::new(bin());
    cmd.args(args);
    for (k, v) in env_extra {
        cmd.env(k, v);
    }
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = spawn_locked(&mut cmd)?;
    if let Some(input) = stdin {
        use std::io::Write;
        child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("piped stdin"))?
            .write_all(input.as_bytes())?;
    }
    child.wait_with_output()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn run_cli_cwd(cwd: &Path, args: &[&str]) -> std::io::Result<Output> {
    let mut cmd = Command::new(bin());
    cmd.args(args).current_dir(cwd);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    spawn_locked(&mut cmd)?.wait_with_output()
}

fn code(out: &Output) -> Option<i32> {
    out.status.code()
}

/// Serializes tests that override the runtime dir process-wide.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_runtime_dir<T>(f: impl FnOnce(&Path) -> T) -> std::io::Result<T> {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tmp = tempfile::tempdir()?;
    let dir = tmp.path().join("rt");
    // Explicit override: `set_var` is an `unsafe fn` in edition 2024 and
    // cannot be used under the workspace lints.
    proto::set_runtime_dir_override(Some(dir.clone()));
    let out = f(&dir);
    proto::set_runtime_dir_override(None);
    Ok(out)
}

fn pty_available() -> bool {
    proto::capabilities().pty
}

fn blank_frame_json(cols: u16, rows: u16) -> String {
    let screen = tuiscotti::Screen::blank(cols, rows);
    tuiscotti::assert::frame_from_screen(&screen).to_json()
}

#[path = "cli/ops.rs"]
mod ops;

#[path = "cli/ops_session.rs"]
mod ops_session;

#[cfg(all(unix, feature = "pty"))]
#[path = "cli/ops_session_pty.rs"]
mod ops_session_pty;

#[path = "cli/cli_core.rs"]
mod cli_core;

#[path = "cli/cli_ops.rs"]
mod cli_ops;

#[path = "cli/cli_machine_accept.rs"]
mod cli_machine_accept;
