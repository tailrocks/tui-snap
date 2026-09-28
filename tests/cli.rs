//! CLI core + typed op protocol tests (A01, A02-partial, A04-partial).
//!
//! Covers [`tuisnap::proto::execute`] for every op (happy + error paths), the
//! `--machine` JSON-lines shape, every CLI subcommand round trip in temp dirs,
//! exit codes, and the inspect/import never-executes guarantee.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tuisnap::proto::{
    self, Capabilities, Envelope, Op, OpResult, SessionStatus, EXIT_OP_ERROR, EXIT_VERIFY_FAIL,
};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tuisnap"))
}

fn run_cli(args: &[&str], env_extra: &[(&str, &str)], stdin: Option<&str>) -> Output {
    let mut cmd = Command::new(bin());
    cmd.args(args);
    for (k, v) in env_extra {
        cmd.env(k, v);
    }
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn tuisnap");
    if let Some(input) = stdin {
        use std::io::Write;
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(input.as_bytes())
            .expect("write stdin");
    }
    child.wait_with_output().expect("wait tuisnap")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("exit code")
}

/// Serializes tests that mutate `TUISNAP_RUNTIME_DIR` process-wide.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_runtime_dir<T>(f: impl FnOnce(&Path) -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("rt");
    std::env::set_var("TUISNAP_RUNTIME_DIR", &dir);
    let out = f(&dir);
    std::env::remove_var("TUISNAP_RUNTIME_DIR");
    out
}

fn pty_available() -> bool {
    proto::capabilities().pty
}

fn blank_frame_json(cols: u16, rows: u16) -> String {
    let screen = tuisnap::Screen::blank(cols, rows);
    tuisnap::assert::frame_from_screen(&screen).to_json()
}

// ---------------------------------------------------------------------------
// execute(): version / capabilities / assert
// ---------------------------------------------------------------------------

#[test]
fn op_version_and_capabilities() {
    match proto::execute(&Op::Version).expect("version") {
        OpResult::Version { protocol, tuisnap } => {
            assert_eq!(protocol, proto::PROTOCOL_VERSION);
            assert!(!tuisnap.is_empty());
        }
        r => panic!("wrong result: {r:?}"),
    }
    match proto::execute(&Op::Capabilities).expect("capabilities") {
        OpResult::Capabilities { capabilities } => {
            let c: Capabilities = capabilities;
            assert_eq!(c.protocol, proto::PROTOCOL_VERSION);
            assert!(c.render && c.record);
        }
        r => panic!("wrong result: {r:?}"),
    }
}

#[test]
fn op_assert_paths() {
    let mk = |check: &str,
              text: Option<&str>,
              needle: Option<&str>,
              actual: Option<&str>,
              expected: Option<&str>| {
        Op::Assert {
            check: check.to_string(),
            text: text.map(str::to_string),
            needle: needle.map(str::to_string),
            actual: actual.map(str::to_string),
            expected: expected.map(str::to_string),
        }
    };
    let passed = |r: OpResult| match r {
        OpResult::Asserted { passed, .. } => passed,
        r => panic!("wrong result: {r:?}"),
    };
    assert!(passed(
        proto::execute(&mk("text-contains", Some("hello"), Some("ell"), None, None)).expect("ok")
    ));
    assert!(!passed(
        proto::execute(&mk("text-contains", Some("hello"), Some("zzz"), None, None)).expect("ok")
    ));
    assert!(passed(
        proto::execute(&mk("text-equals", None, None, Some("a"), Some("a"))).expect("ok")
    ));
    assert!(!passed(
        proto::execute(&mk("text-equals", None, None, Some("a"), Some("b"))).expect("ok")
    ));
    // error paths
    let e = proto::execute(&mk("text-contains", Some("x"), Some(""), None, None)).unwrap_err();
    assert_eq!(e.code, "invalid-input");
    let e = proto::execute(&mk("text-contains", None, Some("x"), None, None)).unwrap_err();
    assert_eq!(e.code, "invalid-input");
    let e = proto::execute(&mk("nope", None, None, None, None)).unwrap_err();
    assert_eq!(e.code, "invalid-input");
}

// ---------------------------------------------------------------------------
// execute(): render / diff
// ---------------------------------------------------------------------------

#[test]
fn op_render_all_formats() {
    let frame = blank_frame_json(20, 5);
    for (fmt, b64) in [
        ("txt", false),
        ("ansi", false),
        ("svg", false),
        ("html", false),
        ("png", true),
    ] {
        let r = proto::execute(&Op::Render {
            frame_json: frame.clone(),
            format: fmt.to_string(),
        })
        .expect(fmt);
        match r {
            OpResult::Rendered {
                format,
                data,
                data_b64,
            } => {
                assert_eq!(format, fmt);
                assert_eq!(data_b64, b64);
                assert!(!data.is_empty());
            }
            r => panic!("wrong result: {r:?}"),
        }
    }
    let e = proto::execute(&Op::Render {
        frame_json: frame.clone(),
        format: "mp4".to_string(),
    })
    .unwrap_err();
    assert_eq!(e.code, "invalid-input");
    let e = proto::execute(&Op::Render {
        frame_json: "not json".to_string(),
        format: "txt".to_string(),
    })
    .unwrap_err();
    assert_eq!(e.code, "invalid-input");
}

#[test]
fn op_diff_equal_and_unequal() {
    let png_of = |cols: u16| match proto::execute(&Op::Render {
        frame_json: blank_frame_json(cols, 5),
        format: "png".to_string(),
    })
    .expect("render")
    {
        OpResult::Rendered { data, .. } => data,
        r => panic!("wrong result: {r:?}"),
    };
    let a = png_of(20);
    let b = png_of(21);
    let diff = |e: &str, act: &str| {
        proto::execute(&Op::Diff {
            expected_png_b64: e.to_string(),
            actual_png_b64: act.to_string(),
        })
    };
    match diff(&a, &a).expect("same") {
        OpResult::Diffed {
            pixels_equal,
            dims_equal,
            score,
        } => {
            assert!(pixels_equal && dims_equal);
            assert_eq!(score, 1.0);
        }
        r => panic!("wrong result: {r:?}"),
    }
    match diff(&a, &b).expect("different") {
        OpResult::Diffed { pixels_equal, .. } => assert!(!pixels_equal),
        r => panic!("wrong result: {r:?}"),
    }
    let e = diff("!!!bad!!!", &a).unwrap_err();
    assert_eq!(e.code, "invalid-input");
}

// ---------------------------------------------------------------------------
// execute(): PTY ops via the registry (skipped without the feature)
// ---------------------------------------------------------------------------

#[test]
fn op_pty_lifecycle() {
    if !pty_available() {
        return;
    }
    let id = format!("cli-test-{}", std::process::id());
    let session = match proto::execute(&Op::Spawn {
        argv: vec!["echo".to_string(), "hi-pty".to_string()],
        id: Some(id.clone()),
        cols: None,
        rows: None,
        cwd: None,
        env: Default::default(),
    })
    .expect("spawn")
    {
        OpResult::Spawned { session, .. } => session,
        r => panic!("wrong result: {r:?}"),
    };
    assert_eq!(session, id);
    // wait for exit, then observe evidence
    match proto::execute(&Op::Wait {
        session: session.clone(),
        kind: "exit".to_string(),
        needle: None,
        quiet_ms: None,
        timeout_ms: 10_000,
    })
    .expect("wait exit")
    {
        OpResult::Exited { code, .. } => assert_eq!(code, 0),
        r => panic!("wrong result: {r:?}"),
    }
    match proto::execute(&Op::Observe {
        session: session.clone(),
    })
    .expect("observe")
    {
        OpResult::Observation { observation } => {
            assert!(
                observation.screen.text.contains("hi-pty"),
                "{}",
                observation.screen.text
            )
        }
        r => panic!("wrong result: {r:?}"),
    }
    match proto::execute(&Op::Snapshot {
        session: session.clone(),
    })
    .expect("snapshot")
    {
        OpResult::Snapshot { screen } => assert!(screen.text.contains("hi-pty")),
        r => panic!("wrong result: {r:?}"),
    }
    match proto::execute(&Op::Screenshot {
        session: session.clone(),
    })
    .expect("screenshot")
    {
        OpResult::Screenshot {
            canonical, png_b64, ..
        } => {
            assert!(canonical.contains("tuisnap screen snapshot"));
            assert!(!png_b64.is_empty());
        }
        r => panic!("wrong result: {r:?}"),
    }
    match proto::execute(&Op::Exit {
        session: session.clone(),
        timeout_ms: 5_000,
    })
    .expect("exit")
    {
        OpResult::Exited { code, .. } => assert_eq!(code, 0),
        r => panic!("wrong result: {r:?}"),
    }
    // session is gone now
    let e = proto::execute(&Op::Observe { session }).unwrap_err();
    assert_eq!(e.code, "not-found");
}

#[test]
fn op_pty_error_paths() {
    if !pty_available() {
        return;
    }
    let e = proto::execute(&Op::Spawn {
        argv: vec![],
        id: None,
        cols: None,
        rows: None,
        cwd: None,
        env: Default::default(),
    })
    .unwrap_err();
    assert_eq!(e.code, "invalid-input");
    let e = proto::execute(&Op::Spawn {
        argv: vec!["/nonexistent-binary-xyz".to_string()],
        id: None,
        cols: None,
        rows: None,
        cwd: None,
        env: Default::default(),
    })
    .unwrap_err();
    assert_eq!(e.code, "spawn-failed");
    let e = proto::execute(&Op::Stdin {
        session: "no-such".to_string(),
        text: Some("x".to_string()),
        chord: None,
        bytes_b64: None,
    })
    .unwrap_err();
    assert_eq!(e.code, "not-found");
    // spawn a live session for input-validation errors
    let session = match proto::execute(&Op::Spawn {
        argv: vec!["sleep".to_string(), "30".to_string()],
        id: None,
        cols: None,
        rows: None,
        cwd: None,
        env: Default::default(),
    })
    .expect("spawn sleep")
    {
        OpResult::Spawned { session, .. } => session,
        r => panic!("wrong result: {r:?}"),
    };
    let e = proto::execute(&Op::Stdin {
        session: session.clone(),
        text: Some("a".to_string()),
        chord: Some("Enter".to_string()),
        bytes_b64: None,
    })
    .unwrap_err();
    assert_eq!(e.code, "invalid-input");
    let e = proto::execute(&Op::Wait {
        session: session.clone(),
        kind: "bogus".to_string(),
        needle: None,
        quiet_ms: None,
        timeout_ms: 100,
    })
    .unwrap_err();
    assert_eq!(e.code, "invalid-input");
    let e = proto::execute(&Op::Wait {
        session: session.clone(),
        kind: "text".to_string(),
        needle: Some("never-appears-xyz".to_string()),
        quiet_ms: None,
        timeout_ms: 100,
    })
    .unwrap_err();
    assert_eq!(e.code, "timeout");
    // exit on a running child times out but still tears the session down
    let e = proto::execute(&Op::Exit {
        session: session.clone(),
        timeout_ms: 100,
    })
    .unwrap_err();
    assert_eq!(e.code, "timeout");
    let e = proto::execute(&Op::Observe { session }).unwrap_err();
    assert_eq!(e.code, "not-found");
}

// ---------------------------------------------------------------------------
// execute(): named sessions
// ---------------------------------------------------------------------------

#[test]
fn op_named_session_round_trip() {
    with_runtime_dir(|dir| {
        let info = proto::session_start("rt1", &["sleep".to_string(), "30".to_string()], false)
            .expect("start");
        assert_eq!(info.name, "rt1");
        assert!(dir.join("rt1.json").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir).expect("stat").permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "runtime dir must be owner-only");
        }
        // collision without force
        let e = proto::session_start("rt1", &["sleep".to_string(), "1".to_string()], false)
            .unwrap_err();
        assert_eq!(e.code, "session-exists");
        let list = proto::session_list().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].status, SessionStatus::Running);
        // force replaces
        let info2 = proto::session_start("rt1", &["sleep".to_string(), "30".to_string()], true)
            .expect("force start");
        assert_ne!(info.pid, info2.pid);
        proto::session_stop("rt1").expect("stop");
        assert!(!dir.join("rt1.json").is_file());
        let e = proto::session_stop("rt1").unwrap_err();
        assert_eq!(e.code, "not-found");
        // bad names rejected
        for bad in ["", "../evil", "a/b", &"x".repeat(65)] {
            let e = proto::session_start(bad, &["sleep".to_string()], false).unwrap_err();
            assert_eq!(e.code, "invalid-input", "{bad:?}");
        }
        // prune removes dead endpoints
        proto::session_start("short", &["true".to_string()], false).expect("start short");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let pruned = proto::session_prune().expect("prune");
        assert!(pruned.contains(&"short".to_string()), "{pruned:?}");
    });
}

#[test]
fn op_session_ops_via_execute() {
    with_runtime_dir(|_| {
        match proto::execute(&Op::SessionStart {
            name: "ex1".to_string(),
            argv: vec!["sleep".to_string(), "30".to_string()],
            force: false,
        })
        .expect("start")
        {
            OpResult::Session { session } => assert_eq!(session.name, "ex1"),
            r => panic!("wrong result: {r:?}"),
        }
        match proto::execute(&Op::SessionList).expect("list") {
            OpResult::SessionList { sessions } => {
                assert!(sessions.iter().any(|s| s.name == "ex1"))
            }
            r => panic!("wrong result: {r:?}"),
        }
        match proto::execute(&Op::SessionStop {
            name: "ex1".to_string(),
        })
        .expect("stop")
        {
            OpResult::Session { session } => assert_eq!(session.name, "ex1"),
            r => panic!("wrong result: {r:?}"),
        }
    });
}

// ---------------------------------------------------------------------------
// machine envelope shape
// ---------------------------------------------------------------------------

#[test]
fn machine_line_shapes() {
    let (line, ok) = proto::run_machine_line(r#"{"type":"version"}"#);
    assert!(ok);
    let env: Envelope = serde_json::from_str(&line).expect("envelope json");
    assert!(env.ok && env.result.is_some() && env.error.is_none());
    let (line, ok) = proto::run_machine_line("this is not json");
    assert!(!ok);
    let env: Envelope = serde_json::from_str(&line).expect("envelope json");
    assert!(!env.ok);
    assert_eq!(env.error.expect("error").code, "invalid-input");
    let (line, ok) = proto::run_machine_line(r#"{"type":"assert","check":"bogus"}"#);
    assert!(!ok);
    let env: Envelope = serde_json::from_str(&line).expect("envelope json");
    assert_eq!(env.error.expect("error").code, "invalid-input");
    // every line is self-delimiting single-line JSON
    assert!(!line.contains('\n'));
}

// ---------------------------------------------------------------------------
// CLI: help / version / schema / doctor
// ---------------------------------------------------------------------------

#[test]
fn cli_help_and_version() {
    let out = run_cli(&["--help"], &[], None);
    assert_eq!(code(&out), 0);
    let h = stdout(&out);
    for cmd in [
        "init", "doctor", "schema", "capture", "inspect", "render", "diff", "review", "report",
        "import", "session", "record", "trace",
    ] {
        assert!(h.contains(cmd), "help lists {cmd}:\n{h}");
    }
    let out = run_cli(&["--version"], &[], None);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("tuisnap"));
    let out = run_cli(&["init", "--help"], &[], None);
    assert_eq!(code(&out), 0);
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
    let out = run_cli(&["schema"], &[], None);
    assert_eq!(code(&out), 0);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("schema is JSON");
    assert!(v.get("definitions").is_some());
    assert!(stdout(&out).contains("session-start"));
    let out = run_cli(&["doctor"], &[], None);
    assert_eq!(code(&out), 0);
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
    let out = run_cli(&["init", "--dir", &root], &[], None);
    assert_eq!(
        code(&out),
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
    let out = run_cli(&["init", "--dir", &root], &[], None);
    assert_eq!(code(&out), EXIT_OP_ERROR);
    let out = run_cli(&["init", "--dir", &root, "--force"], &[], None);
    assert_eq!(code(&out), 0);
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
            ok.to_str().unwrap(),
            "--",
            "echo",
            "cap-hi",
        ],
        &[],
        None,
    );
    assert_eq!(
        code(&out),
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
            fail.to_str().unwrap(),
            "--",
            "sh",
            "-c",
            "exit 7",
        ],
        &[],
        None,
    );
    assert_eq!(code(&out), 7, "child exit code preserved");
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
    let out = run_cli(&["inspect", "--dir", dir.to_str().unwrap()], &[], None);
    assert_eq!(
        code(&out),
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
            a_frame.to_str().unwrap(),
            "--format",
            "txt",
            "--format",
            "png",
            "--out",
            a_out.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(
        code(&out),
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
            b_frame.to_str().unwrap(),
            "--format",
            "png",
            "--out",
            b_out.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(code(&out), 0);
    // identical diff passes
    let out = run_cli(
        &[
            "diff",
            "--expected",
            tmp.path().join("a.png").to_str().unwrap(),
            "--actual",
            tmp.path().join("a.png").to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(code(&out), 0, "{}", stdout(&out));
    // differing diff fails with the verify-fail status
    let out = run_cli(
        &[
            "diff",
            "--expected",
            tmp.path().join("a.png").to_str().unwrap(),
            "--actual",
            tmp.path().join("b.png").to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(code(&out), EXIT_VERIFY_FAIL);
}

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
        &["review", "--dir", tmp.path().to_str().unwrap()],
        &[],
        None,
    );
    assert_eq!(code(&out), EXIT_VERIFY_FAIL);
    assert!(stdout(&out).contains("two"), "{}", stdout(&out));
    let html = tmp.path().join("report.html");
    let out = run_cli(
        &[
            "report",
            "--dir",
            tmp.path().to_str().unwrap(),
            "--out",
            html.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(
        code(&out),
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
        &["import", "--dir", tmp.path().to_str().unwrap()],
        &[],
        None,
    );
    assert_eq!(
        code(&out),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout(&out).contains("0"), "{}", stdout(&out));
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
    let out = run_cli(&["session", "stop", "--name", "ghost"], env, None);
    assert_eq!(code(&out), EXIT_OP_ERROR);
    let out = run_cli(
        &["session", "start", "--name", "s1", "--", "sleep", "30"],
        env,
        None,
    );
    assert_eq!(
        code(&out),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run_cli(&["session", "list"], env, None);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("s1"), "{}", stdout(&out));
    let out = run_cli(
        &["session", "start", "--name", "s1", "--", "sleep", "1"],
        env,
        None,
    );
    assert_eq!(code(&out), EXIT_OP_ERROR, "collision without --force");
    let out = run_cli(
        &[
            "session", "start", "--name", "s1", "--force", "--", "sleep", "30",
        ],
        env,
        None,
    );
    assert_eq!(code(&out), 0);
    let out = run_cli(&["session", "stop", "--name", "s1"], env, None);
    assert_eq!(code(&out), 0);
    let out = run_cli(&["session", "list"], env, None);
    assert_eq!(code(&out), 0);
    assert!(!stdout(&out).contains("s1"), "{}", stdout(&out));
    let out = run_cli(&["session", "prune"], env, None);
    assert_eq!(code(&out), 0);
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
            journal.to_str().unwrap(),
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
    );
    assert_eq!(
        code(&out),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run_cli(&["trace", "--input", journal.to_str().unwrap()], &[], None);
    assert_eq!(code(&out), 0);
    let t = stdout(&out);
    assert!(t.contains("start") && t.contains("exit"), "{t}");
    let out = run_cli(
        &[
            "trace",
            "--input",
            journal.to_str().unwrap(),
            "--kind",
            "exit",
        ],
        &[],
        None,
    );
    assert_eq!(code(&out), 0);
    assert!(!stdout(&out).contains("start"), "kind filter applies");
    // tiny bound trips the recorder instead of truncating silently
    let small = tmp.path().join("small.jsonl");
    let out = run_cli(
        &[
            "record",
            "--out",
            small.to_str().unwrap(),
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
    );
    assert_eq!(code(&out), EXIT_OP_ERROR);
}

// ---------------------------------------------------------------------------
// CLI: machine mode over stdio
// ---------------------------------------------------------------------------

#[test]
fn cli_machine_mode() {
    let input = "{\"type\":\"version\"}\n{\"type\":\"assert\",\"check\":\"text-equals\",\"actual\":\"a\",\"expected\":\"a\"}\n";
    let out = run_cli(&["--machine"], &[], Some(input));
    assert_eq!(
        code(&out),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let env: Envelope = serde_json::from_str(line).expect("envelope json");
        assert!(env.ok);
    }
    let out = run_cli(
        &["--machine"],
        &[],
        Some("{\"type\":\"version\"}\ngarbage\n"),
    );
    assert_eq!(code(&out), EXIT_OP_ERROR);
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    let second: Envelope = serde_json::from_str(lines[1]).expect("envelope");
    assert!(!second.ok);
}
