//! Cursor visibility shim for ratatui 0.29, whose `TestBackend` (unlike
//! 0.30's) exposes no visibility getter.
//!
//! 0.29's `Terminal::try_draw` sets backend visibility deterministically: a
//! `None` frame cursor calls only `hide_cursor` (leaving the stored position
//! untouched), while `Some(p)` calls `show_cursor` + `set_cursor_position(p)`
//! — identical to 0.30. `set_cursor_position` stores its argument unclamped,
//! so when the backend is planted with [`CURSOR_SENTINEL`] before `draw`, a
//! post-draw position equal to it means the closure never placed the cursor
//! (hidden); anything else means visible. Exact for every reachable capture.

use ratatui::backend::{Backend, TestBackend};
use ratatui::layout::Position;

/// Out-of-bounds cursor sentinel: no draw can place the cursor here, since
/// `Terminal::try_draw` only stores positions the closure sets (always
/// in-bounds) and `TestBackend` starts at `(0, 0)`.
pub const CURSOR_SENTINEL: Position = Position {
    x: u16::MAX,
    y: u16::MAX,
};

/// Plant [`CURSOR_SENTINEL`] on a test terminal before `draw` so the capture
/// paths can recover post-draw cursor visibility via `normalize_cursor()`.
/// Adapter-owned draws ([`super::render_screen`], [`super::draw_frame`])
/// plant automatically; callers that draw directly must plant first.
///
/// # Errors
///
/// Forwards backend I/O failures (unreachable on `TestBackend`).
pub fn plant_cursor_sentinel(term: &mut ratatui::Terminal<TestBackend>) -> std::io::Result<()> {
    term.backend_mut().set_cursor_position(CURSOR_SENTINEL)
}

/// Normalize a post-draw stored cursor position into `(position, visible)`
/// with 0.30 parity: a stored sentinel means the draw placed no cursor, so
/// report the fresh-`TestBackend` default `(0, 0)` hidden (what 0.30's
/// getters return on the same flow) and never leak the sentinel into
/// captured frames. All adapter flows draw exactly once on a fresh terminal,
/// where the pre-draw position is always `(0, 0)`.
///
/// Exclusion: a draw placing the cursor exactly at `(u16::MAX, u16::MAX)`
/// would be misread as hidden; unreachable, since `try_draw` only stores
/// closure-set in-bounds positions.
#[must_use]
pub(crate) fn normalize_cursor(pos: Position) -> (Position, bool) {
    if pos == CURSOR_SENTINEL {
        (Position::new(0, 0), false)
    } else {
        (pos, true)
    }
}
