//! Write failures, oversize IPC, races, force.

use std::path::{Path, PathBuf};

use super::super::{code, run_cli, stdout};
use super::helpers::{
    TRes, check_code, daemon_pid_of, fresh_rt, pid_dead, pty_env, started_pid, teardown, wait_for,
};
use tuiscotti::proto::EXIT_OP_ERROR;

#[test]
fn pty_blocked_endpoint_fails_before_spawn() {
    let (_tmp, rt) = fresh_rt("blocked").expect("tempdir");
    let env = pty_env(&rt);
    let rt_path = PathBuf::from(&rt);
    std::fs::create_dir_all(rt_path.join("b.json")).expect("block endpoint");
    // Absolute marker: the child inherits the caller's cwd, so a relative
    // marker would land outside the runtime dir (and pollute the repo).
    let marker = rt_path.join("MARKER-SPAWNED");
    let script = format!("touch {}; sleep 30", marker.to_string_lossy());
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "b", "--", "sh", "-c", &script,
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, EXIT_OP_ERROR).expect("blocked endpoint refuses start");
    assert!(rt_path.join("b.json").is_dir(), "blocker untouched");
    assert!(!marker.exists(), "losing starter never spawns");
    // The daemon autostarted anyway (it serves the dir); it idles out.
    teardown(Path::new(&rt), &[], true).expect("teardown");
}

#[test]
fn pty_raw_ipc_bounds_and_validation() {
    let (_tmp, rt) = fresh_rt("rawipc").expect("tempdir");
    let env = pty_env(&rt);
    let rt_path = PathBuf::from(&rt);
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "raw", "--", "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, 0).expect("start");
    let child = started_pid(&out).expect("started pid");
    // Speak the wire protocol directly over the socket.
    let sock = rt_path.join("daemon.sock");
    let transact_raw = |line: &[u8]| -> TRes<serde_json::Value> {
        use std::io::{Read as _, Write as _};
        use std::os::unix::net::UnixStream;
        let mut s = UnixStream::connect(&sock).map_err(|e| format!("connect: {e}"))?;
        s.set_read_timeout(Some(std::time::Duration::from_secs(15)))
            .map_err(|e| format!("timeout: {e}"))?;
        // The server may refuse + close mid-write (oversize line): a
        // broken pipe then is expected — its verdict still follows.
        if s.write_all(line).is_err() {
            // Server already refused; read its verdict below.
        }
        if s.write_all(b"\n").is_err() {
            // Server already refused; read its verdict below.
        }
        let mut buf = Vec::new();
        let mut one = [0u8; 1];
        loop {
            s.read_exact(&mut one)
                .map_err(|e| format!("reply byte: {e}"))?;
            if one[0] == b'\n' {
                break;
            }
            buf.push(one[0]);
        }
        serde_json::from_slice(&buf).map_err(|e| format!("reply json: {e}"))
    };
    // Oversize request: rejected with its bound code, id 0 (unparsed).
    let big = vec![b'x'; 1_048_577];
    let res = transact_raw(&big).expect("ipc");
    assert_eq!(res["ok"], false);
    assert_eq!(res["error"]["code"], "bound-exceeded");
    // Malformed JSON: invalid-input, id 0.
    let res = transact_raw(b"{nope").expect("ipc");
    assert_eq!(res["ok"], false);
    assert_eq!(res["error"]["code"], "invalid-input");
    // Hostile name: the daemon re-validates before dispatch.
    let res = transact_raw(br#"{"id":9,"op":{"op":"observe","name":"../evil"}}"#).expect("ipc");
    assert_eq!(res["id"], 9);
    assert_eq!(res["ok"], false);
    assert_eq!(res["error"]["code"], "invalid-input");
    // Unknown signal name: invalid-input, session attached.
    let res = transact_raw(br#"{"id":10,"op":{"op":"signal","name":"raw","sig":"SIGTERM"}}"#)
        .expect("ipc");
    assert_eq!(res["ok"], false);
    assert_eq!(res["error"]["code"], "invalid-input");
    // Unknown wait kind: invalid-input.
    let res = transact_raw(
        br#"{"id":11,"op":{"op":"wait","name":"raw","kind":"bogus","timeout_ms":100}}"#,
    )
    .expect("ipc");
    assert_eq!(res["ok"], false);
    assert_eq!(res["error"]["code"], "invalid-input");
    // Unknown session: not-found (the daemon still serves afterwards).
    let res = transact_raw(br#"{"id":12,"op":{"op":"observe","name":"ghost"}}"#).expect("ipc");
    assert_eq!(res["ok"], false);
    assert_eq!(res["error"]["code"], "not-found");
    // And the session itself is untouched by the hostile lines.
    let out = run_cli(&["session", "observe", "--name", "raw"], &env, None).expect("observe");
    check_code(&out, 0).expect("observe after hostile lines");
    let out = run_cli(&["session", "stop", "--name", "raw"], &env, None).expect("stop");
    check_code(&out, 0).expect("stop");
    teardown(Path::new(&rt), &[child], true).expect("teardown");
}

#[test]
fn pty_concurrent_same_name_single_winner() {
    let (_tmp, rt) = fresh_rt("race").expect("tempdir");
    let env = pty_env(&rt);
    let mut handles = Vec::new();
    for _ in 0..4 {
        let rt = rt.clone();
        handles.push(std::thread::spawn(move || {
            let env = pty_env(&rt);
            run_cli(
                &[
                    "session", "start", "--pty", "--name", "race", "--", "sleep", "30",
                ],
                &env,
                None,
            )
        }));
    }
    let mut winners = Vec::new();
    for h in handles {
        let out = h.join().expect("thread").expect("start");
        match code(&out) {
            Some(0) => winners.push(started_pid(&out).expect("started pid")),
            Some(3) => assert!(
                String::from_utf8_lossy(&out.stderr).contains("session-exists"),
                "{:?}",
                out.stderr
            ),
            c => panic!("unexpected exit {c:?}: {}", stdout(&out)),
        }
    }
    assert_eq!(winners.len(), 1, "exactly one winner");
    let out = run_cli(&["session", "list"], &env, None).expect("list");
    assert!(stdout(&out).contains("race"), "{}", stdout(&out));
    let out = run_cli(&["session", "stop", "--name", "race"], &env, None).expect("stop");
    check_code(&out, 0).expect("stop");
    teardown(Path::new(&rt), &winners, true).expect("teardown");
}

#[test]
fn pty_daemon_double_start_single_owner() {
    let (_tmp, rt) = fresh_rt("dbl").expect("tempdir");
    let env = pty_env(&rt);
    let rt_path = PathBuf::from(&rt);
    std::fs::create_dir_all(&rt_path).expect("mkdir rt");
    // Stale lock residue (dead owner, ancient stamp): taken over, not fatal.
    std::fs::write(rt_path.join("daemon.lock"), format!("{} 1\n", u32::MAX)).expect("seed");
    // Two starters race the autostart with different names: both win, one
    // daemon serves both, no boot failure is recorded anywhere.
    let mut handles = Vec::new();
    for name in ["d1", "d2"] {
        let rt = rt.clone();
        handles.push(std::thread::spawn(move || {
            let env = pty_env(&rt);
            run_cli(
                &[
                    "session", "start", "--pty", "--name", name, "--", "sleep", "30",
                ],
                &env,
                None,
            )
        }));
    }
    let mut children = Vec::new();
    for h in handles {
        let out = h.join().expect("thread").expect("start");
        check_code(&out, 0).expect("racing start");
        children.push(started_pid(&out).expect("started pid"));
    }
    let daemon = daemon_pid_of(&rt_path).expect("one daemon pid");
    assert!(
        !pid_dead(daemon).expect("probe"),
        "the single owner is alive"
    );
    assert!(!rt_path.join("daemon.err").exists(), "no boot failure");
    let out = run_cli(&["session", "list"], &env, None).expect("list");
    check_code(&out, 0).expect("list");
    assert!(
        stdout(&out).contains("d1") && stdout(&out).contains("d2"),
        "{}",
        stdout(&out)
    );
    for name in ["d1", "d2"] {
        let out = run_cli(&["session", "stop", "--name", name], &env, None).expect("stop");
        check_code(&out, 0).expect("stop");
    }
    teardown(Path::new(&rt), &children, true).expect("teardown");
}

#[test]
fn pty_force_start_replaces_across_backends() {
    let (_tmp, rt) = fresh_rt("force").expect("tempdir");
    let env = pty_env(&rt);
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "f", "--", "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, 0).expect("start");
    let first = started_pid(&out).expect("started pid");
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "f", "--", "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, EXIT_OP_ERROR).expect("collision without force");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("session-exists"),
        "{:?}",
        out.stderr
    );
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "f", "--force", "--", "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("force start");
    check_code(&out, 0).expect("force replaces");
    let second = started_pid(&out).expect("started pid");
    assert_ne!(first, second);
    wait_for("replaced child to die", 10, || pid_dead(first)).expect("replaced child to die");
    // Force across backends, both directions.
    let out = run_cli(
        &[
            "session", "start", "--name", "f", "--force", "--", "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("piped force over pty");
    check_code(&out, 0).expect("piped force over pty");
    let third = started_pid(&out).expect("started pid");
    wait_for("pty child to die", 10, || pid_dead(second)).expect("pty child to die");
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "f", "--force", "--", "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("pty force over piped");
    check_code(&out, 0).expect("pty force over piped");
    let fourth = started_pid(&out).expect("started pid");
    wait_for("piped child to die", 10, || pid_dead(third)).expect("piped child to die");
    let out = run_cli(&["session", "stop", "--name", "f"], &env, None).expect("stop");
    check_code(&out, 0).expect("stop");
    teardown(Path::new(&rt), &[first, second, third, fourth], true).expect("teardown");
}
