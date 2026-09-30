//! Cross-process lifecycle with transparent autostart.

use std::path::{Path, PathBuf};

use super::super::{code, run_cli, stdout};
use super::helpers::{
    check_code, fresh_rt, list_until, observe_until, pty_env, started_pid, teardown,
};
use tuiscotti::proto::EXIT_OP_ERROR;

#[test]
fn pty_cross_process_input_observe_attach_stop() {
    let (_tmp, rt) = fresh_rt("life").expect("tempdir");
    let env = pty_env(&rt);
    // `stty -echo` first: typed bytes appear only when DELIVERED (the
    // line discipline won't echo them), so the marker below proves the
    // input path rather than PTY echo.
    let out = run_cli(
        &[
            "session",
            "start",
            "--pty",
            "--name",
            "life",
            "--",
            "sh",
            "-c",
            "echo ready-marker; stty -echo; cat",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, 0).expect("start --pty autostarts the daemon");
    let child = started_pid(&out).expect("started pid");
    assert!(PathBuf::from(&rt).join("daemon.sock").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(PathBuf::from(&rt).join("daemon.sock"))
            .expect("stat")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "daemon socket is owner-only");
    }
    let out = run_cli(&["session", "list"], &env, None).expect("list");
    check_code(&out, 0).expect("list");
    assert!(
        stdout(&out).contains("life") && stdout(&out).contains("Running"),
        "{}",
        stdout(&out)
    );
    observe_until(&env, "life", "ready-marker", 10).expect("poll");
    let out = run_cli(
        &[
            "session",
            "input",
            "--name",
            "life",
            "--text",
            "typed-123\n",
        ],
        &env,
        None,
    )
    .expect("input");
    check_code(&out, 0).expect("input from process B");
    // `cat` echoes the delivered line (echo was disabled above).
    observe_until(&env, "life", "typed-123", 10).expect("poll");
    let out = run_cli(&["session", "attach", "--name", "life"], &env, Some("")).expect("attach");
    check_code(&out, 0).expect("attach renders + detaches on EOF");
    assert!(stdout(&out).contains("typed-123"), "{}", stdout(&out));
    assert!(stdout(&out).contains("detached:"), "{}", stdout(&out));
    let out = run_cli(&["session", "stop", "--name", "life"], &env, None).expect("stop");
    check_code(&out, 0).expect("stop from process C");
    assert!(stdout(&out).contains("stopped: life"), "{}", stdout(&out));
    let out = run_cli(&["session", "list"], &env, None).expect("list");
    check_code(&out, 0).expect("list");
    assert!(!stdout(&out).contains("life"), "{}", stdout(&out));
    teardown(Path::new(&rt), &[child], true).expect("teardown");
}

#[test]
fn pty_dotted_names_round_trip() {
    let (_tmp, rt) = fresh_rt("dots").expect("tempdir");
    let env = pty_env(&rt);
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "a.b", "--", "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("start a.b");
    check_code(&out, 0).expect("start a.b");
    let ab = started_pid(&out).expect("started pid");
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "x.y.z", "--", "sh", "-c", "exit 0",
        ],
        &env,
        None,
    )
    .expect("start x.y.z");
    check_code(&out, 0).expect("start x.y.z");
    let xyz = started_pid(&out).expect("started pid");
    list_until(&env, "x.y.z", "Exited", 10).expect("poll");
    let out = run_cli(&["session", "prune"], &env, None).expect("prune");
    check_code(&out, 0).expect("prune");
    assert!(stdout(&out).contains("x.y.z"), "{}", stdout(&out));
    let out = run_cli(&["session", "stop", "--name", "a.b"], &env, None).expect("stop");
    check_code(&out, 0).expect("stop a.b");
    let out = run_cli(&["session", "list"], &env, None).expect("list");
    assert!(!stdout(&out).contains("a.b"), "{}", stdout(&out));
    teardown(Path::new(&rt), &[ab, xyz], true).expect("teardown");
}

#[test]
fn pty_geometry_pairs_and_ranges() {
    let (_tmp, rt) = fresh_rt("geo").expect("tempdir");
    let env = pty_env(&rt);
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "geo", "--cols", "100", "--rows", "30", "--",
            "sleep", "30",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, 0).expect("paired geometry");
    let child = started_pid(&out).expect("started pid");
    let out = run_cli(&["session", "observe", "--name", "geo"], &env, None).expect("observe");
    check_code(&out, 0).expect("observe");
    assert!(stdout(&out).contains("100x30"), "{}", stdout(&out));
    // Unpaired geometry is a usage-shaped error, not a spawn.
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "bad", "--cols", "100", "--", "sleep", "1",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, EXIT_OP_ERROR).expect("unpaired cols");
    // Geometry without --pty is a CLI usage error.
    let out = run_cli(
        &[
            "session", "start", "--name", "bad2", "--cols", "100", "--rows", "30", "--", "true",
        ],
        &env,
        None,
    )
    .expect("start");
    assert_eq!(code(&out), Some(2), "{}", stdout(&out));
    // Out-of-range geometry never spawns.
    let out = run_cli(
        &[
            "session", "start", "--pty", "--name", "bad3", "--cols", "1", "--rows", "1", "--",
            "true",
        ],
        &env,
        None,
    )
    .expect("start");
    check_code(&out, EXIT_OP_ERROR).expect("cols below backend range");
    assert!(!PathBuf::from(&rt).join("bad3.json").exists());
    let out = run_cli(&["session", "stop", "--name", "geo"], &env, None).expect("stop");
    check_code(&out, 0).expect("stop");
    teardown(Path::new(&rt), &[child], true).expect("teardown");
}

#[test]
fn pty_daemon_name_reserved_and_piped_guards() {
    let (_tmp, rt) = fresh_rt("reserved").expect("tempdir");
    let env = pty_env(&rt);
    for backend in [&[][..], &["--pty"][..]] {
        let mut args = vec!["session", "start", "--name", "daemon"];
        args.extend(backend.iter().copied());
        args.extend(["--", "true"]);
        let out = run_cli(&args, &env, None).expect("start");
        check_code(&out, EXIT_OP_ERROR).expect("`daemon` is reserved");
    }
    // Piped sessions have no input/observe transport: honest errors.
    let out = run_cli(
        &["session", "start", "--name", "piped", "--", "sleep", "30"],
        &env,
        None,
    )
    .expect("start piped");
    check_code(&out, 0).expect("piped start");
    let child = started_pid(&out).expect("started pid");
    let out = run_cli(
        &["session", "input", "--name", "piped", "--text", "hi"],
        &env,
        None,
    )
    .expect("input");
    check_code(&out, EXIT_OP_ERROR).expect("piped input refused");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unsupported"),
        "{:?}",
        out.stderr
    );
    let out = run_cli(&["session", "observe", "--name", "piped"], &env, None).expect("observe");
    check_code(&out, EXIT_OP_ERROR).expect("piped observe refused");
    // Piped attach still tails the log.
    let out = run_cli(&["session", "attach", "--name", "piped"], &env, Some("")).expect("attach");
    check_code(&out, 0).expect("piped attach");
    assert!(
        stdout(&out).contains("detached: stdin EOF"),
        "{}",
        stdout(&out)
    );
    let out = run_cli(&["session", "stop", "--name", "piped"], &env, None).expect("stop");
    check_code(&out, 0).expect("piped stop");
    teardown(Path::new(&rt), &[child], true).expect("teardown");
}
