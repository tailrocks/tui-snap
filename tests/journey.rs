//! M2 vertical slice, part 2: real settings-navigation PTY journey.
//!
//! Item 3 (`settings_navigation`): `/bin/sh` runs the committed
//! `tests/fixtures/journey/menu.sh` fixture (ANSI menu, 3 settings rows,
//! arrow-key navigation, Space toggles, `q` quits with exit = toggled count)
//! inside a real PTY ([`tuisnap::tui::Tui`]). The test snapshots the initial
//! grid, drives Down/Space/Down/Space, pins the toggled markers with
//! [`tuisnap::locate::Locator`] assertions, screenshots the mid state, quits
//! with `q` (exit code 2), snapshots the final grid, and verifies journal
//! completion plus child teardown (no leaked processes).
//!
//! `INSTA_UPDATE=no` semantics: Insta exposes no `Settings` switch for the
//! update behavior (see `src/assert.rs` docs), so the freeze is the env var,
//! defaulted in-process before the first Insta call (Insta memoizes tool
//! config per binary). An explicit external value (e.g. the one
//! `INSTA_UPDATE=always` generation run) is honored, never overridden — CI
//! with an unset variable can never auto-bless.

#![cfg(feature = "pty")]

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tuisnap::locate::Locator;
use tuisnap::runner::{Journal, JournalStatus, TestContext};
use tuisnap::tui::{process_exists, CancelToken, Tui};

/// Insta environment, fixed exactly once per test binary (env is
/// process-global and tests run in parallel threads, so per-test mutation
/// would race; the values are identical for every caller, making this benign).
/// Externally set values always win (generation runs, custom dirs).
fn freeze_insta_updates() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        if std::env::var_os("INSTA_UPDATE").is_none() {
            std::env::set_var("INSTA_UPDATE", "no");
        }
        if std::env::var_os(tuisnap::assert::SNAPSHOT_DIR_ENV).is_none() {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
            std::env::set_var(tuisnap::assert::SNAPSHOT_DIR_ENV, &dir);
        }
    });
}

fn deadline(secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(secs)
}

fn menu_script() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/journey/menu.sh")
}

#[test]
fn settings_navigation() {
    freeze_insta_updates();
    let ctx = TestContext::current("settings-journey").expect("test context");
    let mut journal = Journal::open(&ctx.journal_path()).expect("open journal");
    journal.append("start", "settings-journey").expect("journal start");

    let script = menu_script();
    assert!(script.is_file(), "fixture menu script: {}", script.display());

    let session = Tui::new(["/bin/sh", &script.to_string_lossy()])
        .size(48, 12)
        .spawn()
        .expect("spawn menu fixture");
    let pid = session.pid().expect("child pid");
    journal
        .append("spawned", &format!("pid {pid}"))
        .expect("journal spawned");

    // Initial grid: title + 3 unchecked rows, selection on row 0.
    let initial = session
        .wait_stable(deadline(10), &CancelToken::new())
        .expect("menu settles");
    assert!(
        Locator::text("Settings (space toggles, q quits)")
            .present_now(&initial)
            .expect("locator"),
        "title visible (rev {})",
        initial.revision
    );
    assert_eq!(
        Locator::text("[ ]")
            .resolve_obs(&initial)
            .expect("locator")
            .len(),
        3,
        "all rows unchecked initially"
    );
    tuisnap::assert_snapshot!("journey__settings_initial", &initial.screen);
    journal.append("snapshotted", "initial").expect("journal");

    // Down/Space/Down/Space: toggle rows 1 and 2 (selection ends on row 2).
    let mut observe = || session.observe_now().expect("observe");
    session.press("Down").expect("send Down");
    session
        .wait_stable(deadline(10), &CancelToken::new())
        .expect("settle after Down");
    session.press("Space").expect("send Space");
    Locator::text("[x]")
        .expect_count(&mut observe, 1, Duration::from_secs(10))
        .expect("first toggle visible");
    session.press("Down").expect("send Down");
    session
        .wait_stable(deadline(10), &CancelToken::new())
        .expect("settle after Down");
    session.press("Space").expect("send Space");
    let checked = Locator::text("[x]")
        .expect_count(&mut observe, 2, Duration::from_secs(10))
        .expect("both toggles visible");
    assert_eq!(checked.len(), 2);
    let unchecked = Locator::text("[ ]")
        .expect_count(&mut observe, 1, Duration::from_secs(10))
        .expect("one row left unchecked");
    assert_eq!(unchecked.len(), 1);
    // Row identities survive the toggles: each name still unique on screen.
    let mid_obs = session.observe_now().expect("observe mid");
    for name in ["autosave", "line_numbers", "word_wrap"] {
        Locator::text(name)
            .resolve_unique(&mid_obs.screen, mid_obs.revision)
            .unwrap_or_else(|e| panic!("row {name:?} unique: {e}"));
    }
    journal
        .append("navigated", "toggled line_numbers + word_wrap")
        .expect("journal");

    tuisnap::assert_screenshot!("journey__settings_mid", &mid_obs.screen);
    journal.append("snapshotted", "mid").expect("journal");

    // Quit: exit code must equal the toggled count (2).
    session.press("q").expect("send q");
    let final_obs = session
        .expect_exit(deadline(10), &CancelToken::new())
        .expect("child exits")
        .code(2)
        .expect("exit code 2");
    tuisnap::assert_snapshot!("journey__settings_final", &final_obs.screen);
    journal.append("exited", "code 2").expect("journal");

    // Cleanup: graceful finish reaps the child; nothing leaks.
    let status = session.finish(deadline(5)).expect("finish session");
    assert_eq!(status.code(), 2);
    assert!(
        !process_exists(pid),
        "child {pid} reaped: no leaked processes"
    );

    journal.complete("pass").expect("journal complete");
    match Journal::status(ctx.scratch_dir()) {
        JournalStatus::Complete { status } => assert_eq!(status, "pass"),
        JournalStatus::Incomplete { reason } => panic!("journal incomplete: {reason}"),
    }
}
