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
    let mut child = cmd.spawn()?;
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
    cmd.output()
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

#[path = "cli/cli_core.rs"]
mod cli_core;

#[path = "cli/cli_ops.rs"]
mod cli_ops;

#[path = "cli/cli_machine_accept.rs"]
mod cli_machine_accept;
