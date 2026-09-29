//! Live observation + replay-vs-rerun tests (backlog A03, A05).

#![cfg(feature = "pty")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use tuiscotti::observe::{Replay, Rerun, Watcher, compare_replay_vs_rerun, screen_text};
use tuiscotti::screen::Screen;
use tuiscotti::tui::{CancelToken, Tui};
use tuiscotti::tui_shell::Recording;

fn contains(screen: &Screen, needle: &str) -> bool {
    screen_text(screen).contains(needle)
}

#[test]
fn watcher_receives_revisions() {
    let session = Arc::new(
        Tui::new(["/bin/cat"])
            .size(60, 12)
            .spawn()
            .expect("spawn cat"),
    );
    let watcher = Watcher::subscribe(Arc::clone(&session), 16, Duration::from_millis(5));
    let first = watcher
        .next_timeout(Duration::from_secs(5))
        .expect("initial observation");
    session.send_text("watch-me\n").expect("send text");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = None;
    while Instant::now() < deadline {
        match watcher.next_timeout(Duration::from_millis(200)) {
            Some(w) if contains(&w.observation.screen, "watch-me") => {
                seen = Some(w);
                break;
            }
            _ => {}
        }
    }
    let seen = seen.expect("watcher saw injected echo");
    assert!(seen.observation.revision > first.observation.revision);
    // Watcher sees exactly what assertions see: same Observation type.
    let direct = session.observe_now().expect("observe now");
    assert_eq!(direct.revision, session.revision());
    watcher.stop();
}

#[test]
fn watcher_lag_counter_under_flood() {
    let session = Arc::new(
        Tui::new([
            "/bin/sh",
            "-c",
            "i=0; while [ $i -lt 120 ]; do echo tick-$i; i=$((i+1)); sleep 0.02; done; sleep 30",
        ])
        .size(60, 12)
        .spawn()
        .expect("spawn ticker"),
    );
    // Tiny queue, fast producer, no draining: evictions must happen.
    let watcher = Watcher::subscribe(Arc::clone(&session), 2, Duration::from_millis(5));
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        watcher.dropped() > 0,
        "expected evictions under flood, got 0"
    );
    // Latest-wins: drain to the newest; revisions ascend.
    let mut last_rev = 0;
    let mut count = 0;
    while let Some(w) = watcher.try_next() {
        assert!(w.observation.revision >= last_rev);
        last_rev = w.observation.revision;
        count += 1;
    }
    assert!(count > 0);
    assert!(contains(
        &session.observe_now().expect("observe now").screen,
        "tick-"
    ));
    watcher.stop();
}

#[test]
fn inject_while_watching_round_trip() {
    let session = Arc::new(
        Tui::new(["/bin/cat"])
            .size(60, 12)
            .spawn()
            .expect("spawn cat"),
    );
    let watcher = Watcher::subscribe(Arc::clone(&session), 16, Duration::from_millis(5));
    watcher
        .next_timeout(Duration::from_secs(5))
        .expect("initial");
    watcher.inject(b"round-trip-1\n").expect("inject bytes");
    watcher.inject_text("round-trip-2\n").expect("inject text");
    let deadline = Instant::now() + Duration::from_secs(5);
    let (mut got1, mut got2) = (false, false);
    while Instant::now() < deadline && !(got1 && got2) {
        if let Some(w) = watcher.next_timeout(Duration::from_millis(200)) {
            let text = screen_text(&w.observation.screen);
            got1 |= text.contains("round-trip-1");
            got2 |= text.contains("round-trip-2");
        }
    }
    assert!(got1, "watcher missed inject() bytes");
    assert!(got2, "watcher missed inject_text()");
    watcher.stop();
}

#[test]
fn attach_cli_smoke() {
    let bin = env!("CARGO_BIN_EXE_tuiscotti");
    let tmp = tempfile::tempdir().expect("tempdir");
    let rt = tmp.path().join("rt");
    let run = |args: &[&str], input: Option<&[u8]>| -> std::process::Output {
        use std::io::Write;
        let mut cmd = std::process::Command::new(bin);
        cmd.env("TUISCOTTI_RUNTIME_DIR", &rt).args(args);
        if let Some(data) = input {
            cmd.stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut child = cmd.spawn().expect("spawn tuiscotti");
            child
                .stdin
                .take()
                .expect("piped stdin")
                .write_all(data)
                .expect("write stdin");
            child.wait_with_output().expect("wait tuiscotti")
        } else {
            cmd.output().expect("run tuiscotti")
        }
    };
    // Start a sleep session, then attach with piped stdin (immediate EOF).
    let out = run(
        &[
            "session",
            "start",
            "--name",
            "attach-smoke",
            "--",
            "/bin/sleep",
            "30",
        ],
        None,
    );
    assert!(out.status.success(), "start failed: {out:?}");
    let out = run(
        &["session", "attach", "--name", "attach-smoke"],
        Some(b"typed-input\n"),
    );
    assert!(out.status.success(), "attach failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("attached: attach-smoke"),
        "no header: {stdout}"
    );
    assert!(
        stdout.contains("best-effort human view"),
        "no doc line: {stdout}"
    );
    assert!(stdout.contains("detached:"), "no detach line: {stdout}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no input transport"), "no warn: {stderr}");
    // Attach detached without killing: still running, then stop it.
    let out = run(&["session", "list"], None);
    let list = String::from_utf8_lossy(&out.stdout);
    assert!(list.contains("attach-smoke"), "session died: {list}");
    let out = run(&["session", "stop", "--name", "attach-smoke"], None);
    assert!(out.status.success(), "stop failed: {out:?}");
    // Unknown session is a usage-level error, not a hang.
    let out = run(&["session", "attach", "--name", "nope-missing"], Some(b""));
    assert!(!out.status.success());
}

#[test]
fn replay_determinism_no_spawn() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let marker = tmp.path().join("must-not-exist");
    let mut rec = Recording::new(60, 12);
    rec.push_output(b"hello\r\n").expect("record output");
    rec.push_input(b"typed-but-never-replayed")
        .expect("record input");
    rec.push_output(b"world\r\n").expect("record output");
    let a = rec.replay_observations().expect("replay");
    let b = rec.replay_observations().expect("replay");
    assert_eq!(a, b, "same bytes must yield same screens");
    assert_eq!(a.len(), 1);
    assert!(contains(&a[0], "hello"));
    assert!(contains(&a[0], "world"));
    assert!(
        !contains(&a[0], "typed-but-never-replayed"),
        "recorded input leaked into replay"
    );
    // Replay path spawns nothing: marker command would create it, replay must not.
    let replay = Replay::from_recording(&rec);
    let r1 = replay.execute().expect("execute replay");
    let r2 = replay.execute().expect("execute replay");
    assert_eq!(r1.screen, r2.screen);
    assert_eq!(r1.screen, a[0]);
    assert!(!marker.exists(), "replay spawned a child");
    // Same-vs-same comparison reports SAME.
    let cmp = compare_replay_vs_rerun(&a, &r1.screen);
    assert!(cmp.same, "{}", cmp.detail);
    assert_eq!(cmp.replay_hashes.len(), 1);
    drop(CancelToken::new());
}

#[test]
fn rerun_spawns_marker() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let marker = tmp.path().join("spawned-proof");
    let out = Rerun::new(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            format!("touch {} && printf 'rerun-hi\\n'", marker.display()),
        ],
        60,
        12,
    )
    .execute()
    .expect("execute rerun");
    assert!(out.status.success());
    assert!(out.pid.is_some(), "no child pid recorded");
    assert!(marker.exists(), "child never spawned (no marker)");
    assert!(contains(&out.observation.screen, "rerun-hi"));
}

#[test]
fn replay_vs_rerun_diff_report_on_nondeterministic_fixture() {
    // Nondeterministic fixture: `date +%N` differs run to run.
    let probe = std::process::Command::new("/bin/sh")
        .args(["-c", "date +%N"])
        .output()
        .expect("probe date");
    assert!(probe.status.success());
    let mut rec = Recording::new(60, 12);
    rec.push_output(&probe.stdout).expect("record output");
    let replayed = rec.replay_observations().expect("replay");
    let rerun = Rerun::new(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "date +%N".to_string(),
        ],
        60,
        12,
    )
    .execute()
    .expect("execute rerun");
    let cmp = compare_replay_vs_rerun(&replayed, &rerun.observation.screen);
    assert!(!cmp.same, "expected DIFFERENT: {}", cmp.detail);
    assert!(cmp.detail.contains("DIFFERENT"), "{}", cmp.detail);
    assert_eq!(cmp.replay_hashes.len(), replayed.len());
}
