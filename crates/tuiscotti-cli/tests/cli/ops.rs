//! execute(): version, assert, render, diff, PTY ops (split from `cli.rs`; shared helpers live in the root).

use super::{blank_frame_json, pty_available};
use tuiscotti::proto::{self, Capabilities, Op, OpResult};

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
