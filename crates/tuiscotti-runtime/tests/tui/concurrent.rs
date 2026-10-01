//! F12 concurrency proof: independent sessions and proto-registry
//! sessions run concurrently with full isolation (split from `tui.rs`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tuiscotti_runtime::proto::{Op, OpResult, execute};
use tuiscotti_runtime::tui::Tui;

use super::{cancel, deadline};

/// Marker per worker: echoed back by `/bin/cat`, unique per thread.
fn marker(i: usize) -> String {
    format!("isolated-{i}-{}\n", std::process::id())
}

#[test]
fn independent_sessions_run_concurrently() {
    // Four threads, four owned sessions: spawn, drive, and close fully in
    // parallel. Each session must echo ONLY its own marker (isolation),
    // and no session may stall another (no shared lock across blocking
    // waits — the PTY lifecycle guard is spawn/kill-only and short).
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for i in 0..4 {
            handles.push(scope.spawn(move || {
                let s = Tui::new(["/bin/cat"])
                    .size(40, 8)
                    .spawn()
                    .expect("spawn succeeds");
                let want = marker(i);
                s.send_text(&want).expect("send_text succeeds");
                let obs = s
                    .wait_predicate(
                        |o| {
                            super::rows(&o.screen)
                                .is_ok_and(|r| r.iter().any(|row| row.contains(want.trim())))
                        },
                        deadline(15),
                        &cancel(),
                    )
                    .expect("own echo arrives");
                // Isolation: our marker is there...
                assert!(
                    super::rows(&obs.screen)
                        .expect("rows")
                        .iter()
                        .any(|row| row.contains(want.trim())),
                    "own marker missing: {want:?}"
                );
                // ...and nobody else's leaked in (all markers share the
                // pid but differ in the worker index).
                for j in 0..4 {
                    if j != i {
                        let other = format!("isolated-{j}-");
                        assert!(
                            !super::rows(&obs.screen)
                                .expect("rows")
                                .iter()
                                .any(|row| row.contains(&other)),
                            "cross-session leak: {other:?}"
                        );
                    }
                }
                s.close().expect("close succeeds");
            }));
        }
        for h in handles {
            h.join().expect("worker joins");
        }
    });
}

#[test]
fn registry_sessions_operate_concurrently() {
    // Same proof through the op protocol: parallel `wait` ops must not
    // serialize on the registry lock (F12 short critical sections). One
    // slow waiter (2s delayed output) runs beside three fast ones; all
    // four must observe their own markers.
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for i in 0..4usize {
            handles.push(scope.spawn(move || {
                let id = format!("conc-{}-{i}", std::process::id());
                let script = if i == 0 {
                    format!("sleep 2; printf '{m}\\n'; sleep 30", m = marker(i).trim())
                } else {
                    format!("printf '{m}\\n'; sleep 30", m = marker(i).trim())
                };
                match execute(&Op::Spawn {
                    argv: vec!["/bin/sh".to_string(), "-c".to_string(), script],
                    id: Some(id.clone()),
                    cols: Some(40),
                    rows: Some(8),
                    cwd: None,
                    env: HashMap::default(),
                })
                .expect("spawn op succeeds")
                {
                    OpResult::Spawned { session, .. } => assert_eq!(session, id),
                    r => panic!("wrong result: {r:?}"),
                }
                match execute(&Op::Wait {
                    session: id.clone(),
                    kind: "text".to_string(),
                    needle: Some(marker(i).trim().to_string()),
                    quiet_ms: None,
                    timeout_ms: 15_000,
                })
                .expect("wait op succeeds")
                {
                    OpResult::Waited { observation, .. } => {
                        assert!(
                            observation.screen.text.contains(marker(i).trim()),
                            "own marker missing: {}",
                            observation.screen.text
                        );
                    }
                    r => panic!("wrong result: {r:?}"),
                }
                // A second session id is rejected while the first lives.
                let dup = execute(&Op::Spawn {
                    argv: vec!["/bin/sleep".to_string(), "1".to_string()],
                    id: Some(id.clone()),
                    cols: None,
                    rows: None,
                    cwd: None,
                    env: HashMap::default(),
                })
                .expect_err("duplicate id must fail");
                assert_eq!(dup.code, "session-exists", "{dup}");
                // `exit` on the running app times out (evidence attached),
                // then the session is gone.
                let err = execute(&Op::Exit {
                    session: id.clone(),
                    timeout_ms: 100,
                })
                .expect_err("exit on a sleep must time out");
                assert_eq!(err.code, "timeout", "{err}");
                let gone = execute(&Op::Observe {
                    session: id.clone(),
                })
                .expect_err("exited session is gone");
                assert_eq!(gone.code, "not-found", "{gone}");
            }));
        }
        for h in handles {
            h.join().expect("worker joins");
        }
    });
}

#[test]
fn session_meta_is_cheap_and_current() {
    let s = Tui::new(["/bin/cat"])
        .size(40, 8)
        .spawn()
        .expect("spawn succeeds");
    let meta = s.meta().expect("meta present after spawn");
    assert_eq!((meta.cols, meta.rows), (40, 8));
    assert_eq!(meta.revision, s.revision());
    s.resize(60, 16).expect("resize succeeds");
    let meta = s.meta().expect("meta present after resize");
    assert_eq!((meta.cols, meta.rows), (60, 16));
    assert_eq!(
        (meta.revision, s.snapshot().expect("snapshot").cols()),
        (s.revision(), 60)
    );
    // Cheap: 20k metadata reads must finish in well under a second
    // (a worker round trip + screen clone each would take ~seconds).
    let start = Instant::now();
    for _ in 0..20_000 {
        let m = s.meta().expect("meta");
        assert_eq!((m.cols, m.rows), (60, 16));
    }
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "meta() too slow: {:?}",
        start.elapsed()
    );
    s.close().expect("close succeeds");
}
