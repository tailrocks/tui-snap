//! R09 scoped guardian (split from `tui_shell.rs`; shared helpers live in the root).

use super::{cancel, deadline, pgid_of, pgrep, pkill, wait_found, wait_gone};
use std::time::{Duration, Instant};
use tuiscotti_runtime::tui::{Tui, process_exists};
use tuiscotti_runtime::tui_shell::{Containment, Guardian, GuardianReport};

// ---------------------------------------------------------------------------
// R09: scoped guardian
// ---------------------------------------------------------------------------

#[test]
fn guardian_contains_group() {
    let session = Tui::new(["/bin/sh", "-c", "trap '' TERM; sleep 29371"])
        .env("ENV", "/dev/null")
        .size(40, 10)
        .spawn()
        .expect("spawn succeeds");
    let child_pid = session.pid().expect("pid succeeds");
    assert!(!wait_found("29371", 5).is_empty());
    let guardian = Guardian::wrap(session);
    let report: GuardianReport = guardian.finish(deadline(10)).expect("finish succeeds");
    assert_eq!(report.child_pid, Some(child_pid));
    assert_eq!(report.containment, Containment::Full);
    assert!(report.teardown_error.is_none());
    assert!(pgrep("29371").is_empty());
    assert!(!process_exists(child_pid));
}

#[test]
fn guardian_escape_boundary_setsid_outlives() {
    // Pure-sh escape vehicle: `set -m` enables job control, so the background
    // `sleep` is placed in its own process group via setpgid(2) — the same
    // group-leaving boundary as setsid(2), without python (rust-only policy)
    // and without setsid(1), which macOS does not ship.
    let script = "set -m; sleep 29372 & exec sleep 29373";
    let session = Tui::new(["/bin/sh", "-c", script])
        .env("ENV", "/dev/null")
        .size(40, 10)
        .spawn()
        .expect("spawn succeeds");
    let child_pgid = pgid_of(session.pid().expect("pid succeeds")).expect("child pgid resolvable");
    let found = wait_found("29372", 10);
    assert!(!found.is_empty(), "escapee never started");
    assert!(!wait_found("29373", 5).is_empty());
    // A pgrep match is NOT the escape: it also fires for the pre-exec
    // `sh -c`, whose script text holds the token. Teardown legitimately
    // kills anything still in the group (kernel SIGHUP to the foreground
    // group on session-leader exit, then the sweep), so finishing before the
    // background job lands in its own group kills the "escapee" and flakes
    // the boundary assert. Wait for the escape itself: a token-matching pid
    // outside the child's process group.
    let escape_dl = deadline(10);
    let escaped = loop {
        let outside: Vec<u32> = pgrep("29372")
            .into_iter()
            .filter(|p| pgid_of(*p).is_some_and(|g| g != child_pgid))
            .collect();
        if !outside.is_empty() || Instant::now() >= escape_dl {
            break outside;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!escaped.is_empty(), "escapee never left the process group");
    let report = Guardian::wrap(session)
        .finish(deadline(10))
        .expect("finish succeeds");
    // Same-group child contained...
    assert!(pgrep("29373").is_empty());
    // ...but the new-group grandchild outlives: the documented boundary.
    let still = pgrep("29372");
    assert_eq!(still, escaped);
    assert!(!GuardianReport::escape_boundary_note().is_empty());
    // Prove the mechanism: the escapee is in a different process group.
    let escapee_pgid = pgid_of(still[0]).expect("pgid_of succeeds");
    assert_ne!(Some(escapee_pgid), report.pgid);
    // Bounded cleanup of the deliberate escapee.
    pkill("29372");
    wait_gone("29372", 5);
}

#[test]
fn guardian_drop_contains() {
    {
        let session = Tui::new(["/bin/sh", "-c", "trap '' TERM; sleep 29374"])
            .env("ENV", "/dev/null")
            .size(40, 10)
            .spawn()
            .expect("spawn succeeds");
        assert!(!wait_found("29374", 5).is_empty());
        let _guardian = Guardian::wrap(session);
    }
    wait_gone("29374", 5);
}

#[test]
fn guardian_exited_child_degrades_without_killing() {
    // Wrap after the child was reaped: identity unresolvable, kill nothing.
    let session = Tui::new(["/bin/sh", "-c", "exit 0"])
        .env("ENV", "/dev/null")
        .size(40, 10)
        .spawn()
        .expect("spawn succeeds");
    session
        .wait_exit(deadline(10), &cancel())
        .expect("wait_exit succeeds");
    let report = Guardian::wrap(session)
        .finish(deadline(5))
        .expect("finish succeeds");
    assert!(report.signalled.is_empty());
    assert!(matches!(
        report.containment,
        Containment::Unknown { .. } | Containment::Full
    ));
}
