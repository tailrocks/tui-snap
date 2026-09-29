//! Single-cell underline assertions.

use tuiscotti_core::frame::{Color, UnderlineStyle};
use tuiscotti_core::screen::Screen;

// ---------------------------------------------------------------------------
// Cell underline assertion (M03)
// ---------------------------------------------------------------------------

/// Check one cell's underline style + color. `Ok(())` on exact match;
/// `Err` names the cell, what was wanted, and what is there (including a
/// missing cell, which is never a silent pass).
pub fn check_underline_at(
    screen: &Screen,
    x: u16,
    y: u16,
    want_style: UnderlineStyle,
    want_color: Color,
) -> Result<(), String> {
    let Some(cell) = screen.get(x, y) else {
        return Err(format!(
            "cell ({x},{y}): no such cell on {}x{} screen",
            screen.cols(),
            screen.rows()
        ));
    };
    let got_style = cell.mods.effective_underline_style();
    if got_style == want_style && cell.underline_color == want_color {
        Ok(())
    } else {
        Err(format!(
            "cell ({x},{y}) {:?}: want underline {:?} + {:?}, got {:?} + {:?}",
            cell.symbol, want_style, want_color, got_style, cell.underline_color
        ))
    }
}

/// Assert one cell's underline style + color, panicking with the
/// [`check_underline_at`] message on mismatch.
pub fn assert_underline_at(
    screen: &Screen,
    x: u16,
    y: u16,
    want_style: UnderlineStyle,
    want_color: Color,
) {
    if let Err(e) = check_underline_at(screen, x, y, want_style, want_color) {
        panic!("tuisnap assert_underline_at: {e}");
    }
}
