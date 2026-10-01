//! Interaction contracts: real PTY journeys plus piped projections.
//!
//! The `*_fixture` binaries (built once by the outer build) are resolved
//! authoritatively ([`live::fixture_bin`]: runtime env, else the
//! compile-time `CARGO_BIN_EXE_<name>` — no probing, no nested cargo) and
//! driven over a real PTY through the public `tuiscotti` API. Live screens
//! project through the same six formats as pure views; piped `--print` /
//! `--emit-raw` runs project through the pipe decoder, including raw
//! invalid UTF-8.

#![cfg(feature = "pty")]

#[path = "common/mod.rs"]
mod common;

#[path = "common/capture.rs"]
mod capture;

#[path = "common/live.rs"]
mod live;

use std::time::{Duration, Instant};
use tuiscotti::locate::Locator;
use tuiscotti::tui::{CancelToken, Tui, process_exists};
use tuiscotti_fixtures::driver::Scenario;
use tuiscotti_fixtures::views::Theme;

/// Deadline `secs` in the future.
fn deadline(secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(secs)
}

/// Spawn a fixture binary at `cols`×`rows` with `--theme dark`.
fn spawn_fixture(name: &str, cols: u16, rows: u16) -> anyhow::Result<tuiscotti::tui::Session> {
    let bin = live::fixture_bin(name)?;
    assert!(
        bin.is_file(),
        "authoritative binary present: {}",
        bin.display()
    );
    Ok(Tui::new([bin.to_string_lossy().into_owned()])
        .arg("--theme")
        .arg("dark")
        .size(cols, rows)
        .spawn()?)
}

/// Live [`Screen`](tuiscotti::Screen) → canonical frame under the live profile name.
fn live_frame(screen: &tuiscotti::Screen) -> tuiscotti::Frame {
    tuiscotti_render::render::frame_from_screen(screen, "live-pty")
}

/// Assert every format validator passes on a live capture bundle.
fn assert_live_bundle(bundle: &tuiscotti_render::formats::CaptureBundle) -> anyhow::Result<()> {
    use tuiscotti_render::formats::{
        assert_no_escapes, assert_normalized_sgr, assert_opaque_rgb, assert_seven_bit,
        assert_static_offline,
    };
    assert_seven_bit(&bundle.ascii.text)?;
    assert_no_escapes(&bundle.txt)?;
    assert_normalized_sgr(&bundle.ansi)?;
    assert_opaque_rgb(&bundle.png)?;
    assert_static_offline(&bundle.html)?;
    assert!(
        !bundle.generation.id.is_empty(),
        "live generation identified"
    );
    Ok(())
}

#[test]
fn fixture_binaries_resolve_authoritatively() {
    for name in ["menu_fixture", "streams_fixture", "protocol_fixture"] {
        let bin = live::fixture_bin(name).expect("fixture binary resolves");
        assert!(bin.is_file(), "{name} resolves to a built file");
        // The resolved binary is ours and fresh: unknown flags exit 2.
        let out = tuiscotti::command::Command::new(&bin)
            .arg("--bogus-flag")
            .run();
        assert_eq!(out.code(), Some(2), "{name} rejects unknown flags");
    }
}

#[test]
fn menu_journey_toggle_error_and_quit() {
    let session = spawn_fixture("menu_fixture", 48, 12).expect("spawn fixture");
    let pid = session.pid().expect("child pid");
    let mut observe = || session.observe_now().expect("observe");
    // Content waits, not stability waits: a slow first draw is quiet but blank.
    Locator::text("Settings (space toggles, q quits, 0 on)")
        .expect_visible(&mut observe, Duration::from_secs(15))
        .expect("title draws");
    Locator::text("[ ]")
        .expect_count(&mut observe, 3, Duration::from_secs(10))
        .expect("3 rows");
    // Down/Space/Down/Space toggles rows 1 and 2 (selection ends on row 2).
    session.press("Down").expect("Down");
    session
        .wait_stable(deadline(10), &CancelToken::new())
        .expect("settle");
    session.press("Space").expect("Space");
    Locator::text("[x]")
        .expect_count(&mut observe, 1, Duration::from_secs(10))
        .expect("toggle 1");
    session.press("Down").expect("Down");
    session
        .wait_stable(deadline(10), &CancelToken::new())
        .expect("settle");
    session.press("Space").expect("Space");
    Locator::text("[x]")
        .expect_count(&mut observe, 2, Duration::from_secs(10))
        .expect("toggle 2");
    // Disabled row: Space rings an error popup, Esc dismisses it.
    for _ in 0..3 {
        session.press("Down").expect("Down");
    }
    session
        .wait_stable(deadline(10), &CancelToken::new())
        .expect("settle");
    session.press("Space").expect("Space");
    Locator::text("is disabled")
        .expect_visible(&mut observe, Duration::from_secs(10))
        .expect("disabled error");
    session.press("Esc").expect("Esc");
    let settled = session
        .wait_stable(deadline(10), &CancelToken::new())
        .expect("settle");
    assert!(
        !Locator::text("is disabled")
            .present_now(&settled)
            .expect("locator")
    );
    // Pure view and live TUI share rendering: identical labels both sides.
    let mid = session.snapshot().expect("snapshot");
    let live_txt = live_frame(&mid).text();
    let pure_txt = common::menu_frame(48, 12, Theme::Dark, Scenario::Demo).text();
    for name in ["autosave", "line_numbers", "word_wrap"] {
        assert!(pure_txt.contains(name), "pure renders {name}");
        assert!(live_txt.contains(name), "live renders {name}");
    }
    // Live capture exports every format plus one stable generation.
    let mut renderer = capture::renderer().expect("renderer");
    let profile_name = renderer.profile().name.clone();
    let bundle =
        tuiscotti_render::formats::capture_all(&mut renderer, &live_frame(&mid), "menu live")
            .expect("capture");
    assert_live_bundle(&bundle).expect("live bundle valid");
    let again = session.snapshot().expect("snapshot");
    let gen2 = tuiscotti_render::formats::generation_for(&live_frame(&again), &profile_name);
    assert_eq!(bundle.generation, gen2, "stable screen, stable generation");
    // Quit cleanly with no leaked process.
    session.press("q").expect("q");
    session
        .expect_exit(deadline(10), &CancelToken::new())
        .expect("exits")
        .success()
        .expect("exit 0");
    let status = session.finish(deadline(5)).expect("finish");
    assert!(status.success());
    assert!(!process_exists(pid), "child reaped");
}

#[test]
fn streams_journey_scroll_resize_and_quit() {
    let session = spawn_fixture("streams_fixture", 60, 12).expect("spawn fixture");
    let pid = session.pid().expect("child pid");
    let mut observe = || session.observe_now().expect("observe");
    Locator::text("Streams")
        .expect_visible(&mut observe, Duration::from_secs(15))
        .expect("draws");
    Locator::text("tail: end of deterministic log")
        .expect_visible(&mut observe, Duration::from_secs(10))
        .expect("tail visible");
    Locator::text("日本語")
        .expect_visible(&mut observe, Duration::from_secs(10))
        .expect("CJK");
    // Home leaves the tail: the head of the log becomes visible.
    session.press("Home").expect("Home");
    Locator::text("trace: plain default color")
        .expect_visible(&mut observe, Duration::from_secs(10))
        .expect("scrolled to head");
    // Resize reflows the live view without breaking the frame.
    session.resize(80, 20).expect("resize");
    let resized = session
        .wait_stable(deadline(10), &CancelToken::new())
        .expect("settle");
    assert_eq!((resized.screen.cols(), resized.screen.rows()), (80, 20));
    let frame = live_frame(&resized.screen);
    frame.validate().expect("resized frame valid");
    let bundle = tuiscotti_render::formats::capture_all(
        &mut capture::renderer().expect("renderer"),
        &frame,
        "streams live",
    )
    .expect("capture");
    assert_live_bundle(&bundle).expect("live bundle valid");
    assert!(bundle.txt.contains("Streams"), "content survives resize");
    session.press("q").expect("q");
    session
        .expect_exit(deadline(10), &CancelToken::new())
        .expect("exits")
        .success()
        .expect("exit 0");
    let status = session.finish(deadline(5)).expect("finish");
    assert!(status.success());
    assert!(!process_exists(pid), "child reaped");
}

#[test]
fn protocol_journey_paste_focus_resize_and_quit() {
    let session = spawn_fixture("protocol_fixture", 50, 12).expect("spawn fixture");
    let pid = session.pid().expect("child pid");
    let mut observe = || session.observe_now().expect("observe");
    Locator::text("paste=on")
        .expect_visible(&mut observe, Duration::from_secs(15))
        .expect("draws");
    // Pastes from the committed data file land in the echo area verbatim.
    let raw = String::from_utf8(common::read_data("protocol-pastes.txt").expect("fixture data"))
        .expect("utf8");
    let payloads: Vec<&str> = raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .collect();
    assert!(payloads.len() >= 2);
    for payload in &payloads[..2] {
        session.paste(payload).expect("paste");
        Locator::text(*payload)
            .expect_visible(&mut observe, Duration::from_secs(10))
            .expect("paste echoed");
    }
    // Focus tracking, raw typing, and resize all reach the model.
    session.focus_out().expect("focus out");
    Locator::text("blurred")
        .expect_visible(&mut observe, Duration::from_secs(10))
        .expect("blur");
    session.focus_in().expect("focus in");
    Locator::text("focused")
        .expect_visible(&mut observe, Duration::from_secs(10))
        .expect("focus");
    session.send_text("Z").expect("type");
    Locator::text("Z")
        .expect_visible(&mut observe, Duration::from_secs(10))
        .expect("typed");
    session.resize(60, 16).expect("resize");
    Locator::text("size=60x16")
        .expect_visible(&mut observe, Duration::from_secs(10))
        .expect("resize logged");
    let shot = session.snapshot().expect("snapshot");
    let frame = live_frame(&shot);
    let bundle = tuiscotti_render::formats::capture_all(
        &mut capture::renderer().expect("renderer"),
        &frame,
        "protocol live",
    )
    .expect("capture");
    assert_live_bundle(&bundle).expect("live bundle valid");
    session.press("q").expect("q");
    session
        .expect_exit(deadline(10), &CancelToken::new())
        .expect("exits")
        .success()
        .expect("exit 0");
    let status = session.finish(deadline(5)).expect("finish");
    assert!(status.success());
    assert!(!process_exists(pid), "child reaped");
}

#[test]
fn piped_print_projections_are_clean() {
    for (name, marker) in [
        ("menu_fixture", "menu_fixture summary"),
        ("streams_fixture", "streams_fixture summary"),
        ("protocol_fixture", "protocol_fixture summary"),
    ] {
        let out = tuiscotti::command::Command::new(
            live::fixture_bin(name).expect("fixture binary resolves"),
        )
        .arg("--print")
        .run();
        assert!(out.success(), "{name} --print exits 0: {out:?}");
        let pipe = tuiscotti_render::formats::pipe_projection(&out.stdout, 65536).expect("project");
        assert!(!pipe.truncated, "{name} fits the bound");
        assert_eq!(pipe.replacements, 0, "{name} prints valid UTF-8");
        assert!(!pipe.lossy());
        assert!(pipe.text.contains(marker));
    }
    // Menu content rides the pipe: every committed item label present.
    let out = tuiscotti::command::Command::new(
        live::fixture_bin("menu_fixture").expect("fixture binary resolves"),
    )
    .arg("--print")
    .run();
    let pipe = tuiscotti_render::formats::pipe_projection(&out.stdout, 65536).expect("project");
    for label in [
        "autosave",
        "line_numbers",
        "word_wrap",
        "日本語モード",
        "legacy_mode",
    ] {
        assert!(pipe.text.contains(label), "pipe carries {label}");
    }
}

#[test]
fn piped_raw_invalid_utf8_is_accounted() {
    let run = || {
        tuiscotti::command::Command::new(
            live::fixture_bin("streams_fixture").expect("fixture binary resolves"),
        )
        .arg("--emit-raw")
        .run()
    };
    let first = run();
    assert!(first.success());
    let strict = tuiscotti_render::formats::pipe_strict(&first.stdout).expect_err("strict rejects");
    assert_eq!(strict.offset, Some(23), "first bad byte located");
    let lossy = tuiscotti_render::formats::pipe_projection(&first.stdout, 65536).expect("project");
    assert_eq!(lossy.replacements, 3, "every invalid sequence counted");
    assert!(!lossy.truncated);
    assert!(lossy.text.contains("valid-cjk"), "valid content intact");
    let second = run();
    let again = tuiscotti_render::formats::pipe_projection(&second.stdout, 65536).expect("project");
    assert_eq!(lossy.id, again.id, "pipe generation stable");
    let cut = tuiscotti_render::formats::pipe_projection(&first.stdout, 10).expect("project");
    assert!(cut.truncated, "small bound cuts explicitly");
}

#[test]
fn missing_and_corrupt_approvals_are_explicit() {
    let scratch = live::scratch_dir("approvals").expect("scratch dir");
    // Missing approval: an explicit IO error, never a panic or a pass.
    assert!(std::fs::read(scratch.join("no-such-baseline.txt")).is_err());
    // Positive control: a faithful copy matches byte for byte.
    let expected = capture::read_expected("menu-demo-40x10.txt").expect("committed baseline");
    std::fs::write(scratch.join("menu-demo-40x10.txt"), &expected).expect("stage");
    let staged = std::fs::read_to_string(scratch.join("menu-demo-40x10.txt")).expect("read");
    assert_eq!(staged, expected);
    // Corrupt approvals fail closed with explicit causes.
    std::fs::write(scratch.join("bad.json"), "{not json").expect("stage");
    let bad = std::fs::read_to_string(scratch.join("bad.json")).expect("read");
    assert!(tuiscotti_render::formats::parse_canonical(&bad).is_err());
    let mut wrong = serde_json::from_str::<serde_json::Value>(
        &common::menu_frame(10, 4, Theme::Dark, Scenario::Empty).to_json(),
    )
    .expect("json");
    wrong["version"] = serde_json::Value::from(1);
    assert!(tuiscotti_render::formats::parse_canonical(&wrong.to_string()).is_err());
    // Mixed generations never compare equal.
    let menu = common::menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    let streams = common::streams_frame(60, 12, Theme::Dark, false);
    let a = tuiscotti_render::formats::generation_for(&menu, "test");
    let b = tuiscotti_render::formats::generation_for(&streams, "test");
    assert!(!tuiscotti_render::formats::generations_match(&a, &b));
    tuiscotti_render::formats::require_same_generation(&a, &b).expect_err("mixed");
}
