use super::convert::{convert_color, convert_mods, lead_cell};
use crate::frame::{Cell, Cursor, CursorStyle, Frame, Provenance};
use ratatui::backend::Backend;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;

/// Convert a buffer plus explicit cursor state into a [`Frame`].
///
/// `cursor`: `(position, visible)`. Read it from
/// `TestBackend::get_cursor_position` after draw; `None` hides the cursor.
pub fn from_buffer(
    buf: &Buffer,
    cols: u16,
    rows: u16,
    cursor: Option<(Position, bool)>,
    provenance: Provenance,
) -> Frame {
    let mut frame = Frame::blank(cols, rows, provenance);
    // Straightforward row-major conversion with wide-cell continuations.
    for y in 0..rows {
        let mut x = 0u16;
        while x < cols {
            let Some(rc) = buf.cell((x, y)) else {
                x += 1;
                continue;
            };
            // Ratatui marks wide-cell followers with an empty symbol.
            if rc.symbol().is_empty() {
                let mut cont = Cell::blank(x, y);
                cont.fg = convert_color(rc.fg);
                cont.bg = convert_color(rc.bg);
                cont.mods = convert_mods(rc.modifier);
                cont.underline_color = convert_color(rc.underline_color);
                cont.width = 0;
                cont.continuation = true;
                cont.symbol = String::new();
                frame.set(cont);
                x += 1;
                continue;
            }
            let (lead, w) = lead_cell(x, y, rc);
            if w == 2 && x + 1 >= cols {
                // Wide grapheme at the exact row end: no room for its
                // continuation (a terminal would wrap or clip it). Downgrade
                // to width 1 so the frame stays valid; geometry still shows
                // the full symbol in one cell.
                let mut narrow = lead;
                narrow.width = 1;
                frame.set(narrow);
                x += 1;
                continue;
            }
            frame.set(lead);
            if w == 2 && x + 1 < cols {
                let mut cont = Cell::blank(x + 1, y);
                cont.fg = convert_color(rc.fg);
                cont.bg = convert_color(rc.bg);
                cont.mods = convert_mods(rc.modifier);
                cont.underline_color = convert_color(rc.underline_color);
                cont.width = 0;
                cont.continuation = true;
                cont.symbol = String::new();
                frame.set(cont);
                x += 2;
            } else {
                x += 1;
            }
        }
    }
    if let Some((pos, visible)) = cursor {
        frame.cursor = Cursor {
            x: pos.x,
            y: pos.y,
            visible,
            style: CursorStyle::Block,
            blinking: false,
        };
    }
    // Blank filler cells already carry width 1; normalize any untouched cell
    // that the buffer left as default (ratatui guarantees full coverage, but
    // stay total here).
    frame
}

/// Capture the current state of a `TestBackend` terminal: buffer + cursor.
///
/// Reads `get_cursor_position` post-draw so cursor-only changes are gated.
pub fn capture(
    term: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
    provenance: Provenance,
) -> Frame {
    let backend = term.backend_mut();
    let area = backend.buffer().area;
    let cursor = backend
        .get_cursor_position()
        .ok()
        .map(|pos| (pos, backend.cursor_visible()));
    // `Buffer::clone` via re-read: TestBackend exposes `buffer()`.
    let buf = backend.buffer().clone();
    from_buffer(&buf, area.width, area.height, cursor, provenance)
}

/// Render any `Widget` into a [`Frame`] at `cols`×`rows`.
///
/// The hardware cursor is forced hidden: widget unit tests pin content, and
/// a backend-default cursor at (0,0) would make the gate depend on backend
/// defaults instead of the view. Use [`draw_frame`]/[`capture`] for
/// stateful cursor placement.
pub fn widget_frame<W>(widget: W, cols: u16, rows: u16, provenance: Provenance) -> Frame
where
    W: ratatui::widgets::Widget,
{
    let mut frame = draw_frame(cols, rows, provenance, |f| {
        f.render_widget(widget, f.area());
    });
    frame.cursor.visible = false;
    frame
}

/// Render via a draw closure (full-app frames, layouts, stateful widgets).
/// Cursor is captured post-draw, so stateful cursor placement is preserved.
pub fn draw_frame(
    cols: u16,
    rows: u16,
    provenance: Provenance,
    draw: impl FnOnce(&mut ratatui::Frame),
) -> Frame {
    let backend = ratatui::backend::TestBackend::new(cols, rows);
    let mut term = ratatui::Terminal::new(backend).expect("test terminal");
    term.draw(draw).expect("draw");
    capture(&mut term, provenance)
}
