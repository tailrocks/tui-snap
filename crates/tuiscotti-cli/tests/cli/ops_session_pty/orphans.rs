//! Pid 0 / reuse, orphans, failed kills.

use std::path::{Path, PathBuf};

use super::super::run_cli;
use super::helpers::{
    check_code, daemon_pid_of, fresh_rt, kill9, list_until, observe_until, pid_dead, pty_env,
    started_pid, teardown, wait_for,
};
use tuiscotti::proto::EXIT_OP_ERROR;

#[test]
fn pty_pid_zero_rejected_without_signaling() {
    let (_tmp, rt) = fresh_rt("pid0").expect("tempdir");
    let env = pty_env(&rt);
    let rt_path = PathBuf::from(&rt);
    std::fs::create_dir_all(&rt_path).expect("mkdir rt");
    std::fs::write(
        rt_path.join("z.json"),
        r#"{"version":1,"name":"z","pid":0,"argv":["x"],"backend":"pty","started_unix":1,"owner":0,"daemon_pid":1}"#,
    )
    .expect("seed");
    let before = std::fs::read(rt_path.join("z.json")).expect("read");
    for args in [
        vec!["session", "list"],
        vec!["session", "stop", "--name", "z"],
        vec!["session", "input", "--name", "z", "--text", "x"],
        vec!["session", "observe", "--name", "z"],
    ] {
        let out = run_cli(&args, &env, None).expect("pid-0 op");
        check_code(&out, EXIT_OP_ERROR).unwrap_or_else(|_| panic!("pid 0 {args:?}"));
    }
    assert_eq!(std::fs::read(rt_path.join("z.json")).expect("read"), before);
    teardown(Path::new(&rt), &[], true).expect("teardown");
}

#[test]
fn pty_stop_after_exit_succeeds() {
    let (_tmp, rt) = fresh_rt("exited").expect("tempdir");
    let env = pty_env(&rt);
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "quick", "--", "sh", "-c", "exit 3",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, 0).expect("start");
    let child = started_pid(&out).expect("started pid");
    // The child is long gone (and its pid may already be reused): stop
    // must succeed by closing, never by signaling the stale pid.
    list_until(&env, "quick", "Exited", 10).expect("poll");
    let out = run_cli(&["session", "stop", "--name", "quick"], &env, None).expect("stop");
    check_code(&out, 0).expect("stop after exit");
    assert!(!PathBuf::from(&rt).join("quick.json").exists());
    teardown(Path::new(&rt), &[child], true).expect("teardown");
}

#[test]
fn pty_daemon_crash_orphans_recover_cleanly() {
    let (_tmp, rt) = fresh_rt("crash").expect("tempdir");
    let env = pty_env(&rt);
    let rt_path = PathBuf::from(&rt);
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "orph", "--", "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, 0).expect("start");
    let child = started_pid(&out).expect("started pid");
    observe_until(&env, "orph", "revision=", 10).expect("poll");
    let daemon = daemon_pid_of(&rt_path).expect("daemon pid");
    kill9(daemon).expect("kill");
    wait_for("daemon to die", 10, || pid_dead(daemon)).expect("daemon to die");
    // Orphaned: lists Exited, transports fail cleanly (no hang).
    list_until(&env, "orph", "Exited", 10).expect("poll");
    for args in [
        vec!["session", "input", "--name", "orph", "--text", "x"],
        vec!["session", "observe", "--name", "orph"],
    ] {
        let out = run_cli(&args, &env, None).expect("orphan op");
        check_code(&out, EXIT_OP_ERROR).unwrap_or_else(|_| panic!("orphan {args:?}"));
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("not-found"),
            "{:?}",
            out.stderr
        );
    }
    // Stop kills the orphan through the validated pid path, then a fresh
    // autostart serves the runtime dir again (stale files swept).
    let out = run_cli(&["session", "stop", "--name", "orph"], &env, None).expect("stop");
    check_code(&out, 0).expect("orphan stop kills + removes");
    assert!(!rt_path.join("orph.json").exists());
    wait_for("orphan to die", 10, || pid_dead(child)).expect("orphan to die");
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "after", "--", "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, 0).expect("autostart after crash");
    let child2 = started_pid(&out).expect("started pid");
    let daemon2 = daemon_pid_of(&rt_path).expect("new daemon pid");
    assert_ne!(daemon, daemon2, "a fresh daemon serves now");
    let out = run_cli(&["session", "stop", "--name", "after"], &env, None).expect("stop");
    check_code(&out, 0).expect("stop");
    teardown(Path::new(&rt), &[child, child2], true).expect("teardown");
}

#[test]
fn pty_failed_stop_preserves_endpoint_and_session() {
    // Split brain, staged deterministically: an endpoint naming a live
    // FOREIGN daemon. Every mutating op must fail closed with both the
    // endpoint and the live session untouched.
    let (_tmp_a, rt_a) = fresh_rt("split-a").expect("tempdir");
    let (_tmp_b, rt_b) = fresh_rt("split-b").expect("tempdir");
    let env_a = pty_env(&rt_a);
    let env_b = pty_env(&rt_b);
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "s1", "--", "sleep", "30",
        ],
        &env_a,
        None,
    )
    .expect("start s1 in A");
    check_code(&out, 0).expect("start s1");
    let child_a = started_pid(&out).expect("started pid");
    let daemon_a = daemon_pid_of(Path::new(&rt_a)).expect("daemon A");
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "other", "--", "sleep", "30",
        ],
        &env_b,
        None,
    )
    .expect("start other in B");
    check_code(&out, 0).expect("start other");
    let child_b = started_pid(&out).expect("started pid");
    // Plant A's record in B: B's daemon is live, but the recorded owner
    // (A, alive) is not the serving daemon — split brain.
    let planted = PathBuf::from(&rt_b).join("s1.json");
    std::fs::copy(PathBuf::from(&rt_a).join("s1.json"), &planted).expect("plant");
    let before = std::fs::read(&planted).expect("read");
    assert!(!pid_dead(daemon_a).expect("probe"), "owner A must be alive");
    for args in [
        vec!["session", "stop", "--name", "s1"],
        vec!["session", "prune"],
        vec!["session", "input", "--name", "s1", "--text", "x"],
        vec!["session", "observe", "--name", "s1"],
    ] {
        let out = run_cli(&args, &env_b, None).expect("split-brain op");
        check_code(&out, EXIT_OP_ERROR).unwrap_or_else(|_| panic!("split brain {args:?}"));
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("op-failed"),
            "{:?}",
            out.stderr
        );
    }
    assert_eq!(
        std::fs::read(&planted).expect("read"),
        before,
        "endpoint kept"
    );
    // The live session is untouched: A still serves it.
    observe_until(&env_a, "s1", "revision=", 10).expect("poll");
    list_until(&env_a, "s1", "Running", 10).expect("poll");
    // Cleanup: remove the plant, stop both sessions normally.
    std::fs::remove_file(&planted).expect("unplant");
    let out = run_cli(&["session", "stop", "--name", "s1"], &env_a, None).expect("stop");
    check_code(&out, 0).expect("stop s1");
    let out = run_cli(&["session", "stop", "--name", "other"], &env_b, None).expect("stop");
    check_code(&out, 0).expect("stop other");
    teardown(Path::new(&rt_a), &[child_a], true).expect("teardown");
    teardown(Path::new(&rt_b), &[child_b], true).expect("teardown");
}
