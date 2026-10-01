//! M09: pure-view external consumer proof.
//!
//! Builds against `tuiscotti` with `default-features = false`, so the `pty`
//! feature (termpane) must NOT be required.
//! Exercises only the always-available API: [`tuiscotti::Frame`] canonical
//! data plus the [`tuiscotti::ratatui`] pure-view adapter (no PTY, no spawn).

use ratatui::widgets::Paragraph;

fn main() {
    // 1. Canonical frame data: blank grid + styled cell, no emulator.
    let provenance = tuiscotti::Provenance::now("consumer", "raw", vec![]);
    let mut frame = tuiscotti::Frame::blank(8, 3, provenance);
    let mut cell = tuiscotti::Cell::blank(0, 0);
    cell.symbol = "X".to_string();
    cell.mods.bold = true;
    cell.mods.dim = true;
    cell.mods.hidden = true;
    frame.set(cell);
    let got = frame.get(0, 0).unwrap();
    assert!(got.mods.bold && got.mods.dim && got.mods.hidden);
    assert_eq!(got.symbol, "X");

    // 2. Pure-view path: production Ratatui widget -> canonical frame.
    let provenance = tuiscotti::Provenance::now("consumer", "view", vec![]);
    let view = tuiscotti::ratatui::widget_frame(Paragraph::new("hi"), 8, 3, provenance);
    assert_eq!(view.text().chars().next().unwrap(), 'h');

    // 3. Screen observation type is available without the pty feature.
    let _screen_type: Option<tuiscotti::Screen> = None;

    println!("ordinary consumer: pure-view API works without default features");
}
