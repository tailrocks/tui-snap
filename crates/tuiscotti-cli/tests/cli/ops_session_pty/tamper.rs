//! Tampered metadata, symlinks, path escape.

use std::path::{Path, PathBuf};

use super::super::{run_cli, spawn_locked, stdout};
use super::helpers::{TRes, check_code, fresh_rt, pty_env, started_pid, teardown};
use tuiscotti::proto::EXIT_OP_ERROR;

/// Every F08 read op against `name` must fail while the record is
/// tampered, and the record must survive each failure.
fn check_tampered_fails(env: &[(&str, &str)], name: &str, endpoint: &Path) -> TRes<()> {
    for args in [
        vec!["session", "list"],
        vec!["session", "stop", "--name", name],
        vec!["session", "input", "--name", name, "--text", "x"],
        vec!["session", "observe", "--name", name],
    ] {
        let out = run_cli(&args, env, None).map_err(|e| format!("tampered {args:?}: {e}"))?;
        check_code(&out, EXIT_OP_ERROR).map_err(|e| format!("tampered {args:?}: {e}"))?;
    }
    if endpoint.is_file() {
        Ok(())
    } else {
        Err("failed ops must preserve the endpoint".to_string())
    }
}

#[test]
fn pty_tampered_metadata_rejected() {
    let (_tmp, rt) = fresh_rt("tamper").expect("tempdir");
    let env = pty_env(&rt);
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "t1", "--", "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, 0).expect("start");
    let child = started_pid(&out).expect("started pid");
    let endpoint = PathBuf::from(&rt).join("t1.json");
    let raw = std::fs::read(&endpoint).expect("read endpoint");
    let base: serde_json::Value = serde_json::from_slice(&raw).expect("json");
    // Flip each identity field in turn (daemon_pid: dropped, zeroed, and
    // pointed at a live foreign pid — our own test process).
    let me = u64::from(std::process::id());
    let owner = base["owner"].as_u64().expect("owner");
    // A deterministically dead, in-range pid: spawn `true`, reap it, use
    // its pid at once (reuse inside the next milliseconds is infeasible
    // — pids cycle sequentially, and nothing forks here).
    let mut true_cmd = std::process::Command::new("true");
    let mut dead_child = spawn_locked(&mut true_cmd).expect("spawn true");
    let dead_pid = dead_child.id();
    dead_child.wait().expect("reap true");
    let flips: Vec<(&str, serde_json::Value)> = vec![
        ("owner", serde_json::json!({"owner": owner ^ 1})),
        ("pid", serde_json::json!({"pid": 0})),
        ("name", serde_json::json!({"name": "other"})),
        ("version", serde_json::json!({"version": 999})),
        ("argv", serde_json::json!({"argv": []})),
        ("started", serde_json::json!({"started_unix": 0})),
        ("daemon-missing", serde_json::json!({"backend": "pty"})),
        ("daemon-zero", serde_json::json!({"daemon_pid": 0})),
        ("daemon-foreign", serde_json::json!({"daemon_pid": me})),
        (
            "daemon-huge",
            serde_json::json!({"daemon_pid": u64::from(u32::MAX)}),
        ),
        ("daemon-dead", serde_json::json!({"daemon_pid": dead_pid})),
    ];
    for (what, patch) in flips {
        let mut val = base.clone();
        if what == "daemon-missing" {
            val.as_object_mut().expect("obj").remove("daemon_pid");
        } else {
            for (k, v) in patch.as_object().expect("patch") {
                val[k] = v.clone();
            }
        }
        // `daemon-dead` names a dead pid: the session reads orphaned
        // (Exited), which list shows honestly instead of failing.
        // (`daemon-huge` is out of the pid_t range: corrupt, like zero.)
        if what == "daemon-dead" {
            std::fs::write(&endpoint, serde_json::to_vec(&val).expect("json")).expect("tamper");
            let out = run_cli(&["session", "list"], &env, None).expect("list");
            check_code(&out, 0).expect("dead owner lists Exited");
            assert!(
                stdout(&out).contains("t1") && stdout(&out).contains("Exited"),
                "{}",
                stdout(&out)
            );
        } else {
            std::fs::write(&endpoint, serde_json::to_vec(&val).expect("json")).expect("tamper");
            check_tampered_fails(&env, "t1", &endpoint).expect("tamper");
            if what == "daemon-foreign" {
                let out = run_cli(&["session", "stop", "--name", "t1"], &env, None).expect("stop");
                assert!(
                    String::from_utf8_lossy(&out.stderr).contains("op-failed"),
                    "live foreign owner fails closed: {:?}",
                    out.stderr
                );
            }
        }
        std::fs::write(&endpoint, &raw).expect("restore");
    }
    // Restored: everything works again through the same daemon.
    let out = run_cli(&["session", "observe", "--name", "t1"], &env, None).expect("observe");
    check_code(&out, 0).expect("observe after restore");
    let out = run_cli(&["session", "stop", "--name", "t1"], &env, None).expect("stop");
    check_code(&out, 0).expect("stop after restore");
    teardown(Path::new(&rt), &[child], true).expect("teardown");
}

#[test]
fn pty_symlinks_refused() {
    let (_tmp, rt) = fresh_rt("links").expect("tempdir");
    let env = pty_env(&rt);
    let rt_path = PathBuf::from(&rt);
    std::fs::create_dir_all(&rt_path).expect("mkdir rt");
    #[cfg(unix)]
    {
        // A symlinked socket path refuses autostart outright.
        let target = rt_path.join("sock-target");
        std::fs::write(&target, b"{}").expect("seed");
        std::os::unix::fs::symlink(&target, rt_path.join("daemon.sock")).expect("symlink");
        let out = run_cli(
            &["session", "start", "--pty", "--name", "s", "--", "true"],
            &env,
            None,
        )
        .expect("start");
        check_code(&out, EXIT_OP_ERROR).expect("socket symlink refuses autostart");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("symlink"),
            "{:?}",
            out.stderr
        );
        assert!(!rt_path.join("daemon.pid").exists(), "no daemon spawned");
        std::fs::remove_file(rt_path.join("daemon.sock")).expect("unlink");
    }
    // A symlinked endpoint is skipped by list, refused by direct ops.
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "real", "--", "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, 0).expect("start");
    let child = started_pid(&out).expect("started pid");
    #[cfg(unix)]
    {
        let target = rt_path.join("target.dat");
        std::fs::write(&target, b"{}").expect("seed");
        std::os::unix::fs::symlink(&target, rt_path.join("s.json")).expect("symlink");
        let out = run_cli(&["session", "list"], &env, None).expect("list");
        check_code(&out, 0).expect("list skips symlink entries");
        assert!(
            !stdout(&out).lines().any(|l| l.starts_with("s ")),
            "{}",
            stdout(&out)
        );
        let out = run_cli(&["session", "stop", "--name", "s"], &env, None).expect("stop");
        check_code(&out, EXIT_OP_ERROR).expect("symlink endpoint refused");
        assert!(rt_path.join("s.json").is_symlink(), "link preserved");
        // A symlinked pidfile fails closed even with a live socket.
        let pid_raw = std::fs::read(rt_path.join("daemon.pid")).expect("pidfile");
        std::fs::remove_file(rt_path.join("daemon.pid")).expect("remove pidfile");
        std::os::unix::fs::symlink(&target, rt_path.join("daemon.pid")).expect("symlink");
        let out = run_cli(&["session", "list"], &env, None).expect("list");
        check_code(&out, EXIT_OP_ERROR).expect("pidfile symlink fails closed");
        std::fs::remove_file(rt_path.join("daemon.pid")).expect("unlink");
        std::fs::write(rt_path.join("daemon.pid"), pid_raw).expect("restore pidfile");
        let out = run_cli(&["session", "list"], &env, None).expect("list");
        check_code(&out, 0).expect("list works after pidfile restore");
    }
    let out = run_cli(&["session", "stop", "--name", "real"], &env, None).expect("stop");
    check_code(&out, 0).expect("stop");
    teardown(Path::new(&rt), &[child], true).expect("teardown");
}

#[test]
fn pty_runtime_dir_symlink_rejected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let real = tmp.path().join("real");
    std::fs::create_dir_all(&real).expect("mkdir");
    #[cfg(unix)]
    {
        let link = tmp.path().join("rt-link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let link = link.to_string_lossy().into_owned();
        let env = pty_env(&link);
        let out = run_cli(
            &["session", "start", "--pty", "--name", "x", "--", "true"],
            &env,
            None,
        )
        .expect("start");
        check_code(&out, EXIT_OP_ERROR).expect("symlink runtime dir refused");
    }
}

#[test]
fn pty_path_escape_rejected() {
    let (_tmp, rt) = fresh_rt("escape").expect("tempdir");
    let env = pty_env(&rt);
    let rt_path = PathBuf::from(&rt);
    std::fs::create_dir_all(&rt_path).expect("mkdir rt");
    let sentinel = rt_path.join("sentinel.keep");
    std::fs::write(&sentinel, b"keep").expect("sentinel");
    for bad in ["../evil", "a/b"] {
        let out = run_cli(
            &["session", "start", "--pty", "--name", bad, "--", "true"],
            &env,
            None,
        )
        .expect("start");
        check_code(&out, EXIT_OP_ERROR).expect("escape start rejected");
        for args in [
            vec!["session", "stop", "--name", bad],
            vec!["session", "input", "--name", bad, "--text", "x"],
            vec!["session", "observe", "--name", bad],
            vec!["session", "attach", "--name", bad],
        ] {
            let out = run_cli(&args, &env, None).expect("escape op");
            check_code(&out, EXIT_OP_ERROR).unwrap_or_else(|_| panic!("escape {args:?} rejected"));
        }
    }
    // A hostile payload name never steers a delete outside the dir.
    std::fs::write(
        rt_path.join("e.json"),
        r#"{"version":1,"name":"../../evil","pid":1,"argv":["x"],"backend":"pty","started_unix":1,"owner":0,"daemon_pid":1}"#,
    )
    .expect("seed");
    let out = run_cli(&["session", "prune"], &env, None).expect("prune");
    check_code(&out, EXIT_OP_ERROR).expect("escape name pruned");
    assert!(sentinel.is_file(), "nothing outside was deleted");
    assert!(!rt_path.parent().expect("parent").join("evil").exists());
    teardown(Path::new(&rt), &[], true).expect("teardown");
}
