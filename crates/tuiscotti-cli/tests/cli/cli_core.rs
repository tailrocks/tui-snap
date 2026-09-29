//! CLI: help, schema, init, capture, inspect, render, diff (split from `cli.rs`; shared helpers live in the root).

use super::{blank_frame_json, code, run_cli, stdout};
use tuiscotti::proto::{self, EXIT_OP_ERROR, EXIT_VERIFY_FAIL};

// ---------------------------------------------------------------------------
// CLI: help / version / schema / doctor
// ---------------------------------------------------------------------------

#[test]
fn cli_help_and_version() {
    let out = run_cli(&["--help"], &[], None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    let h = stdout(&out);
    for cmd in [
        "init", "doctor", "schema", "capture", "inspect", "render", "diff", "review", "accept",
        "report", "import", "session", "record", "trace", "machine",
    ] {
        assert!(h.contains(cmd), "help lists {cmd}:\n{h}");
    }
    let out = run_cli(&["--version"], &[], None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    assert!(stdout(&out).contains("tuisnap"));
    let out = run_cli(&["init", "--help"], &[], None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    let h = stdout(&out);
    assert!(
        h.contains("tui-snap.toml"),
        "init help documents config:\n{h}"
    );
    assert!(
        h.contains(".config/nextest.toml"),
        "init help documents nextest:\n{h}"
    );
    assert!(h.contains("insta"), "init help documents insta:\n{h}");
    assert!(proto::CONFIG_DOCS.contains("tui-snap.toml"));
}

#[test]
fn cli_schema_and_doctor() {
    let out = run_cli(&["schema"], &[], None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("schema is JSON");
    assert!(v.get("definitions").is_some());
    assert!(stdout(&out).contains("session-start"));
    let out = run_cli(&["doctor"], &[], None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    let d = stdout(&out);
    assert!(d.contains("toolchain"), "doctor covers toolchain:\n{d}");
    assert!(d.contains("font"), "doctor covers fonts:\n{d}");
    assert!(d.contains("profile"), "doctor covers profiles:\n{d}");
}

// ---------------------------------------------------------------------------
// CLI: init scaffolding
// ---------------------------------------------------------------------------

#[test]
fn cli_init_scaffolds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_string_lossy().into_owned();
    let out = run_cli(&["init", "--dir", &root], &[], None).expect("run tuisnap");
    assert_eq!(
        code(&out).expect("exit code"),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(tmp.path().join("tui-snap.toml").is_file());
    assert!(tmp.path().join(".config/nextest.toml").is_file());
    assert!(tmp.path().join("tests/visual.rs").is_file());
    let toml = std::fs::read_to_string(tmp.path().join("tui-snap.toml")).expect("read");
    assert!(toml.contains("[capture]"));
    // second init refuses without --force
    let out = run_cli(&["init", "--dir", &root], &[], None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), EXIT_OP_ERROR);
    let out = run_cli(&["init", "--dir", &root, "--force"], &[], None).expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
}

// ---------------------------------------------------------------------------
// CLI: capture (exit passthrough) + inspect (offline, never executes)
// ---------------------------------------------------------------------------

#[test]
fn cli_capture_passthrough() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ok = tmp.path().join("ok");
    let out = run_cli(
        &[
            "capture",
            "--out",
            ok.to_str().expect("utf8 path"),
            "--",
            "echo",
            "cap-hi",
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
    assert!(ok.join("manifest.json").is_file());
    assert!(ok.join("stdout.bin").is_file());
    let manifest = std::fs::read_to_string(ok.join("manifest.json")).expect("manifest");
    assert!(
        manifest.contains("cap-hi") || manifest.contains("Exit"),
        "{manifest}"
    );
    let fail = tmp.path().join("fail");
    let out = run_cli(
        &[
            "capture",
            "--out",
            fail.to_str().expect("utf8 path"),
            "--",
            "sh",
            "-c",
            "exit 7",
        ],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(
        code(&out).expect("exit code"),
        7,
        "child exit code preserved"
    );
}

#[test]
fn cli_inspect_never_executes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("art");
    std::fs::create_dir(&dir).expect("mkdir");
    // a trap executable: if inspect ever spawns it, the marker appears
    let marker = tmp.path().join("EXECUTED");
    let trap = dir.join("run-me.sh");
    std::fs::write(
        &trap,
        format!("#!/bin/sh\ntouch {}\n", marker.to_string_lossy()),
    )
    .expect("trap");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = std::fs::metadata(&trap).expect("stat").permissions();
        p.set_mode(0o755);
        std::fs::set_permissions(&trap, p).expect("chmod");
    }
    std::fs::write(dir.join("manifest.json"), r#"{"argv":["x"],"code":0}"#).expect("manifest");
    let out = run_cli(
        &["inspect", "--dir", dir.to_str().expect("utf8 path")],
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
    assert!(
        stdout(&out).contains("run-me.sh"),
        "lists files:\n{}",
        stdout(&out)
    );
    assert!(!marker.is_file(), "inspect must never execute artifacts");
}

// ---------------------------------------------------------------------------
// CLI: render / diff (offline)
// ---------------------------------------------------------------------------

#[test]
fn cli_render_and_diff() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let a_frame = tmp.path().join("a.frame.json");
    let b_frame = tmp.path().join("b.frame.json");
    std::fs::write(&a_frame, blank_frame_json(20, 5)).expect("frame a");
    std::fs::write(&b_frame, blank_frame_json(21, 5)).expect("frame b");
    let a_out = tmp.path().join("a");
    let out = run_cli(
        &[
            "render",
            "--input",
            a_frame.to_str().expect("utf8 path"),
            "--format",
            "txt",
            "--format",
            "png",
            "--out",
            a_out.to_str().expect("utf8 path"),
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
    assert!(tmp.path().join("a.txt").is_file());
    assert!(tmp.path().join("a.png").is_file());
    let b_out = tmp.path().join("b");
    let out = run_cli(
        &[
            "render",
            "--input",
            b_frame.to_str().expect("utf8 path"),
            "--format",
            "png",
            "--out",
            b_out.to_str().expect("utf8 path"),
        ],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0);
    // identical diff passes
    let out = run_cli(
        &[
            "diff",
            "--expected",
            tmp.path().join("a.png").to_str().expect("utf8 path"),
            "--actual",
            tmp.path().join("a.png").to_str().expect("utf8 path"),
        ],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), 0, "{}", stdout(&out));
    // differing diff fails with the verify-fail status
    let out = run_cli(
        &[
            "diff",
            "--expected",
            tmp.path().join("a.png").to_str().expect("utf8 path"),
            "--actual",
            tmp.path().join("b.png").to_str().expect("utf8 path"),
        ],
        &[],
        None,
    )
    .expect("run tuisnap");
    assert_eq!(code(&out).expect("exit code"), EXIT_VERIFY_FAIL);
}
