//! Input devices: cancel, waits, mouse, focus, paste, keys, profile, env (split from `tui.rs`; shared helpers live in the root).

use super::{cancel, contains, deadline, rows};
use std::time::Duration;
use tuiscotti_runtime::tui::{
    CancelToken, Key, KeyMods, MouseButton, MouseMods, Signal, TerminalProfile, Tui, TuiError,
    WaitError, Wheel,
};

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
            |o| contains(&o.screen, "after-cancel").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "after-cancel").expect("screen rows readable"));
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
    assert!(contains(&obs.screen, "steady").expect("screen rows readable"));
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
        .wait_predicate(
            |o| contains(&o.screen, "<0;6;4M").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "<0;6;4M").expect("screen rows readable"));
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
        .wait_predicate(
            |o| contains(&o.screen, "<64;3;3M").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "<64;3;3M").expect("screen rows readable"));
    s.mouse_down(MouseButton::Left, 1, 1, MouseMods::NONE)
        .expect("mouse_down succeeds");
    s.mouse_drag(MouseButton::Left, 4, 1, MouseMods::NONE)
        .expect("mouse_drag succeeds");
    s.mouse_up(MouseButton::Left, 4, 1, MouseMods::NONE)
        .expect("mouse_up succeeds");
    // Drag motion `32;5;2M` echoed back.
    let obs = s
        .wait_predicate(
            |o| contains(&o.screen, "<32;5;2M").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "<32;5;2M").expect("screen rows readable"));
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
        .wait_predicate(
            |o| contains(&o.screen, "[I").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "[I").expect("screen rows readable"));
    s.focus_out().expect("focus_out succeeds");
    let obs = s
        .wait_predicate(
            |o| contains(&o.screen, "[O").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "[O").expect("screen rows readable"));
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
            |o| contains(&o.screen, "plain-paste").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "plain-paste").expect("screen rows readable"));
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
            |o| contains(&o.screen, "[200~wrapped").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "[200~wrapped").expect("screen rows readable"));
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
        .wait_predicate(
            |o| contains(&o.screen, "y").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "y").expect("screen rows readable"));
    assert!(!contains(&obs.screen, "x").expect("screen rows readable"));
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
        .wait_predicate(
            |o| contains(&o.screen, "v=probe-7").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "v=probe-7").expect("screen rows readable"));
    // macOS resolves /tmp to /private/tmp; both contain "tmp".
    let row = rows(&obs.screen)
        .expect("screen rows readable")
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
        .wait_predicate(
            |o| contains(&o.screen, "raw-9").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "raw-9").expect("screen rows readable"));
    s.close().expect("close succeeds");
}
