//! `execute()`: named sessions + machine envelope shape (split from `cli.rs`; shared helpers live in the root).

use super::with_runtime_dir;
use tuiscotti::proto::{self, Envelope, Op, OpResult, SessionStatus};

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
            .expect_err("session collision must fail");
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
        let e = proto::session_stop("rt1").expect_err("double stop must be not-found");
        assert_eq!(e.code, "not-found");
        // bad names rejected
        for bad in ["", "../evil", "a/b", &"x".repeat(65)] {
            let e = proto::session_start(bad, &["sleep".to_string()], false)
                .expect_err("bad session name must be rejected");
            assert_eq!(e.code, "invalid-input", "{bad:?}");
        }
        // prune removes dead endpoints
        proto::session_start("short", &["true".to_string()], false).expect("start short");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let pruned = proto::session_prune().expect("prune");
        assert!(pruned.contains(&"short".to_string()), "{pruned:?}");
    })
    .expect("isolated runtime dir");
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
                assert!(sessions.iter().any(|s| s.name == "ex1"));
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
    })
    .expect("isolated runtime dir");
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
