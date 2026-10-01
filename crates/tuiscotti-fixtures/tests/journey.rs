//! M2 vertical slice, part 2: real settings-navigation PTY journey.
//!
//! Item 3 (`settings_navigation`): the Rust `menu_fixture --journey`
//! binary (ANSI menu, 3 settings rows, arrow-key navigation, Space
//! toggles, `q` quits with exit = toggled count) runs inside a real PTY
//! ([`tuiscotti::tui::Tui`]). The test snapshots the initial
//! grid, drives Down/Space/Down/Space, pins the toggled markers with
//! [`tuiscotti::locate::Locator`] assertions, screenshots the mid state, quits
//! with `q` (exit code 2), snapshots the final grid, and verifies journal
//! completion plus child teardown (no leaked processes).
//!
//! `INSTA_UPDATE` stays ambient (read-only): Insta exposes no `Settings`
//! switch for the update behavior, and `set_var` is an `unsafe fn` in edition
//! 2024 that cannot be used under the workspace lints. Committed snapshots
//! match, so green-path assertions hold under every mode; run with
//! `INSTA_UPDATE=no` for fail-clean (never auto-bless) or
//! `INSTA_UPDATE=always` to regenerate approvals.

#![cfg(feature = "pty")]

use std::time::{Duration, Instant};

use tuiscotti::locate::Locator;
use tuiscotti::runner::{Journal, JournalStatus, TestContext};
use tuiscotti::tui::{CancelToken, Tui, process_exists};

/// Explicit snapshot dirs: the committed `tests/snapshots` (absolute: the
/// facade's caller-derived default is a *relative* path, which Insta resolves
/// against the facade crate instead of this test). Ambient
/// `TUISCOTTI_SNAPSHOT_DIR` defaulting is replaced by an explicit
/// `tuiscotti::assert::Policy::EvolvingIn`, since `set_var` is unavailable;
/// evidence keeps the default `tuiscotti::assert::evidence_dir`.
fn policy() -> tuiscotti::assert::Policy {
    tuiscotti::assert::Policy::EvolvingIn {
        snapshots: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
        evidence: tuiscotti::assert::evidence_dir(),
    }
}

fn deadline(secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(secs)
}

/// Authoritative path of the `menu_fixture` binary: the runtime
/// environment first (`tuiscotti::runner::resolve_bin`, correct under
/// nextest archive/remap runs), else the compile-time
/// `CARGO_BIN_EXE_menu_fixture` cargo bakes into this test target.
fn menu_bin() -> std::path::PathBuf {
    if let Ok(path) = tuiscotti::runner::resolve_bin("tuiscotti-fixtures", "menu_fixture") {
        return path;
    }
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_menu_fixture"))
}

/// Spawn `menu_fixture --journey`; asserts the binary ships and records
/// the child pid in the journal.
fn spawn_menu(journal: &mut Journal) -> anyhow::Result<(tuiscotti::tui::Session, u32)> {
    let bin = menu_bin();
    assert!(bin.is_file(), "fixture menu binary: {}", bin.display());
    let session = Tui::new([bin.to_string_lossy().into_owned()])
        .arg("--journey")
        .size(48, 12)
        .spawn()?;
    let pid = session.pid().ok_or_else(|| anyhow::anyhow!("child pid"))?;
    journal.append("spawned", &format!("pid {pid}"))?;
    Ok((session, pid))
}

/// Drive Down/Space/Down/Space through the live menu: pins both toggles plus
/// row identities, records navigation in the journal, and returns the
/// mid-journey observation.
fn drive_toggles(
    session: &tuiscotti::tui::Session,
    observe: &mut impl FnMut() -> tuiscotti::Observation,
    journal: &mut Journal,
) -> anyhow::Result<tuiscotti::Observation> {
    session.press("Down")?;
    session.wait_stable(deadline(10), &CancelToken::new())?;
    session.press("Space")?;
    Locator::text("[x]").expect_count(observe, 1, Duration::from_secs(10))?;
    session.press("Down")?;
    session.wait_stable(deadline(10), &CancelToken::new())?;
    session.press("Space")?;
    let checked = Locator::text("[x]").expect_count(observe, 2, Duration::from_secs(10))?;
    assert_eq!(checked.len(), 2);
    let unchecked = Locator::text("[ ]").expect_count(observe, 1, Duration::from_secs(10))?;
    assert_eq!(unchecked.len(), 1);
    // Row identities survive the toggles: each name still unique on screen.
    let mid_obs = session.observe_now()?;
    for name in ["autosave", "line_numbers", "word_wrap"] {
        Locator::text(name)
            .resolve_unique(&mid_obs.screen, mid_obs.revision)
            .map_err(|e| anyhow::anyhow!("row {name:?} unique: {e}"))?;
    }
    journal.append("navigated", "toggled line_numbers + word_wrap")?;
    Ok(mid_obs)
}

#[test]
fn settings_navigation() {
    let policy = policy();
    let ctx = TestContext::current("settings-journey").expect("test context");
    let mut journal = Journal::open(&ctx.journal_path()).expect("open journal");
    journal
        .append("start", "settings-journey")
        .expect("journal start");

    let (session, pid) = spawn_menu(&mut journal).expect("spawn menu fixture");

    // Initial grid: title + 3 unchecked rows, selection on row 0.
    // Content wait, not stability wait: a slow first draw is quiet but
    // blank (freshly linked binary), and `wait_stable` would return rev 0.
    let mut observe = || session.observe_now().expect("observe");
    Locator::text("Settings (space toggles, q quits)")
        .expect_visible(&mut observe, Duration::from_secs(15))
        .expect("title draws");
    let initial = session
        .wait_stable(deadline(10), &CancelToken::new())
        .expect("menu settles");
    assert_eq!(
        Locator::text("[ ]")
            .resolve_obs(&initial)
            .expect("locator")
            .len(),
        3,
        "all rows unchecked initially"
    );
    tuiscotti::assert_snapshot!("journey__settings_initial", &initial.screen, &policy);
    journal.append("snapshotted", "initial").expect("journal");

    // Down/Space/Down/Space: toggle rows 1 and 2 (selection ends on row 2).
    let mid_obs = drive_toggles(&session, &mut observe, &mut journal).expect("drive toggles");

    tuiscotti::assert_screenshot!("journey__settings_mid", &mid_obs.screen, &policy);
    journal.append("snapshotted", "mid").expect("journal");

    // Quit: exit code must equal the toggled count (2).
    session.press("q").expect("send q");
    let final_obs = session
        .expect_exit(deadline(10), &CancelToken::new())
        .expect("child exits")
        .code(2)
        .expect("exit code 2");
    tuiscotti::assert_snapshot!("journey__settings_final", &final_obs.screen, &policy);
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
