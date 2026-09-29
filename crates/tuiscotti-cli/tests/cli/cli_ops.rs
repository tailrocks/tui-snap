//! CLI: review, report, import, session, record, trace (split from `cli.rs`; shared helpers live in the root).

use super::{blank_frame_json, code, run_cli, stdout};
use std::path::PathBuf;
use tuiscotti::proto::{EXIT_OP_ERROR, EXIT_VERIFY_FAIL};

// ---------------------------------------------------------------------------
// CLI: review / report / import (offline)
// ---------------------------------------------------------------------------

#[test]
fn cli_review_and_report() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        tmp.path().join("one.verdict.json"),
        r#"{"name":"one","status":"pass","detail":""}"#,
    )
    .expect("v1");
    std::fs::write(
        tmp.path().join("two.verdict.json"),
        r#"{"name":"two","status":"fail","detail":"pixels differ"}"#,
    )
    .expect("v2");
    let out = run_cli(
        &["review", "--dir", tmp.path().to_str().expect("utf8 path")],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), EXIT_VERIFY_FAIL);
    assert!(stdout(&out).contains("two"), "{}", stdout(&out));
    let html = tmp.path().join("report.html");
    let out = run_cli(
        &[
            "report",
            "--dir",
            tmp.path().to_str().expect("utf8 path"),
            "--out",
            html.to_str().expect("utf8 path"),
        ],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(
        code(&out).expect("exit code"),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let body = std::fs::read_to_string(&html).expect("report");
    assert!(body.contains("two") && body.contains("1 failed"), "{body}");
}

#[test]
fn cli_import_readonly() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let before: Vec<PathBuf> = std::fs::read_dir(tmp.path())
        .expect("read")
        .map(|e| e.expect("entry").path())
        .collect();
    let out = run_cli(
        &["import", "--dir", tmp.path().to_str().expect("utf8 path")],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(
        code(&out).expect("exit code"),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout(&out).contains('0'), "{}", stdout(&out));
    let after: Vec<PathBuf> = std::fs::read_dir(tmp.path())
        .expect("read")
        .map(|e| e.expect("entry").path())
        .collect();
    assert_eq!(before, after, "import writes nothing");
}

// ---------------------------------------------------------------------------
// CLI: named sessions
// ---------------------------------------------------------------------------

#[test]
fn cli_session_round_trip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let rt = tmp.path().join("rt").to_string_lossy().into_owned();
    let env = &[("TUISNAP_RUNTIME_DIR", rt.as_str())][..];
    // stop-before-start is a clean op error, not a crash
    let out = run_cli(&["session", "stop", "--name", "ghost"], env, None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), EXIT_OP_ERROR);
    let out = run_cli(
        &["session", "start", "--name", "s1", "--", "sleep", "30"],
        env,
        None,
    )
    .expect("run tuisnap");
    assert_eq!(
        code(&out).expect("exit code"),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run_cli(&["session", "list"], env, None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    assert!(stdout(&out).contains("s1"), "{}", stdout(&out));
    let out = run_cli(
        &["session", "start", "--name", "s1", "--", "sleep", "1"],
        env,
        None,
    )
    .expect("run tuisnap");
    assert_eq!(
        code(&out).expect("exit code"),
        EXIT_OP_ERROR,
        "collision without --force"
    );
    let out = run_cli(
        &[
            "session", "start", "--name", "s1", "--force", "--", "sleep", "30",
        ],
        env,
        None,
    )
    .expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    let out = run_cli(&["session", "stop", "--name", "s1"], env, None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    let out = run_cli(&["session", "list"], env, None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    assert!(!stdout(&out).contains("s1"), "{}", stdout(&out));
    let out = run_cli(&["session", "prune"], env, None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
}

// ---------------------------------------------------------------------------
// CLI: record + trace
// ---------------------------------------------------------------------------

#[test]
fn cli_record_and_trace() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let journal = tmp.path().join("trace.jsonl");
    let out = run_cli(
        &[
            "record",
            "--out",
            journal.to_str().expect("utf8 path"),
            "--max-events",
            "100",
            "--max-bytes",
            "100000",
            "--",
            "echo",
            "rec-hi",
        ],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(
        code(&out).expect("exit code"),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run_cli(
        &["trace", "--input", journal.to_str().expect("utf8 path")],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    let t = stdout(&out);
    assert!(t.contains("start") && t.contains("exit"), "{t}");
    let out = run_cli(
        &[
            "trace",
            "--input",
            journal.to_str().expect("utf8 path"),
            "--kind",
            "exit",
        ],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    assert!(!stdout(&out).contains("start"), "kind filter applies");
    // tiny bound trips the recorder instead of truncating silently
    let small = tmp.path().join("small.jsonl");
    let out = run_cli(
        &[
            "record",
            "--out",
            small.to_str().expect("utf8 path"),
            "--max-events",
            "1",
            "--max-bytes",
            "100000",
            "--",
            "echo",
            "x",
        ],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), EXIT_OP_ERROR);
}

// ---------------------------------------------------------------------------
// CLI: missing values are usage errors (exit 2), never op errors (exit 3)
// ---------------------------------------------------------------------------

#[test]
fn cli_missing_values_are_usage_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let frame = tmp.path().join("f.frame.json");
    std::fs::write(&frame, blank_frame_json(10, 4)).expect("frame");
    let frame_arg = frame.to_str().expect("utf8").to_string();
    let out_arg = tmp.path().join("o").to_string_lossy().into_owned();
    // `render` without any --format: usage error, like a typed rejection.
    let out = run_cli(
        &["render", "--input", &frame_arg, "--out", &out_arg],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(
        code(&out).expect("exit code"),
        2,
        "empty --format must be exit 2: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Typed --format rejection stays exit 2 with the valid set listed.
    let out = run_cli(
        &[
            "render", "--input", &frame_arg, "--format", "mp4", "--out", &out_arg,
        ],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 2);
    // Missing child argv after `--`: usage error on every spawner.
    let cap = tmp.path().join("cap").to_string_lossy().into_owned();
    let out = run_cli(&["capture", "--out", &cap], &[], None).expect("run tuisnap");
    assert_eq!(
        code(&out).expect("exit code"),
        2,
        "capture without argv must be exit 2: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rec = tmp.path().join("r.jsonl").to_string_lossy().into_owned();
    let out = run_cli(&["record", "--out", &rec], &[], None).expect("run tuisnap");
    assert_eq!(
        code(&out).expect("exit code"),
        2,
        "record without argv must be exit 2: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run_cli(&["session", "start", "--name", "noargv"], &[], None).expect("run tuisnap");
    assert_eq!(
        code(&out).expect("exit code"),
        2,
        "session start without argv must be exit 2: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---------------------------------------------------------------------------
// CLI: trace --kind is typed (exit 2 on unknown, valid set listed)
// ---------------------------------------------------------------------------

#[test]
fn cli_trace_typed_kind() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let journal = tmp.path().join("trace.jsonl");
    let journal_arg = journal.to_string_lossy().into_owned();
    let out = run_cli(
        &["record", "--out", &journal_arg, "--", "echo", "kind-hi"],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    // Every typed kind is accepted.
    for kind in ["start", "output", "exit", "complete"] {
        let out = run_cli(
            &["trace", "--input", &journal_arg, "--kind", kind],
            &[],
            None,
        )
        .expect("run tuisnap");
        assert_eq!(
            code(&out).expect("exit code"),
            0,
            "kind {kind}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // Unknown kinds are usage errors with the valid set listed.
    let out = run_cli(
        &["trace", "--input", &journal_arg, "--kind", "bogus"],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 2);
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    for kind in ["start", "output", "exit", "complete"] {
        assert!(err.contains(kind), "stderr lists {kind}:\n{err}");
    }
}
