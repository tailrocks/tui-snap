//! `execute()`: named sessions + machine envelope shape (split from `cli.rs`; shared helpers live in the root).

use super::{code, run_cli, stdout, with_runtime_dir};
use tuiscotti::proto::{self, EXIT_OP_ERROR, Envelope, Op, OpResult, SessionStatus};

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

// ---------------------------------------------------------------------------
// F08: endpoint records are untrusted metadata
// ---------------------------------------------------------------------------

#[test]
fn op_session_dotted_names_consistent() {
    with_runtime_dir(|dir| {
        for name in ["a.b", "x.y.z"] {
            proto::session_start(name, &["sleep".to_string(), "30".to_string()], false)
                .expect("start dotted");
            assert!(dir.join(format!("{name}.json")).is_file());
        }
        let list = proto::session_list().expect("list");
        assert!(list.iter().any(|s| s.name == "a.b"), "{list:?}");
        assert!(list.iter().any(|s| s.name == "x.y.z"), "{list:?}");
        proto::session_start("d.e", &["true".to_string()], false).expect("start short");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let pruned = proto::session_prune().expect("prune");
        assert!(pruned.contains(&"d.e".to_string()), "{pruned:?}");
        for name in ["a.b", "x.y.z"] {
            proto::session_stop(name).expect("stop dotted");
        }
        assert!(proto::session_list().expect("list").is_empty());
    })
    .expect("isolated runtime dir");
}

#[cfg(unix)]
#[test]
fn op_session_tampered_owner_rejected() {
    with_runtime_dir(|dir| {
        let info = proto::session_start("t1", &["sleep".to_string(), "30".to_string()], false)
            .expect("start");
        let path = dir.join("t1.json");
        let raw = std::fs::read(&path).expect("read endpoint");
        let mut val: serde_json::Value = serde_json::from_slice(&raw).expect("json");
        let owner = val["owner"].as_u64().expect("owner field");
        val["owner"] = serde_json::json!(owner ^ 1);
        std::fs::write(&path, serde_json::to_vec(&val).expect("json")).expect("tamper");
        let e = proto::session_list().expect_err("tampered owner listed");
        assert_eq!(e.code, "owner-mismatch");
        let e = proto::session_stop("t1").expect_err("tampered owner stopped");
        assert_eq!(e.code, "owner-mismatch");
        assert!(path.is_file(), "failed stop preserves the endpoint");
        std::fs::write(&path, raw).expect("restore");
        let list = proto::session_list().expect("list");
        assert_eq!(list[0].status, SessionStatus::Running);
        assert_eq!(list[0].pid, info.pid);
        proto::session_stop("t1").expect("stop");
    })
    .expect("isolated runtime dir");
}

#[test]
fn op_session_tampered_name_rejected_no_escape() {
    with_runtime_dir(|dir| {
        proto::session_list().expect("create runtime dir");
        let sentinel = dir.join("sentinel.keep");
        std::fs::write(&sentinel, b"keep").expect("sentinel");
        std::fs::write(
            dir.join("a.json"),
            r#"{"version":1,"name":"b","pid":1,"argv":["x"],"backend":"process","started_unix":1,"owner":0}"#,
        )
        .expect("seed");
        let e = proto::session_list().expect_err("mismatched name listed");
        assert_eq!(e.code, "invalid-input");
        let e = proto::session_stop("a").expect_err("mismatched name stopped");
        assert_eq!(e.code, "invalid-input");
        assert!(dir.join("a.json").is_file(), "endpoint preserved");
        std::fs::write(
            dir.join("e.json"),
            r#"{"version":1,"name":"../../evil","pid":1,"argv":["x"],"backend":"process","started_unix":1,"owner":0}"#,
        )
        .expect("seed");
        let e = proto::session_prune().expect_err("escape name pruned");
        assert_eq!(e.code, "invalid-input");
        assert!(sentinel.is_file(), "nothing outside was deleted");
        assert!(!dir.parent().expect("parent").join("evil").exists());
    })
    .expect("isolated runtime dir");
}

#[test]
fn op_session_pid_zero_rejected_without_signaling() {
    with_runtime_dir(|dir| {
        proto::session_list().expect("create runtime dir");
        std::fs::write(
            dir.join("z.json"),
            r#"{"version":1,"name":"z","pid":0,"argv":["x"],"backend":"process","started_unix":1,"owner":0}"#,
        )
        .expect("seed");
        let before = std::fs::read(dir.join("z.json")).expect("read");
        let e = proto::session_list().expect_err("pid 0 listed");
        assert_eq!(e.code, "invalid-input");
        let e = proto::session_stop("z").expect_err("pid 0 stopped");
        assert_eq!(e.code, "invalid-input");
        assert_eq!(std::fs::read(dir.join("z.json")).expect("read"), before);
    })
    .expect("isolated runtime dir");
}

#[test]
fn op_session_symlink_and_dir_entries_rejected() {
    with_runtime_dir(|dir| {
        proto::session_list().expect("create runtime dir");
        #[cfg(unix)]
        {
            let target = dir.join("target.dat");
            std::fs::write(&target, b"{}").expect("seed");
            std::os::unix::fs::symlink(&target, dir.join("s.json")).expect("symlink");
            let list = proto::session_list().expect("list");
            assert!(!list.iter().any(|s| s.name == "s"), "{list:?}");
            let e = proto::session_stop("s").expect_err("symlink followed");
            assert_eq!(e.code, "invalid-input");
            assert!(dir.join("s.json").is_symlink(), "link preserved");
        }
        std::fs::create_dir(dir.join("d.json")).expect("mkdir");
        let list = proto::session_list().expect("list");
        assert!(!list.iter().any(|s| s.name == "d"), "{list:?}");
        let e = proto::session_stop("d").expect_err("dir read");
        assert_eq!(e.code, "invalid-input");
        assert!(dir.join("d.json").is_dir(), "dir preserved");
    })
    .expect("isolated runtime dir");
}

#[cfg(unix)]
#[test]
fn op_session_runtime_dir_symlink_rejected() {
    with_runtime_dir(|dir| {
        let real = dir.join("real");
        std::fs::create_dir_all(&real).expect("mkdir");
        let link = dir.join("rt-link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        proto::set_runtime_dir_override(Some(link));
        let e = proto::session_list().expect_err("symlink runtime dir used");
        assert_eq!(e.code, "invalid-input");
    })
    .expect("isolated runtime dir");
}

#[cfg(unix)]
#[test]
fn op_session_log_symlink_refuses_start() {
    with_runtime_dir(|dir| {
        proto::session_list().expect("create runtime dir");
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"untouched").expect("seed");
        std::os::unix::fs::symlink(&victim, dir.join("l.log")).expect("symlink");
        let e = proto::session_start("l", &["sleep".to_string(), "30".to_string()], false)
            .expect_err("log symlink accepted");
        assert_eq!(e.code, "invalid-input");
        assert!(!dir.join("l.json").exists(), "no endpoint published");
        assert_eq!(std::fs::read(&victim).expect("read"), b"untouched");
        assert!(proto::session_list().expect("list").is_empty());
    })
    .expect("isolated runtime dir");
}

#[test]
fn op_session_concurrent_same_name_single_winner() {
    with_runtime_dir(|_| {
        let mut handles = Vec::new();
        for _ in 0..8 {
            handles.push(std::thread::spawn(|| {
                proto::session_start("race", &["sleep".to_string(), "30".to_string()], false)
            }));
        }
        let mut pids = Vec::new();
        for h in handles {
            match h.join().expect("thread") {
                Ok(info) => pids.push(info.pid),
                Err(e) => assert_eq!(e.code, "session-exists", "{e}"),
            }
        }
        assert_eq!(pids.len(), 1, "exactly one winner");
        let list = proto::session_list().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].pid, pids[0]);
        proto::session_stop("race").expect("stop");
        assert!(proto::session_list().expect("list").is_empty());
    })
    .expect("isolated runtime dir");
}

#[test]
fn op_session_failed_stop_preserves_state() {
    with_runtime_dir(|dir| {
        proto::session_list().expect("create runtime dir");
        std::fs::write(
            dir.join("pz.json"),
            r#"{"version":1,"name":"pz","pid":0,"argv":["x"],"backend":"process","started_unix":1,"owner":0}"#,
        )
        .expect("seed");
        assert!(proto::session_stop("pz").is_err());
        assert!(dir.join("pz.json").is_file(), "endpoint preserved");
        std::fs::create_dir(dir.join("dz.json")).expect("mkdir");
        assert!(proto::session_stop("dz").is_err());
        assert!(dir.join("dz.json").is_dir(), "dir preserved");
        proto::session_start("gone", &["true".to_string()], false).expect("start");
        std::thread::sleep(std::time::Duration::from_millis(500));
        proto::session_stop("gone").expect("stop dead");
        let e = proto::session_stop("gone").expect_err("double stop");
        assert_eq!(e.code, "not-found");
    })
    .expect("isolated runtime dir");
}

#[test]
fn op_session_setup_failure_spawns_nothing() {
    with_runtime_dir(|dir| {
        std::fs::create_dir_all(dir).expect("mkdir rt");
        std::fs::create_dir(dir.join("b.log")).expect("mkdir");
        // The log path is blocked by a directory: start must fail before
        // any spawn, publishing nothing.
        let e = proto::session_start("b", &["sleep".to_string(), "30".to_string()], false)
            .expect_err("blocked log accepted");
        assert_eq!(e.code, "io");
        assert!(!dir.join("b.json").exists());
        assert!(proto::session_list().expect("list").is_empty());
        let file = dir.join("f");
        std::fs::write(&file, b"x").expect("seed");
        proto::set_runtime_dir_override(Some(file.join("rt")));
        assert!(proto::session_start("c", &["sleep".to_string()], false).is_err());
    })
    .expect("isolated runtime dir");
}

#[test]
fn op_session_stop_reports_exited() {
    with_runtime_dir(|_| {
        proto::session_start("st1", &["sleep".to_string(), "30".to_string()], false)
            .expect("start");
        let info = proto::session_stop("st1").expect("stop");
        assert_eq!(info.status, SessionStatus::Exited);
        assert_eq!(info.name, "st1");
    })
    .expect("isolated runtime dir");
}

// ---------------------------------------------------------------------------
// F08: retained sessions across separate CLI processes (piped owner)
// ---------------------------------------------------------------------------

#[test]
fn cli_session_cross_process_dotted_and_hardened() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let rt = tmp.path().join("rt").to_string_lossy().into_owned();
    let env = &[("TUISCOTTI_RUNTIME_DIR", rt.as_str())][..];
    let out = run_cli(
        &["session", "start", "--name", "cli.dot", "--", "sleep", "30"],
        env,
        None,
    )
    .expect("run tuiscotti");
    assert_eq!(code(&out).expect("exit code"), 0, "{}", stdout(&out));
    let out = run_cli(&["session", "list"], env, None).expect("run tuiscotti");
    assert_eq!(code(&out).expect("exit code"), 0);
    assert!(stdout(&out).contains("cli.dot"), "{}", stdout(&out));
    // hostile attach name: rejected, exit 3, nothing outside touched.
    let out =
        run_cli(&["session", "attach", "--name", "../evil"], env, None).expect("run tuiscotti");
    assert_eq!(code(&out).expect("exit code"), EXIT_OP_ERROR);
    let out = run_cli(&["session", "stop", "--name", "cli.dot"], env, None).expect("run tuiscotti");
    assert_eq!(code(&out).expect("exit code"), 0, "{}", stdout(&out));
    assert!(
        stdout(&out).contains("stopped: cli.dot"),
        "{}",
        stdout(&out)
    );
    let out = run_cli(
        &["session", "start", "--name", "short.lived", "--", "true"],
        env,
        None,
    )
    .expect("run tuiscotti");
    assert_eq!(code(&out).expect("exit code"), 0);
    std::thread::sleep(std::time::Duration::from_millis(500));
    let out = run_cli(&["session", "prune"], env, None).expect("run tuiscotti");
    assert_eq!(code(&out).expect("exit code"), 0);
    assert!(stdout(&out).contains("short.lived"), "{}", stdout(&out));
}
