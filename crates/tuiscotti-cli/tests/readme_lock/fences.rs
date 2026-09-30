//! README fences: quickstart, bulk, accept, interactive TUI (split from `readme_lock.rs`; shared helpers live in the root).

use super::home_frame;
use tuiscotti::snapshot::{Status, Store};
use tuiscotti::{Profile, VENDORED_FACES};

// ---------------------------------------------------------------------------
// Quick start fence: pure view test through Store::check.
// ---------------------------------------------------------------------------

#[test]
fn readme_quickstart_pure_view() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store = Store::new(&tmp.path().join("visual"));
    let profile = Profile::default_profile();
    let frame = home_frame();
    // First run fails with missing-approval (fail-closed), actuals on disk.
    let outcome = store
        .check("home", &frame, &profile, &VENDORED_FACES, 1.0)
        .expect("check");
    assert_eq!(outcome.status.as_str(), "missing-approval");
    assert!(outcome.ensure_matched().is_err());
    assert!(outcome.actual_frame.is_file());
    assert!(outcome.actual_png.is_file());
    // Explicit accept, then the gate passes.
    store.accept("home").expect("accept");
    let outcome = store
        .check("home", &frame, &profile, &VENDORED_FACES, 1.0)
        .expect("re-check");
    outcome.ensure_matched().expect("gate passes after accept");
}

// ---------------------------------------------------------------------------
// Bulk fence: profile.renderer + check_with + report_with.
// ---------------------------------------------------------------------------

#[test]
fn readme_bulk_renderer_reuse() {
    fn bulk(
        store: &tuiscotti::snapshot::Store,
        profile: &tuiscotti::Profile,
        frame: &tuiscotti::Frame,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut renderer = profile.renderer(&tuiscotti::VENDORED_FACES)?;
        let outcome = store.check_with(&mut renderer, "home", frame, 1.0)?;
        outcome.ensure_matched()?;
        let report = store.report_with(&mut renderer, 1.0, "my suite")?;
        assert_eq!(report.failed(), 0);
        Ok(())
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let store = Store::new(&tmp.path().join("visual"));
    let profile = Profile::default_profile();
    let frame = home_frame();
    store.accept("home").expect_err("nothing to accept yet");
    // Seed approval, then the bulk fence passes end to end.
    let first = store
        .check("home", &frame, &profile, &VENDORED_FACES, 1.0)
        .expect("check");
    assert!(first.ensure_matched().is_err());
    store.accept("home").expect("accept");
    bulk(&store, &profile, &frame).expect("bulk fence");
    assert!(store.root().join("report.html").is_file());
}

// ---------------------------------------------------------------------------
// Accept fence: store.accept + actual_names loop.
// ---------------------------------------------------------------------------

#[test]
fn readme_accept_flow() {
    fn accept_reviewed(
        store: &tuiscotti::snapshot::Store,
    ) -> Result<(), tuiscotti::snapshot::SnapshotError> {
        store.accept("home")?;
        for name in store.actual_names()? {
            store.accept(&name)?;
        }
        Ok(())
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let store = Store::new(&tmp.path().join("visual"));
    let profile = Profile::default_profile();
    let frame = home_frame();
    let first = store
        .check("home", &frame, &profile, &VENDORED_FACES, 1.0)
        .expect("check");
    assert!(matches!(first.status, Status::MissingApproval));
    accept_reviewed(&store).expect("accept reviewed");
    let outcome = store
        .check("home", &frame, &profile, &VENDORED_FACES, 1.0)
        .expect("re-check");
    outcome.ensure_matched().expect("gate passes after accept");
}

// ---------------------------------------------------------------------------
// Interactive fence: Tui::new / wait_predicate / press / mouse_wheel /
// wait_stable / frame_from_screen / close. Mirrors the README statements
// against a runnable shell instead of ./my-tui.
// ---------------------------------------------------------------------------

#[test]
#[cfg(feature = "pty")]
fn readme_interactive_tui() {
    use std::time::{Duration, Instant};
    use tuiscotti::tui::{CancelToken, Tui};

    // The documented chord shape parses: modifiers + special keys, +-joined.
    let (key, mods) = tuiscotti::tui::parse_chord("ctrl+Up").expect("parse chord");
    assert_eq!(key, tuiscotti::tui::Key::Up);
    assert!(mods.ctrl);

    let s = Tui::new(["/bin/sh", "-c", "printf 'Ready\\n'; sleep 30"])
        .size(120, 40)
        .spawn()
        .expect("spawn shell");
    let cancel = CancelToken::new();
    let _obs = s
        .wait_predicate(
            |o| tuiscotti::proto::screen_text(&o.screen).contains("Ready"),
            Instant::now() + Duration::from_secs(10),
            &cancel,
        )
        .expect("wait ready");
    s.press("ctrl+Up").expect("press chord");
    s.send_text("hello").expect("send text");
    // Mouse input without app-enabled reporting fails closed, never drops.
    assert!(matches!(
        s.mouse_wheel(
            tuiscotti::tui::Wheel::Up,
            10,
            5,
            tuiscotti::tui::MouseMods::NONE
        ),
        Err(tuiscotti::tui::TuiError::ModeNotEnabled(_))
    ));
    let _settled = s
        .wait_stable(Instant::now() + Duration::from_secs(10), &cancel)
        .expect("wait stable");
    let frame = tuiscotti::assert::frame_from_screen(&s.snapshot().expect("snapshot"));
    assert_eq!((frame.cols, frame.rows), (120, 40));
    s.close().expect("close");
}
