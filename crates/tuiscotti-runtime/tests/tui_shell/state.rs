//! R13 terminal-state assertions (split from `tui_shell.rs`; shared helpers live in the root).

use super::{cancel, deadline};
use tuiscotti_core::frame::Rgb;
use tuiscotti_core::screen::Maybe;
use tuiscotti_runtime::tui::Tui;
use tuiscotti_runtime::tui_shell::{
    ClipboardTarget, TermSnapshot, assert_bells_eq, assert_clipboard_empty,
    assert_clipboard_latest_eq, assert_default_colors, assert_hyperlink_present, assert_mode_set,
    assert_mode_unset, assert_palette_entry, assert_scrollback_contains, assert_title_eq,
    replay_bytes,
};

// ---------------------------------------------------------------------------
// R13: terminal-state assertions
// ---------------------------------------------------------------------------

#[test]
fn live_title_bells_modes_palette() {
    let s = Tui::new(["/bin/cat"])
        .size(60, 12)
        .spawn()
        .expect("spawn succeeds");
    // Trailing newline: cat only echoes full lines.
    s.send_text("\x1b]2;HelloTitle\x07\n")
        .expect("send_text succeeds");
    let obs = s
        .wait_predicate(
            |o| o.state.title == Maybe::Known("HelloTitle".to_string()),
            deadline(10),
            &cancel(),
        )
        .expect("to_string succeeds");
    let snap = TermSnapshot::from_observation(&obs);
    assert_title_eq(&snap, "HelloTitle").expect("assert_title_eq succeeds");
    assert!(assert_title_eq(&snap, "Nope").is_err());

    s.send_text("\x07\n").expect("send_text succeeds");
    let obs = s
        .wait_predicate(
            |o| matches!(o.state.bells, Maybe::Known(n) if n == 1),
            deadline(10),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    let snap = TermSnapshot::from_observation(&obs);
    assert_bells_eq(&snap, 1).expect("assert_bells_eq succeeds");
    assert!(assert_bells_eq(&snap, 2).is_err());

    assert_mode_unset(&snap, 1049).expect("assert_mode_unset succeeds");
    assert!(assert_mode_set(&snap, 1049).is_err());

    assert_palette_entry(&snap, 1, Rgb::from_indexed(1)).expect("from_indexed succeeds");
    // Backend gap (O6): the emulator drops OSC 4, so a live palette
    // override is never observed — the entry keeps resolving to the
    // nominal default. Send one anyway and pin that the observation
    // stays nominal instead of hanging on an update that never comes.
    s.send_text("\x1b]4;1;rgb:ff/00/00\x1b\\\n")
        .expect("send_text succeeds");
    let obs = s.observe_now().expect("observe_now succeeds");
    let snap = TermSnapshot::from_observation(&obs);
    assert_palette_entry(&snap, 1, Rgb::from_indexed(1)).expect("from_indexed succeeds");
    s.close().expect("close succeeds");
}

#[test]
fn live_unsupported_state_reported_not_fabricated() {
    let s = Tui::new(["/bin/cat"])
        .size(40, 10)
        .spawn()
        .expect("spawn succeeds");
    let obs = s.observe_now().expect("observe_now succeeds");
    let snap = TermSnapshot::from_observation(&obs);
    assert_eq!(snap.defaults, Maybe::Unsupported);
    assert_eq!(snap.clipboard, Maybe::Unsupported);
    assert_eq!(snap.hyperlinks, Maybe::Unsupported);
    assert_eq!(snap.scrollback, Maybe::Unsupported);
    for err in [
        assert_hyperlink_present(&snap, "https://example.test/"),
        assert_scrollback_contains(&snap, "x"),
        assert_default_colors(&snap, None, None),
        assert_clipboard_latest_eq(&snap, "x"),
    ] {
        let msg = err
            .expect_err("assert_clipboard_latest_eq must fail")
            .to_string();
        assert!(msg.contains("unsupported"), "message: {msg}");
    }
    s.close().expect("close succeeds");
}

#[test]
fn replay_full_state_asserts() {
    let mut bytes: Vec<u8> = b"\x1b]2;ReTitle\x07\x07\x1b]4;2;rgb:00/ff/00\x1b\\".to_vec();
    bytes.extend_from_slice(b"\x1b]10;rgb:ff/00/00\x1b\\\x1b]11;rgb:00/00/ff\x1b\\");
    bytes.extend_from_slice(b"\x1b]52;c;aGVsbG8=\x1b\\");
    bytes.extend_from_slice(b"\x1b]8;;https://example.test/x\x1b\\LINK\x1b]8;;\x1b\\\r\n");
    for i in 1..=15 {
        bytes.extend_from_slice(format!("line{i:02}\r\n").as_bytes());
    }
    let r = replay_bytes(&bytes, 40, 10).expect("replay_bytes succeeds");
    assert_eq!(r.bytes_fed, bytes.len());
    assert_title_eq(&r.state, "ReTitle").expect("assert_title_eq succeeds");
    assert!(assert_title_eq(&r.state, "Nope").is_err());
    assert_bells_eq(&r.state, 1).expect("assert_bells_eq succeeds");
    // Backend gaps (O6/O7): replayed OSC 4 sets and OSC 10/11 set forms
    // are dropped by the emulator, so the palette entry resolves to the
    // nominal default and no default-color overrides are observed.
    assert_palette_entry(&r.state, 2, Rgb::from_indexed(2)).expect("assert_palette_entry succeeds");
    assert_default_colors(&r.state, None, None).expect("None succeeds");
    assert_clipboard_latest_eq(&r.state, "hello").expect("assert_clipboard_latest_eq succeeds");
    assert!(assert_clipboard_empty(&r.state).is_err());
    assert_hyperlink_present(&r.state, "https://example.test/x")
        .expect("assert_hyperlink_present succeeds");
    assert!(assert_hyperlink_present(&r.state, "https://other.test/").is_err());
    assert_scrollback_contains(&r.state, "line01").expect("assert_scrollback_contains succeeds");
    assert!(assert_scrollback_contains(&r.state, "missing-needle").is_err());
    assert_mode_unset(&r.state, 1049).expect("assert_mode_unset succeeds");
}

#[test]
fn replay_empty_state_known_not_guessed() {
    let r = replay_bytes(b"hi", 20, 5).expect("replay_bytes succeeds");
    assert_default_colors(&r.state, None, None).expect("assert_default_colors succeeds");
    assert_clipboard_empty(&r.state).expect("assert_clipboard_empty succeeds");
    let err = assert_title_eq(&r.state, "x")
        .expect_err("assert_title_eq must fail")
        .to_string();
    assert!(err.contains("unknown"), "message: {err}");
    let err = assert_hyperlink_present(&r.state, "x")
        .expect_err("assert_hyperlink_present must fail")
        .to_string();
    assert!(err.contains("not present"), "message: {err}");
}

/// O8: an empty OSC 2 title resets to unknown; OSC 1 folds into the
/// title, matching the old backend on both rules.
#[test]
fn replay_title_empty_resets_and_icon_folds() {
    let r = replay_bytes(b"\x1b]2;First\x07\x1b]2;\x07", 20, 5).expect("replay_bytes succeeds");
    assert_eq!(r.state.title, Maybe::Unknown);
    let r = replay_bytes(b"\x1b]1;IconTitle\x07", 20, 5).expect("replay_bytes succeeds");
    assert_title_eq(&r.state, "IconTitle").expect("assert_title_eq succeeds");
}

/// O9: OSC 52 selection map (`c` clipboard, `p`/`s` selection) plus the
/// validity gates: `?` reads stay silent, bad base64 and non-UTF-8 store
/// nothing.
#[test]
fn replay_clipboard_selection_map_and_gates() {
    let r = replay_bytes(b"\x1b]52;p;aGk=\x1b\\", 20, 5).expect("replay_bytes succeeds");
    let Maybe::Known(clip) = &r.state.clipboard else {
        panic!("clipboard must be known");
    };
    let latest = clip.latest().expect("one store captured");
    assert_eq!(latest.target, ClipboardTarget::Selection);
    assert_eq!(latest.text, "hi");
    assert_clipboard_latest_eq(&r.state, "hi").expect("assert_clipboard_latest_eq succeeds");

    let r = replay_bytes(
        b"\x1b]52;c;?\x07\x1b]52;c;!!!\x07\x1b]52;c;/w==\x1b\\",
        20,
        5,
    )
    .expect("replay_bytes succeeds");
    assert_clipboard_empty(&r.state).expect("assert_clipboard_empty succeeds");
}

/// O16: per-cell blink (SGR 5/6) is tracked and projected, slow and rapid.
#[test]
fn replay_blink_cells_projected() {
    let r = replay_bytes(b"\x1b[5mS\x1b[6mR\x1b[0mP", 20, 5).expect("replay_bytes succeeds");
    let slow = r.screen.get(0, 0).expect("cell in grid");
    assert!(slow.mods.blink, "SGR 5 blinks: {slow:?}");
    let rapid = r.screen.get(1, 0).expect("cell in grid");
    assert!(rapid.mods.blink, "SGR 6 blinks: {rapid:?}");
    let plain = r.screen.get(2, 0).expect("cell in grid");
    assert!(!plain.mods.blink, "SGR 0 clears: {plain:?}");
}
