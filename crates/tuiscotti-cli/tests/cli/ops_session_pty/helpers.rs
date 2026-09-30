//! Shared helpers for the `session --pty` CLI tests.
//!
//! Helpers return `Result`: only `#[test]` bodies may expect/panic.

use std::path::Path;
use std::process::Output;

use super::super::{code, run_cli, spawn_locked, stdout};

/// Test-helper result: `String` errors, surfaced by the test's `expect`.
pub(super) type TRes<T> = Result<T, String>;

/// Isolated runtime dir + 1 s daemon idle for one test.
pub(super) fn pty_env(rt: &str) -> [(&str, &str); 2] {
    [
        ("TUISCOTTI_RUNTIME_DIR", rt),
        ("TUISCOTTI_DAEMON_IDLE_SECS", "1"),
    ]
}

pub(super) fn fresh_rt(test: &str) -> TRes<(tempfile::TempDir, String)> {
    let tmp = tempfile::tempdir().map_err(|e| format!("tempdir: {e}"))?;
    let rt = tmp.path().join(test).to_string_lossy().into_owned();
    Ok((tmp, rt))
}

pub(super) fn check_code(out: &Output, want: i32) -> TRes<()> {
    let got = code(out).ok_or_else(|| "no exit code".to_string())?;
    if got != want {
        return Err(format!(
            "exit {got}, want {want}: stdout={} stderr={}",
            stdout(out),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// Parse `started: {name} (pid {N})` from a start's stdout.
pub(super) fn started_pid(out: &Output) -> TRes<u32> {
    let text = stdout(out);
    let (_, pid) = text
        .trim()
        .rsplit_once("(pid ")
        .ok_or_else(|| format!("no pid in {text:?}"))?;
    pid.trim_end_matches(')')
        .trim()
        .parse()
        .map_err(|_| format!("bad pid in {text:?}"))
}

/// First absolute `kill(1)` that is a regular file (never a PATH lookup).
pub(super) fn kill_bin() -> TRes<&'static str> {
    for bin in ["/bin/kill", "/usr/bin/kill"] {
        if std::fs::symlink_metadata(bin).is_ok_and(|m| m.file_type().is_file()) {
            return Ok(bin);
        }
    }
    Err("no kill binary".to_string())
}

/// True when `kill -0` says the pid is gone (spawn under the shared
/// lock: every fork in this binary takes it, piped or not).
pub(super) fn pid_dead(pid: u32) -> TRes<bool> {
    let bin = kill_bin()?;
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("-0")
        .arg(pid.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = spawn_locked(&mut cmd)
        .map_err(|e| format!("kill -0 {pid}: {e}"))?
        .wait_with_output()
        .map_err(|e| format!("kill -0 {pid}: {e}"))?;
    Ok(!out.status.success())
}

/// Poll `cond` until true or `secs` elapse.
pub(super) fn wait_for(what: &str, secs: u64, mut cond: impl FnMut() -> TRes<bool>) -> TRes<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if cond()? {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Err(format!("timed out waiting for {what}"))
}

/// Poll `session observe` until the screen contains `needle`.
pub(super) fn observe_until(
    env: &[(&str, &str)],
    name: &str,
    needle: &str,
    secs: u64,
) -> TRes<String> {
    let mut last = String::new();
    wait_for(&format!("{name:?} to show {needle:?}"), secs, || {
        let out = run_cli(&["session", "observe", "--name", name], env, None)
            .map_err(|e| e.to_string())?;
        if code(&out) != Some(0) {
            return Ok(false);
        }
        last = stdout(&out);
        Ok(last.contains(needle))
    })?;
    Ok(last)
}

/// Poll `session list` until `name` lists with `status` (`Running`/`Exited`).
pub(super) fn list_until(env: &[(&str, &str)], name: &str, status: &str, secs: u64) -> TRes<()> {
    wait_for(&format!("{name:?} to list {status}"), secs, || {
        let out = run_cli(&["session", "list"], env, None).map_err(|e| e.to_string())?;
        Ok(code(&out) == Some(0)
            && stdout(&out)
                .lines()
                .any(|l| l.contains(name) && l.contains(status)))
    })
}

pub(super) fn daemon_pid_of(rt: &Path) -> Option<u32> {
    std::fs::read_to_string(rt.join("daemon.pid"))
        .ok()
        .and_then(|t| t.trim().parse().ok())
}

pub(super) fn kill9(pid: u32) -> TRes<()> {
    let bin = kill_bin()?;
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("-9").arg(pid.to_string());
    let st = spawn_locked(&mut cmd)
        .map_err(|e| format!("kill -9 {pid}: {e}"))?
        .wait()
        .map_err(|e| format!("kill -9 {pid}: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("kill -9 {pid} failed"))
    }
}

/// End-of-test gate: every known child reaped, the daemon exited, and
/// (clean shutdowns only) its socket + pidfile swept.
pub(super) fn teardown(rt: &Path, children: &[u32], swept: bool) -> TRes<()> {
    for pid in children {
        wait_for(&format!("child {pid} to die"), 10, || pid_dead(*pid))?;
    }
    if let Some(d) = daemon_pid_of(rt) {
        wait_for(&format!("daemon {d} to idle out"), 15, || pid_dead(d))?;
    }
    if swept {
        if rt.join("daemon.sock").exists() {
            return Err("daemon socket not swept after idle exit".to_string());
        }
        if rt.join("daemon.pid").exists() {
            return Err("daemon pidfile not swept after idle exit".to_string());
        }
    }
    Ok(())
}
