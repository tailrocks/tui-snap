//! Backend-swap pins (O16 blink, I2 urxvt encoding): split from `input.rs`
//! to hold the file line gate (shared helpers live in the root).

use super::{cancel, contains, deadline};
use tuiscotti_runtime::tui::{MouseButton, MouseMods, TerminalProfile, Tui};

/// O16: per-cell blink is tracked now, so the profile accepts and blink
/// bytes project onto the live grid.
#[test]
fn profile_cell_blink_accepted_and_live() {
    let profile = TerminalProfile {
        cell_blink: true,
        ..TerminalProfile::default()
    };
    // The child emits the SGR bytes itself: a `send_text` here would
    // race PTY echo (caret notation, unparsed) before the child runs.
    let s = Tui::new(["/bin/sh", "-c", "printf '\\033[5mB\\n'; exec /bin/cat"])
        .size(40, 10)
        .profile(profile)
        .spawn()
        .expect("spawn succeeds");
    let obs = s
        .wait_predicate(
            |o| contains(&o.screen, "B").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    let cell = obs.screen.get(0, 0).expect("cell in grid");
    assert!(cell.mods.blink, "SGR 5 blinks live: {cell:?}");
    s.close().expect("close succeeds");
}

/// I2: urxvt mouse encoding (1015) maps to legacy X10 bytes. The app
/// enables 1000+1015; the click succeeds (mode gate) and the echo shows
/// X10 bytes, not SGR: press-left at (5,3) is `ESC [ M SP & $`, where
/// `M` deletes the line and `&$` prints.
#[test]
fn mouse_click_urxvt_maps_to_x10() {
    let s = Tui::new(["/bin/sh", "-c", "printf '\\e[?1000h\\e[?1015h'; cat"])
        .size(60, 12)
        .spawn()
        .expect("spawn succeeds");
    s.wait_predicate(
        |o| {
            o.state.modes.known().is_some_and(|m| {
                // 1015 itself stays unreported (old-backend parity).
                m.contains(&1000) && !m.contains(&1015)
            })
        },
        deadline(5),
        &cancel(),
    )
    .expect("contains succeeds");
    s.click(MouseButton::Left, 5, 3, MouseMods::NONE)
        .expect("click succeeds");
    let obs = s
        .wait_predicate(
            |o| contains(&o.screen, "&$").expect("screen rows readable"),
            deadline(5),
            &cancel(),
        )
        .expect("wait_predicate succeeds");
    assert!(contains(&obs.screen, "&$").expect("screen rows readable"));
    s.close().expect("close succeeds");
}
