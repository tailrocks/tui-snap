//! `session --pty`: retained PTY sessions behind the daemon (F08-F2).
//!
//! Cross-process CLI tests: every op runs in its own `tuiscotti`
//! invocation against an isolated `TUISCOTTI_RUNTIME_DIR`, with
//! `TUISCOTTI_DAEMON_IDLE_SECS=1` so daemons exit a second after their
//! last session. Every test ends by asserting its daemon and children
//! are gone (no strays outlive the suite).

use super::{code, run_cli, stdout};
use std::path::{Path, PathBuf};
use std::process::Output;
use tuiscotti::proto::EXIT_OP_ERROR;

// ---------------------------------------------------------------------------
// helpers (return `Result`: only `#[test]` bodies may expect/panic)
// ---------------------------------------------------------------------------

/// Test-helper result: `String` errors, surfaced by the test's `expect`.
type TRes<T> = Result<T, String>;

/// Isolated runtime dir + 1 s daemon idle for one test.
fn pty_env(rt: &str) -> [(&str, &str); 2] {
    [
        ("TUISCOTTI_RUNTIME_DIR", rt),
        ("TUISCOTTI_DAEMON_IDLE_SECS", "1"),
    ]
}

fn fresh_rt(test: &str) -> TRes<(tempfile::TempDir, String)> {
    let tmp = tempfile::tempdir().map_err(|e| format!("tempdir: {e}"))?;
    let rt = tmp.path().join(test).to_string_lossy().into_owned();
    Ok((tmp, rt))
}

fn check_code(out: &Output, want: i32) -> TRes<()> {
    let got = code(out).ok_or_else(|| "no exit code".to_string())?;
    if got != want {
        return Err(format!(
            "exit {got}, want {want}: stdout={} stderr={}",
            stdout(out),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// Parse `started: {name} (pid {N})` from a start's stdout.
fn started_pid(out: &Output) -> TRes<u32> {
    let text = stdout(out);
    let (_, pid) = text
        .trim()
        .rsplit_once("(pid ")
        .ok_or_else(|| format!("no pid in {text:?}"))?;
    pid.trim_end_matches(')')
        .trim()
        .parse()
        .map_err(|_| format!("bad pid in {text:?}"))
}

/// First absolute `kill(1)` that is a regular file (never a PATH lookup).
fn kill_bin() -> TRes<&'static str> {
    for bin in ["/bin/kill", "/usr/bin/kill"] {
        if std::fs::symlink_metadata(bin).is_ok_and(|m| m.file_type().is_file()) {
            return Ok(bin);
        }
    }
    Err("no kill binary".to_string())
}

/// True when `kill -0` says the pid is gone (spawn under the shared
/// lock: every fork in this binary takes it, piped or not).
fn pid_dead(pid: u32) -> TRes<bool> {
    let bin = kill_bin()?;
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("-0")
        .arg(pid.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = super::spawn_locked(&mut cmd)
        .map_err(|e| format!("kill -0 {pid}: {e}"))?
        .wait_with_output()
        .map_err(|e| format!("kill -0 {pid}: {e}"))?;
    Ok(!out.status.success())
}

/// Poll `cond` until true or `secs` elapse.
fn wait_for(what: &str, secs: u64, mut cond: impl FnMut() -> TRes<bool>) -> TRes<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if cond()? {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Err(format!("timed out waiting for {what}"))
}

/// Poll `session observe` until the screen contains `needle`.
fn observe_until(env: &[(&str, &str)], name: &str, needle: &str, secs: u64) -> TRes<String> {
    let mut last = String::new();
    wait_for(&format!("{name:?} to show {needle:?}"), secs, || {
        let out = run_cli(&["session", "observe", "--name", name], env, None)
            .map_err(|e| e.to_string())?;
        if code(&out) != Some(0) {
            return Ok(false);
        }
        last = stdout(&out);
        Ok(last.contains(needle))
    })?;
    Ok(last)
}

/// Poll `session list` until `name` lists with `status` (`Running`/`Exited`).
fn list_until(env: &[(&str, &str)], name: &str, status: &str, secs: u64) -> TRes<()> {
    wait_for(&format!("{name:?} to list {status}"), secs, || {
        let out = run_cli(&["session", "list"], env, None).map_err(|e| e.to_string())?;
        Ok(code(&out) == Some(0)
            && stdout(&out)
                .lines()
                .any(|l| l.contains(name) && l.contains(status)))
    })
}

fn daemon_pid_of(rt: &Path) -> Option<u32> {
    std::fs::read_to_string(rt.join("daemon.pid"))
        .ok()
        .and_then(|t| t.trim().parse().ok())
}

fn kill9(pid: u32) -> TRes<()> {
    let bin = kill_bin()?;
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("-9").arg(pid.to_string());
    let st = super::spawn_locked(&mut cmd)
        .map_err(|e| format!("kill -9 {pid}: {e}"))?
        .wait()
        .map_err(|e| format!("kill -9 {pid}: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("kill -9 {pid} failed"))
    }
}

/// End-of-test gate: every known child reaped, the daemon exited, and
/// (clean shutdowns only) its socket + pidfile swept.
fn teardown(rt: &Path, children: &[u32], swept: bool) -> TRes<()> {
    for pid in children {
        wait_for(&format!("child {pid} to die"), 10, || pid_dead(*pid))?;
    }
    if let Some(d) = daemon_pid_of(rt) {
        wait_for(&format!("daemon {d} to idle out"), 15, || pid_dead(d))?;
    }
    if swept {
        if rt.join("daemon.sock").exists() {
            return Err("daemon socket not swept after idle exit".to_string());
        }
        if rt.join("daemon.pid").exists() {
            return Err("daemon pidfile not swept after idle exit".to_string());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// cross-process lifecycle with transparent autostart
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// tampered metadata, symlinks, path escape
// ---------------------------------------------------------------------------

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
    let mut dead_child = super::spawn_locked(&mut true_cmd).expect("spawn true");
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

// ---------------------------------------------------------------------------
// pid 0 / reuse, orphans, failed kills
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// write failures, oversize IPC, races, force
// ---------------------------------------------------------------------------

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
