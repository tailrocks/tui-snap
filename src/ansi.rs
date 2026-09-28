//! Raw ANSI stream replay through an established emulator (feature `pty`).
//!
//! Contract (see docs/RESEARCH.md §6):
//! - **raw streams** (recorded PTY bytes, tmux `capture-pane -e` output)
//!   replay through `termpane` with explicit dimensions — cursor motion,
//!   alternate screen, and scrolling are interpreted, not discarded;
//! - **normalized dumps** ([`crate::render::ansi_dump`]) are debugging views
//!   generated FROM a frame and must never be re-parsed as state.
//!
//! The old hand-written SGR replay parser is gone on purpose.
//!
//! Raw replay preserves all cell attributes through termpane's native model
//! (independent bold/dim, blink slow||rapid, conceal, strikethrough).
//! Cursor appearance remains unsupported here: use the PTY path when shape
//! or blinking is part of the assertion. Position and visibility are retained.

use crate::frame::{Cell, Color, Cursor, CursorStyle, Frame, Mods, Provenance, Rgb};

fn convert_color(c: termpane::Color) -> Color {
    match c {
        termpane::Color::Default => Color::Default,
        termpane::Color::Idx(i) => Color::Indexed(i),
        termpane::Color::Rgb(r, g, b) => Color::Rgb(Rgb::new(r, g, b)),
    }
}

/// Replay raw terminal bytes into a canonical [`Frame`].
///
/// `scrollback` bounds retained history (0 keeps the visible screen only).
/// Fails on empty dimensions; never guesses.
pub fn replay_raw(
    bytes: &[u8],
    cols: u16,
    rows: u16,
    scrollback: usize,
    provenance: Provenance,
) -> anyhow::Result<Frame> {
    anyhow::ensure!(cols > 0 && rows > 0, "dimensions must be nonzero");
    let mut grid = termpane::DamageGrid::new(rows, cols, scrollback);
    grid.process(bytes);
    let mut frame = Frame::blank(cols, rows, provenance);
    for r in 0..rows {
        for c in 0..cols {
            let Some(tp) = grid.cell(r, c) else {
                continue;
            };
            if tp.is_wide_continuation {
                let mut cont = Cell::blank(c, r);
                cont.fg = convert_color(tp.fgcolor());
                cont.bg = convert_color(tp.bgcolor());
                cont.mods = Mods {
                    bold: tp.bold(),
                    dim: tp.dim(),
                    italic: tp.italic(),
                    underline: tp.underline(),
                    reverse: tp.inverse(),
                    strikethrough: tp.strikethrough(),
                    hidden: tp.conceal(),
                    blink: tp.slow_blink() || tp.rapid_blink(),
                };
                cont.width = 0;
                cont.continuation = true;
                cont.symbol = String::new();
                frame.set(cont);
                continue;
            }
            let symbol = if tp.contents().is_empty() {
                " ".to_string()
            } else {
                tp.contents().to_string()
            };
            frame.set(Cell {
                x: c,
                y: r,
                symbol,
                width: if tp.is_wide { 2 } else { 1 },
                continuation: false,
                fg: convert_color(tp.fgcolor()),
                bg: convert_color(tp.bgcolor()),
                mods: Mods {
                    bold: tp.bold(),
                    dim: tp.dim(),
                    italic: tp.italic(),
                    underline: tp.underline(),
                    strikethrough: tp.strikethrough(),
                    hidden: tp.conceal(),
                    blink: tp.slow_blink() || tp.rapid_blink(),
                    reverse: tp.inverse(),
                },
            });
        }
    }
    // termpane reports the phantom pending-wrap column (== cols) while a
    // deferred wrap is armed; the canonical frame holds a physical cursor,
    // so clamp to cols-1 without discarding the pending wrap in the grid.
    // `validate` below still rejects out-of-grid cursors loudly.
    let (row, col) = grid.cursor_position();
    let col = col.min(cols.saturating_sub(1));
    frame.cursor = Cursor {
        x: col,
        y: row,
        visible: !grid.hide_cursor(),
        style: CursorStyle::Block,
        blinking: false,
    };
    frame.validate().map_err(anyhow::Error::msg)?;
    Ok(frame)
}
