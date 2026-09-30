//! F11 bound locators: immediate lookup vs retrying expectation, atomic
//! clicks (split from `tui.rs`; shared helpers live in the root).

use std::time::{Duration, Instant};

use tuiscotti_core::frame::{Cell, Color, Cursor, Mods};
use tuiscotti_core::locate::{LocateError, Locator};
use tuiscotti_core::screen::{CaptureProvenance, CaptureReason, Observation, Screen, TermState};
use tuiscotti_runtime::bound_locator::{ActionError, BoundLocator};
use tuiscotti_runtime::tui::Tui;

use super::{cancel, deadline, rows};

fn dup_count(screen: &Screen) -> usize {
    rows(screen).map_or(0, |r| r.iter().filter(|row| row.contains("dup")).count())
}

#[test]
fn expect_visible_waits_for_delayed_appearance() {
    let s = Tui::new([
        "/bin/sh",
        "-c",
        "sleep 1; printf 'DelayedReady\\n'; sleep 30",
    ])
    .size(40, 8)
    .spawn()
    .expect("spawn succeeds");
    let start = Instant::now();
    let span = s
        .get_by_text("DelayedReady")
        .expect_visible()
        .expect("delayed target appears");
    let waited = start.elapsed();
    assert!(span.text.contains("DelayedReady"), "span: {span}");
    assert!(
        waited >= Duration::from_secs(1),
        "expect_visible must wait for the target, returned after {waited:?}"
    );
    assert!(
        waited < BoundLocator::DEFAULT_EXPECT_VISIBLE,
        "returned before the default deadline: {waited:?}"
    );
    s.close().expect("close succeeds");
}

#[test]
fn duplicate_matches_fail_unique_lookup_and_name_ambiguity_on_timeout() {
    let s = Tui::new(["/bin/sh", "-c", "printf 'dup\\noth\\ndup\\n'; sleep 30"])
        .size(40, 8)
        .spawn()
        .expect("spawn succeeds");
    // Both copies must be on screen before asserting ambiguity.
    s.wait_predicate(|o| dup_count(&o.screen) == 2, deadline(10), &cancel())
        .expect("both dup rows draw");
    let err = s
        .get_by_text("dup")
        .visible_now()
        .expect_err("duplicates are not unique");
    match err {
        ActionError::Locate(LocateError::Ambiguous { matches }) => {
            assert_eq!(matches.len(), 2, "{matches:?}");
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
    // The retrying form waits out its bound, then names the last state.
    let err = s
        .get_by_text("dup")
        .expect_visible_within(Duration::from_millis(150))
        .expect_err("duplicates never become unique");
    match err {
        ActionError::Locate(LocateError::Timeout { reason, .. }) => {
            assert!(reason.contains("ambiguous"), "{reason}");
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
    s.close().expect("close succeeds");
}

#[test]
fn permanent_errors_fail_without_waiting() {
    let s = Tui::new(["/bin/sh", "-c", "printf 'Ready\\n'; sleep 30"])
        .size(40, 8)
        .spawn()
        .expect("spawn succeeds");
    for bad in ["", "has\nnewline"] {
        let start = Instant::now();
        let err = s
            .get_by_text(bad)
            .expect_visible_within(Duration::from_secs(5))
            .expect_err("usage error must fail");
        assert!(
            matches!(err, ActionError::Locate(LocateError::Usage(_))),
            "{bad:?}: {err:?}"
        );
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "{bad:?}: permanent error waited out the deadline"
        );
    }
    s.close().expect("close succeeds");
}

/// A session whose emulator has mouse click + SGR reporting on (the bytes
/// come from the app, so the live `TermMode` gates open): clicks deliver.
fn mouse_session() -> Result<tuiscotti_runtime::tui::Session, tuiscotti_runtime::tui::TuiError> {
    Tui::new([
        "/bin/sh",
        "-c",
        "printf '\\033[?1000h\\033[?1006hReady\\n'; sleep 30",
    ])
    .size(40, 8)
    .spawn()
}

#[test]
fn click_delivers_at_the_resolved_revision() {
    let s = mouse_session().expect("spawn succeeds");
    s.get_by_text("Ready")
        .expect_visible()
        .expect("target draws");
    let before = s.revision();
    let span = s.get_by_text("Ready").click().expect("click delivers");
    assert!(span.text.contains("Ready"), "span: {span}");
    // One worker step: the revision acted on is live (never older than the
    // session had, never from the future). Exact equality is NOT asserted:
    // delivering input legitimately advances the session (PTY echo and any
    // app output publish new revisions after the click resolved).
    assert!(span.revision >= before, "span: {span}, before: {before}");
    assert!(
        span.revision <= s.revision(),
        "span: {span}, current: {}",
        s.revision()
    );
    s.close().expect("close succeeds");
}

#[test]
fn click_tracks_the_target_across_injected_updates() {
    let s = mouse_session().expect("spawn succeeds");
    s.get_by_text("Ready")
        .expect_visible()
        .expect("target draws");
    let first_rev = s.revision();
    // Inject updates immediately around every delivery: each resize
    // publishes a new revision from another thread while this thread
    // clicks. Atomic resolve-at-delivery keeps every click fresh.
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for i in 0..6 {
                let cols = if i % 2 == 0 { 41 } else { 40 };
                s.resize(cols, 8).expect("resize succeeds");
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        for _ in 0..8 {
            let span = s.get_by_text("Ready").click().expect("click delivers");
            assert!(span.text.contains("Ready"), "span: {span}");
        }
    });
    let last_rev = s.revision();
    assert!(
        last_rev > first_rev,
        "updates were injected during the clicks ({first_rev} -> {last_rev})"
    );
    s.close().expect("close succeeds");
}

fn obs_with_text(row: &str, revision: u64) -> Result<Observation, String> {
    let mut cells = Vec::with_capacity(20 * 4);
    for y in 0..4u16 {
        for x in 0..20u16 {
            let ch = if y == 0 {
                row.chars().nth(x as usize).unwrap_or(' ')
            } else {
                ' '
            };
            cells.push(Cell {
                x,
                y,
                symbol: ch.to_string(),
                width: 1,
                continuation: false,
                fg: Color::Default,
                bg: Color::Default,
                mods: Mods::default(),
                underline_color: Color::Default,
            });
        }
    }
    let screen =
        Screen::validate(20, 4, 0, 0, cells, Cursor::default()).map_err(|e| e.to_string())?;
    Ok(Observation::new(
        screen,
        revision,
        CaptureReason::Manual,
        TermState::default(),
        CaptureProvenance::new(0, None, None, 0),
    ))
}

#[test]
fn click_refuses_a_target_shifted_by_resize() {
    // Live-session StaleTarget refusal: resolve at one revision, shift the
    // target with a resize (deterministic: resize blocks for its worker
    // reply, which always advances the revision), then delivery against the
    // new revision must fail StaleTarget with the sink never running.
    let s = mouse_session().expect("spawn succeeds");
    s.get_by_text("Ready")
        .expect_visible()
        .expect("target draws");
    let before = s.observe_now().expect("observe at N");
    let pending = Locator::text("Ready")
        .prepare_action(&before)
        .expect("target ready at N");
    s.resize(41, 8).expect("resize shifts the target");
    let after = s.observe_now().expect("observe at N+1");
    assert!(
        after.revision > before.revision,
        "resize must advance the revision ({} -> {})",
        before.revision,
        after.revision
    );
    let mut delivered = 0;
    let err = pending
        .click(&after, &mut |_| delivered += 1)
        .expect_err("shifted target must be refused");
    assert_eq!(
        err,
        LocateError::StaleTarget {
            expected: before.revision,
            current: after.revision,
        },
        "wrong refusal"
    );
    assert_eq!(delivered, 0, "the sink never ran");
    s.close().expect("close succeeds");
}

/// Mouse session with PTY echo off: without this, each click's SGR bytes
/// echo back as literal `^[[<...` cells (ECHOCTL) and a long click series
/// buries the target under its own echo. Full-screen apps run echoless;
/// the churn test needs the same to click 100 times at one target.
fn quiet_mouse_session() -> Result<tuiscotti_runtime::tui::Session, tuiscotti_runtime::tui::TuiError>
{
    Tui::new([
        "/bin/sh",
        "-c",
        "stty -echo; printf '\\033[?1000h\\033[?1006hReady\\n'; sleep 30",
    ])
    .size(40, 8)
    .spawn()
}

#[test]
fn clicks_stay_fresh_under_resize_churn() {
    // Worker-atomicity proof (F11 revert detector): the owning worker
    // resolves each click at its own current revision and delivers press +
    // release with no interleaving op, so EVERY click succeeds fresh no
    // matter how many resizes land around it. The old two-reads shape fails
    // here: any revision shift between its two observations surfaces as a
    // spurious StaleTarget. Bounded fixed counts, no sleeps: resize() and
    // click() both block for their worker replies; the scope join is the
    // only sync.
    const CLICKS: usize = 100;
    const RESIZES: usize = 600;
    let s = quiet_mouse_session().expect("spawn succeeds");
    s.get_by_text("Ready")
        .expect_visible()
        .expect("target draws");
    let first_rev = s.revision();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for i in 0..RESIZES {
                let cols = if i % 2 == 0 { 41 } else { 40 };
                s.resize(cols, 8).expect("resize succeeds");
            }
        });
        for _ in 0..CLICKS {
            // Success-only assertion: keeps compiling against the old
            // `Result<(), _>` click shape so the revert trial fails
            // behaviorally (StaleTarget), not at build time.
            let _span = s
                .get_by_text("Ready")
                .click()
                .expect("atomic click delivers under churn");
        }
    });
    assert!(
        s.revision() >= first_rev + RESIZES as u64,
        "churn landed during the clicks ({} -> {})",
        first_rev,
        s.revision()
    );
    s.close().expect("close succeeds");
}

#[test]
fn pending_action_refuses_an_update_injected_before_delivery() {
    // The detached primitive behind the atomic click: readiness at one
    // revision, an update injected before delivery, refusal without effect.
    let before = obs_with_text("ClickMe", 7).expect("screen");
    let pending = Locator::text("ClickMe")
        .prepare_action(&before)
        .expect("target ready at rev 7");
    let after = obs_with_text("ClickMe", 8).expect("screen");
    let mut delivered = 0;
    let err = pending
        .click(&after, &mut |_| delivered += 1)
        .expect_err("stale delivery must be refused");
    assert!(
        matches!(
            err,
            LocateError::StaleTarget {
                expected: 7,
                current: 8
            }
        ),
        "{err:?}"
    );
    assert_eq!(delivered, 0, "the sink never ran");
}
