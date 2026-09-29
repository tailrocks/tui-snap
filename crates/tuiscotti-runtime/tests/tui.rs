//! PTY session runtime tests (backlog R06-R11-core): real PTY, real
//! processes (`/bin/cat`, `/bin/sh`, `/bin/sleep`), bounded timeouts.

#![cfg(feature = "pty")]

use std::time::{Duration, Instant};

use tuiscotti_core::screen::Screen;
use tuiscotti_runtime::tui::{
    CancelToken, Key, KeyMods, MouseButton, MouseMods, Signal, TerminalProfile, Tui, TuiError,
    WaitError, Wheel, parse_chord, process_exists,
};

fn deadline(secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(secs)
}

fn cancel() -> CancelToken {
    CancelToken::new()
}

/// Plain-text rows of a screen (trailing blanks trimmed per row).
fn rows(screen: &Screen) -> Vec<String> {
    let mut out = Vec::with_capacity(screen.rows() as usize);
    for y in 0..screen.rows() {
        let mut s = String::new();
        for x in 0..screen.cols() {
            let c = screen
                .get(x, y)
                .unwrap_or_else(|| panic!("missing cell {x},{y}"));
            if !c.continuation {
                s.push_str(&c.symbol);
            }
        }
        out.push(s.trim_end().to_string());
    }
    out
}

fn contains(screen: &Screen, needle: &str) -> bool {
    rows(screen).iter().any(|r| r.contains(needle))
}

#[test]
fn spawn_and_observe() {
    let mut s = Tui::new(["/bin/cat"])
        .size(40, 10)
        .spawn()
        .expect("spawn succeeds");
    let obs = s.observe_now().expect("observe_now succeeds");
    assert_eq!(obs.screen.cols(), 40);
    assert_eq!(obs.screen.rows(), 10);
    assert!(obs.screen.cursor().visible);
    // Modes always known; title unknown until the app sets one.
    assert!(obs.state.modes.is_known());
    assert!(obs.state.palette.is_known());
    assert!(obs.state.bells.is_known());
    s.close().expect("close succeeds");
}

#[test]
fn typed_input_echoes() {
    let mut s = Tui::new(["/bin/cat"])
        .size(60, 12)
        .spawn()
        .expect("spawn succeeds");
    s.send_text("hello-tui\n").expect("send_text succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "hello-tui"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "hello-tui"));
    s.close().expect("close succeeds");
}

#[test]
fn chord_press_sends_key() {
    let mut s = Tui::new(["/bin/cat"])
        .size(60, 12)
        .spawn()
        .expect("spawn succeeds");
    s.send_text("hi").expect("send_text succeeds");
    s.press("Enter").expect("press succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "hi"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "hi"));
    s.close().expect("close succeeds");
}

#[test]
fn chord_parser_shapes() {
    let (k, m) = parse_chord("Ctrl+P").expect("parse_chord succeeds");
    assert_eq!(k, Key::Char('P'));
    assert_eq!(m, KeyMods::CTRL);
    let (k, m) = parse_chord("alt+Enter").expect("parse_chord succeeds");
    assert_eq!(k, Key::Enter);
    assert_eq!(m, KeyMods::ALT);
    let (k, m) = parse_chord("Shift+F5").expect("parse_chord succeeds");
    assert_eq!(k, Key::F(5));
    assert_eq!(m, KeyMods::SHIFT);
    let (k, m) = parse_chord("a").expect("parse_chord succeeds");
    assert_eq!(k, Key::Char('a'));
    assert_eq!(m, KeyMods::NONE);
    assert!(parse_chord("Ctrl+").is_err());
    assert!(parse_chord("Nope+X").is_err());
    assert!(parse_chord("F13").is_err());
}

#[test]
fn resize_changes_grid() {
    let mut s = Tui::new(["/bin/cat"])
        .size(80, 24)
        .spawn()
        .expect("spawn succeeds");
    s.resize(40, 10).expect("resize succeeds");
    let snap = s.snapshot().expect("snapshot succeeds");
    assert_eq!((snap.cols(), snap.rows()), (40, 10));
    assert!(s.resize(1, 10).is_err());
    assert!(s.resize(40, 0).is_err());
    s.close().expect("close succeeds");
}

#[test]
fn predicate_timeout_yields_evidence() {
    let mut s = Tui::new(["/bin/cat"])
        .size(50, 8)
        .spawn()
        .expect("spawn succeeds");
    let err = s
        .wait_predicate(|_| false, deadline(1), &cancel())
        .expect_err("wait_predicate must fail");
    match err {
        WaitError::Timeout { evidence, .. } => {
            assert_eq!((evidence.screen.cols(), evidence.screen.rows()), (50, 8));
        }
        other => panic!("expected timeout, got {other}"),
    }
    s.close().expect("close succeeds");
}

#[test]
fn exit_codes_observed() {
    let s = Tui::new(["/bin/sh", "-c", "exit 3"])
        .spawn()
        .expect("spawn succeeds");
    let w = s
        .expect_exit(deadline(5), &cancel())
        .expect("expect_exit succeeds");
    assert!(!w.status.success());
    w.code(3).expect("code succeeds");
    // `w` moved; re-check via a fresh failing assertion on another session.
    let s = Tui::new(["/bin/sh", "-c", "exit 3"])
        .spawn()
        .expect("spawn succeeds");
    let w = s
        .expect_exit(deadline(5), &cancel())
        .expect("expect_exit succeeds");
    assert!(w.success().is_err());
}

#[test]
fn printf_app_output_and_success() {
    let s = Tui::new(["/bin/sh", "-c", "printf 'out-42\\n'"])
        .spawn()
        .expect("spawn succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "out-42"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "out-42"));
    let w = s
        .expect_exit(deadline(5), &cancel())
        .expect("expect_exit succeeds");
    w.success().expect("success succeeds");
}

#[test]
fn close_reaps_child_no_leak() {
    let mut s = Tui::new(["/bin/sleep", "30"])
        .spawn()
        .expect("spawn succeeds");
    let pid = s.pid().expect("child pid");
    assert!(process_exists(pid));
    s.close().expect("close succeeds");
    assert!(!process_exists(pid), "child {pid} leaked after close");
}

#[test]
fn drop_reaps_child_no_leak() {
    let pid = {
        let s = Tui::new(["/bin/sleep", "30"])
            .spawn()
            .expect("spawn succeeds");
        s.pid().expect("child pid")
    };
    assert!(!process_exists(pid), "child {pid} leaked after drop");
}

#[test]
fn finish_eofs_cat_to_success() {
    let s = Tui::new(["/bin/cat"])
        .size(60, 10)
        .spawn()
        .expect("spawn succeeds");
    s.send_text("bye\n").expect("send_text succeeds");
    let _ = s.wait_predicate(|o| contains(&o.screen, "bye"), deadline(5), &cancel());
    let status = s.finish(deadline(5)).expect("finish succeeds");
    assert!(status.success());
}

#[test]
fn two_sessions_independent() {
    let mut a = Tui::new(["/bin/cat"])
        .size(60, 10)
        .spawn()
        .expect("spawn succeeds");
    let mut b = Tui::new(["/bin/cat"])
        .size(60, 10)
        .spawn()
        .expect("spawn succeeds");
    a.send_text("alpha-1\n").expect("send_text succeeds");
    b.send_text("beta-2\n").expect("send_text succeeds");
    let oa = a
        .wait_predicate(|o| contains(&o.screen, "alpha-1"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    let ob = b
        .wait_predicate(|o| contains(&o.screen, "beta-2"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&oa.screen, "alpha-1"));
    assert!(!contains(&oa.screen, "beta-2"));
    assert!(contains(&ob.screen, "beta-2"));
    assert!(!contains(&ob.screen, "alpha-1"));
    a.close().expect("close succeeds");
    b.close().expect("close succeeds");
}

#[test]
fn cancel_mid_wait() {
    let mut s = Tui::new(["/bin/cat"]).spawn().expect("spawn succeeds");
    let token = CancelToken::new();
    let killer = token.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        killer.cancel();
    });
    let err = s
        .wait_predicate(|_| false, deadline(30), &token)
        .expect_err("wait_predicate must fail");
    handle.join().expect("join succeeds");
    match err {
        WaitError::Cancelled { evidence } => {
            assert_eq!((evidence.screen.cols(), evidence.screen.rows()), (80, 24));
        }
        other => panic!("expected cancelled, got {other}"),
    }
    // Session still usable after a cancelled wait.
    s.send_text("after-cancel\n").expect("send_text succeeds");
    let obs = s
        .wait_predicate(
            |o| contains(&o.screen, "after-cancel"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "after-cancel"));
    s.close().expect("close succeeds");
}

#[test]
fn wait_stable_settles() {
    let mut s = Tui::new(["/bin/sh", "-c", "printf 'steady\\n'"])
        .spawn()
        .expect("spawn succeeds");
    let obs = s
        .wait_stable(deadline(10), &cancel())
        .expect("wait_stable succeeds");
    assert!(contains(&obs.screen, "steady"));
    s.close().expect("close succeeds");
}

#[test]
fn wait_frame_unsupported_with_evidence() {
    let mut s = Tui::new(["/bin/cat"])
        .size(40, 10)
        .spawn()
        .expect("spawn succeeds");
    let err = s
        .wait_frame(deadline(2), &cancel())
        .expect_err("wait_frame must fail");
    match err {
        WaitError::Unsupported { evidence, .. } => {
            assert_eq!(evidence.screen.cols(), 40);
        }
        other => panic!("expected unsupported, got {other}"),
    }
    s.close().expect("close succeeds");
}

#[test]
fn mouse_refused_without_mode() {
    let mut s = Tui::new(["/bin/cat"]).spawn().expect("spawn succeeds");
    let err = s
        .click(MouseButton::Left, 5, 5, MouseMods::NONE)
        .expect_err("click must fail");
    assert!(matches!(err, TuiError::ModeNotEnabled(_)), "got {err}");
    let err = s.focus_in().expect_err("focus_in must fail");
    assert!(matches!(err, TuiError::ModeNotEnabled(_)), "got {err}");
    s.close().expect("close succeeds");
}

#[test]
fn mouse_click_sgr_roundtrip() {
    let mut s = Tui::new(["/bin/sh", "-c", "printf '\\e[?1000h\\e[?1006h'; cat"])
        .size(60, 12)
        .spawn()
        .expect("spawn succeeds");
    // Wait until the app enabled SGR mouse.
    s.wait_predicate(
        |o| {
            o.state
                .modes
                .known()
                .is_some_and(|m| m.contains(&1000) && m.contains(&1006))
        },
        deadline(5),
        &cancel(),
    )
    .expect("contains succeeds");
    s.click(MouseButton::Left, 5, 3, MouseMods::NONE)
        .expect("click succeeds");
    // SGR press `<0;6;4M` echoed back by cat.
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "<0;6;4M"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "<0;6;4M"));
    // Hover needs 1003, which this app did not enable.
    let err = s
        .mouse_move(1, 1, MouseMods::NONE)
        .expect_err("mouse_move must fail");
    assert!(matches!(err, TuiError::ModeNotEnabled(_)), "got {err}");
    s.close().expect("close succeeds");
}

#[test]
fn mouse_wheel_and_drag_roundtrip() {
    let mut s = Tui::new([
        "/bin/sh",
        "-c",
        "printf '\\e[?1000h\\e[?1003h\\e[?1006h'; cat",
    ])
    .size(60, 12)
    .spawn()
    .expect("spawn succeeds");
    // Mouse protocols are mutually exclusive: 1003 supersedes 1000.
    s.wait_predicate(
        |o| {
            o.state
                .modes
                .known()
                .is_some_and(|m| !m.contains(&1000) && m.contains(&1003) && m.contains(&1006))
        },
        deadline(5),
        &cancel(),
    )
    .expect("contains succeeds");
    s.mouse_wheel(Wheel::Up, 2, 2, MouseMods::NONE)
        .expect("mouse_wheel succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "<64;3;3M"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "<64;3;3M"));
    s.mouse_down(MouseButton::Left, 1, 1, MouseMods::NONE)
        .expect("mouse_down succeeds");
    s.mouse_drag(MouseButton::Left, 4, 1, MouseMods::NONE)
        .expect("mouse_drag succeeds");
    s.mouse_up(MouseButton::Left, 4, 1, MouseMods::NONE)
        .expect("mouse_up succeeds");
    // Drag motion `32;5;2M` echoed back.
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "<32;5;2M"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "<32;5;2M"));
    s.close().expect("close succeeds");
}

#[test]
fn focus_roundtrip() {
    let mut s = Tui::new(["/bin/sh", "-c", "printf '\\e[?1004h'; cat"])
        .size(60, 8)
        .spawn()
        .expect("spawn succeeds");
    s.wait_predicate(
        |o| o.state.modes.known().is_some_and(|m| m.contains(&1004)),
        deadline(5),
        &cancel(),
    )
    .expect("contains succeeds");
    s.focus_in().expect("focus_in succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "[I"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "[I"));
    s.focus_out().expect("focus_out succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "[O"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "[O"));
    s.close().expect("close succeeds");
}

#[test]
fn paste_negotiated_and_delimiter_rejected() {
    // Without 2004 the paste goes through plain.
    let mut s = Tui::new(["/bin/cat"])
        .size(60, 8)
        .spawn()
        .expect("spawn succeeds");
    s.paste("plain-paste").expect("paste succeeds");
    let obs = s
        .wait_predicate(
            |o| contains(&o.screen, "plain-paste"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "plain-paste"));
    // Delimiter injection is rejected, never delivered.
    let err = s.paste("a\x1b[201~b").expect_err("paste must fail");
    assert!(matches!(err, TuiError::PasteRejected(_)), "got {err}");
    s.close().expect("close succeeds");

    // With 2004 the paste is bracketed.
    let mut s = Tui::new(["/bin/sh", "-c", "printf '\\e[?2004h'; cat"])
        .size(60, 8)
        .spawn()
        .expect("spawn succeeds");
    s.wait_predicate(
        |o| o.state.modes.known().is_some_and(|m| m.contains(&2004)),
        deadline(5),
        &cancel(),
    )
    .expect("contains succeeds");
    s.paste("wrapped").expect("paste succeeds");
    let obs = s
        .wait_predicate(
            |o| contains(&o.screen, "[200~wrapped"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "[200~wrapped"));
    s.close().expect("close succeeds");
}

#[test]
fn key_release_without_kitty_is_noop() {
    let mut s = Tui::new(["/bin/cat"])
        .size(60, 8)
        .spawn()
        .expect("spawn succeeds");
    // No kitty protocol negotiated: release emits nothing but succeeds.
    s.key_up(Key::Char('x'), KeyMods::NONE)
        .expect("Char succeeds");
    s.key_down(Key::Char('y'), KeyMods::NONE)
        .expect("Char succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "y"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "y"));
    assert!(!contains(&obs.screen, "x"));
    s.close().expect("close succeeds");
}

#[test]
fn profile_beyond_backend_rejected() {
    let profile = TerminalProfile {
        synchronized_output: true,
        ..TerminalProfile::default()
    };
    let err = Tui::new(["/bin/cat"])
        .profile(profile)
        .spawn()
        .expect_err("spawn must fail");
    assert!(matches!(err, TuiError::Unsupported(_)), "got {err}");

    let profile = TerminalProfile {
        cell_blink: true,
        ..TerminalProfile::default()
    };
    let err = Tui::new(["/bin/cat"])
        .profile(profile)
        .spawn()
        .expect_err("spawn must fail");
    assert!(matches!(err, TuiError::Unsupported(_)), "got {err}");
}

#[test]
fn signal_terminates_child() {
    let s = Tui::new(["/bin/sleep", "30"])
        .spawn()
        .expect("spawn succeeds");
    s.signal(Signal::Term).expect("signal succeeds");
    let w = s
        .expect_exit(deadline(5), &cancel())
        .expect("expect_exit succeeds");
    assert!(!w.status.success());
    assert!(w.status.signal().is_some());
}

#[test]
fn child_env_and_cwd_are_child_only() {
    let mut s = Tui::new([
        "/bin/sh",
        "-c",
        "printf \"v=$TUISNAP_TUI_PROBE pwd=$PWD\\n\"; cat",
    ])
    .env("TUISNAP_TUI_PROBE", "probe-7")
    .cwd("/tmp".into())
    .size(80, 8)
    .spawn()
    .expect("spawn succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "v=probe-7"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "v=probe-7"));
    // macOS resolves /tmp to /private/tmp; both contain "tmp".
    let row = rows(&obs.screen)
        .into_iter()
        .find(|r| r.contains("v=probe-7"))
        .expect("contains succeeds");
    assert!(row.contains("pwd=") && row.contains("tmp"), "row: {row:?}");
    assert!(std::env::var_os("TUISNAP_TUI_PROBE").is_none());
    s.close().expect("close succeeds");
}

#[test]
fn raw_bytes_and_enter_key() {
    let mut s = Tui::new(["/bin/cat"])
        .size(60, 8)
        .spawn()
        .expect("spawn succeeds");
    s.send_bytes(b"raw-9").expect("send_bytes succeeds");
    s.press_key(Key::Enter, KeyMods::NONE)
        .expect("press_key succeeds");
    let obs = s
        .wait_predicate(|o| contains(&o.screen, "raw-9"), deadline(5), &cancel())
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "raw-9"));
    s.close().expect("close succeeds");
}
