//! Closed-stdout regression: no subcommand may panic with EPIPE.
//!
//! `println!` panics when stdout is closed (`tuiscotti doctor | head -c0`
//! exited 101). Every stdout path goes through a broken-pipe-tolerant
//! writer — buffered commands flush once, streaming commands
//! (`machine`, `trace`) write line-by-line — so a vanished reader is a
//! clean exit 0.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::Stdio;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tuiscotti"))
}

/// Spawn `tuiscotti <args>` with a piped stdout whose read end is dropped
/// immediately, then return (exit code, stderr).
fn run_with_closed_stdout(args: &[&str]) -> std::io::Result<(Option<i32>, String)> {
    let mut child = std::process::Command::new(bin())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // Drop the read end before the child writes: the next stdout write
    // fails with EPIPE (Rust ignores SIGPIPE).
    drop(child.stdout.take());
    let out = child.wait_with_output()?;
    Ok((
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// [`run_with_closed_stdout`] with `stdin_text` fed on stdin first (for
/// `machine`, which only writes after reading an op line).
fn run_with_closed_stdout_and_stdin(
    args: &[&str],
    stdin_text: &str,
) -> std::io::Result<(Option<i32>, String)> {
    let mut child = std::process::Command::new(bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("piped stdin"))?
        .write_all(stdin_text.as_bytes())?;
    // stdin is EOF-closed by the dropped handle above; now drop the stdout
    // read end so envelope writes fail with EPIPE.
    drop(child.stdout.take());
    let out = child.wait_with_output()?;
    Ok((
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

fn assert_clean_exit_zero(label: &str, args: &[&str]) -> std::io::Result<()> {
    for _ in 0..3 {
        let (code, stderr) = run_with_closed_stdout(args)?;
        assert_eq!(code, Some(0), "{label} over closed stdout: {stderr}");
        assert!(
            !stderr.contains("panicked"),
            "{label} must not panic: {stderr}"
        );
    }
    Ok(())
}

#[test]
fn doctor_closed_stdout_exits_zero() {
    // Single iteration: `doctor` probes subprocesses before writing, so the
    // reader is always gone by flush time.
    let (code, stderr) = run_with_closed_stdout(&["doctor"]).expect("run over closed stdout");
    assert_eq!(code, Some(0), "doctor over closed stdout: {stderr}");
    assert!(
        !stderr.contains("panicked"),
        "doctor must not panic: {stderr}"
    );
}

#[test]
fn schema_closed_stdout_exits_zero() {
    // Same writer path as `doctor`, without the slow toolchain probes.
    assert_clean_exit_zero("schema", &["schema"]).expect("closed-stdout check");
}

#[test]
fn machine_closed_stdout_exits_zero() {
    // Streaming line writer: envelopes hit EPIPE instead of panicking.
    // Many op lines so at least one write lands after the read end drops.
    let input = "{\"type\":\"version\"}\n".repeat(50);
    for _ in 0..3 {
        let (code, stderr) =
            run_with_closed_stdout_and_stdin(&["machine"], &input).expect("run over closed stdout");
        assert_eq!(code, Some(0), "machine over closed stdout: {stderr}");
        assert!(
            !stderr.contains("panicked"),
            "machine must not panic: {stderr}"
        );
    }
}

#[test]
fn offline_commands_closed_stdout_exit_zero() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // inspect fixture: a few files + a manifest.
    let art = tmp.path().join("art");
    std::fs::create_dir(&art).expect("mkdir");
    std::fs::write(art.join("stdout.bin"), b"hi").expect("write");
    std::fs::write(art.join("manifest.json"), r#"{"code":0}"#).expect("write");
    let art_arg = art.to_string_lossy().into_owned();
    // trace fixture: a small journal.
    let journal = tmp.path().join("t.jsonl");
    std::fs::write(
        &journal,
        "{\"seq\":0,\"kind\":\"start\",\"detail\":\"argv\"}\n{\"seq\":1,\"kind\":\"exit\",\"detail\":\"code\"}\n",
    )
    .expect("write");
    let journal_arg = journal.to_string_lossy().into_owned();
    // review fixture: verdicts (use a separate dir; review reads *.verdict.json).
    let verdicts = tmp.path().join("v");
    std::fs::create_dir(&verdicts).expect("mkdir");
    std::fs::write(
        verdicts.join("one.verdict.json"),
        r#"{"name":"one","status":"pass","detail":""}"#,
    )
    .expect("write");
    let verdicts_arg = verdicts.to_string_lossy().into_owned();
    // render fixture: a canonical frame.
    let screen = tuiscotti::Screen::blank(20, 5);
    let frame = tmp.path().join("f.frame.json");
    std::fs::write(
        &frame,
        tuiscotti::assert::frame_from_screen(&screen).to_json(),
    )
    .expect("write");
    let frame_arg = frame.to_string_lossy().into_owned();
    let render_out = tmp.path().join("shot").to_string_lossy().into_owned();

    assert_clean_exit_zero("inspect", &["inspect", "--dir", &art_arg])
        .expect("closed-stdout check");
    assert_clean_exit_zero("trace", &["trace", "--input", &journal_arg])
        .expect("closed-stdout check");
    assert_clean_exit_zero("review", &["review", "--dir", &verdicts_arg])
        .expect("closed-stdout check");
    assert_clean_exit_zero(
        "render",
        &[
            "render",
            "--input",
            &frame_arg,
            "--format",
            "txt",
            "--out",
            &render_out,
        ],
    )
    .expect("closed-stdout check");
}
