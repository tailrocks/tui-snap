//! Closed-stdout regression: report commands must not panic with EPIPE.
//!
//! `println!` panics when stdout is closed (`tuisnap doctor | head -c0`
//! exited 101). Pure-report commands buffer and flush once through a
//! broken-pipe-tolerant writer, so a vanished reader is a clean exit 0.

use std::path::PathBuf;
use std::process::Stdio;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tuisnap"))
}

/// Spawn `tuisnap <args>` with a piped stdout whose read end is dropped
/// immediately, then return (exit code, stderr).
fn run_with_closed_stdout(args: &[&str]) -> (Option<i32>, String) {
    let mut child = std::process::Command::new(bin())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tuisnap");
    // Drop the read end before the child writes: the next stdout write
    // fails with EPIPE (Rust ignores SIGPIPE).
    drop(child.stdout.take());
    let out = child.wait_with_output().expect("wait tuisnap");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn doctor_closed_stdout_exits_zero() {
    // Single iteration: `doctor` probes subprocesses before writing, so the
    // reader is always gone by flush time.
    let (code, stderr) = run_with_closed_stdout(&["doctor"]);
    assert_eq!(code, Some(0), "doctor over closed stdout: {stderr}");
    assert!(
        !stderr.contains("panicked"),
        "doctor must not panic: {stderr}"
    );
}

#[test]
fn schema_closed_stdout_exits_zero() {
    // Same writer path as `doctor`, without the slow toolchain probes.
    for _ in 0..3 {
        let (code, stderr) = run_with_closed_stdout(&["schema"]);
        assert_eq!(code, Some(0), "schema over closed stdout: {stderr}");
        assert!(
            !stderr.contains("panicked"),
            "schema must not panic: {stderr}"
        );
    }
}
