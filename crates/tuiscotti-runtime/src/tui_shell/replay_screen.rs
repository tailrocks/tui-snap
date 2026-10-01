use base64::Engine;
use termpane::DamageGrid;
use termpane::PassthroughEvent;
use termpane::cell::{Color as TermColor, UnderlineStyle as TermUnderline};
use termpane::snapshot::{GridSnapshot, SnapCell};

use super::{ClipboardItem, ClipboardTarget, ReplayError, ReplayEvents, SandboxClipboard};
use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, UnderlineStyle};
use tuiscotti_core::screen::Screen;

pub(crate) fn drain_replay_events(grid: &mut DamageGrid, events: &mut ReplayEvents) {
    for event in grid.drain_passthrough() {
        match event {
            PassthroughEvent::TitleChanged(t) => {
                events.title = if t.is_empty() { None } else { Some(t) };
            }
            PassthroughEvent::IconNameChanged(name) => {
                events.title = if name.is_empty() { None } else { Some(name) };
            }
            PassthroughEvent::Bell => events.bells += 1,
            PassthroughEvent::ClipboardWrite(payload) => {
                store_clipboard(&payload, &mut events.clipboard);
            }
            // No PTY to answer: query replies are dropped, like the live
            // worker drops them once the writer is gone.
            _ => {}
        }
    }
}

/// One OSC 52 store into the sandbox clipboard. Selection and validity
/// rules match the old emulator exactly: first selection byte only
/// (`c` clipboard, `p`/`s` selection, else ignored), `?` reads silent
/// (the old backend denied them by policy), stores gated on valid
/// base64 plus valid UTF-8.
fn store_clipboard(payload: &str, clipboard: &mut SandboxClipboard) {
    let Some((sel, b64)) = payload.split_once(';') else {
        return;
    };
    let target = match sel.as_bytes().first() {
        Some(b'c') => ClipboardTarget::Clipboard,
        Some(b'p' | b's') => ClipboardTarget::Selection,
        _ => return,
    };
    if b64 == "?" {
        return;
    }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else {
        return;
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return;
    };
    clipboard.store(ClipboardItem { target, text });
}

/// Viewport grid + cursor, mirroring the live observation builder's mapping
/// rules (wide-char pairing, orphan continuations, color/cursor mapping).
pub(crate) fn build_replay_screen(
    grid: &DamageGrid,
    snapshot: &GridSnapshot,
    cols: u16,
    rows: u16,
) -> Result<Screen, ReplayError> {
    let mut cells = Vec::with_capacity(cols as usize * rows as usize);
    for y in 0..rows {
        let row = &snapshot.cells[y as usize];
        let mut x: u16 = 0;
        while x < cols {
            let cell = &row[x as usize];
            let paired = cell.is_wide
                && !cell.is_wide_continuation
                && x + 1 < cols
                && row[(x + 1) as usize].is_wide_continuation;
            if paired {
                cells.push(replay_cell(x, y, cell, 2, false));
                cells.push(Cell {
                    x: x + 1,
                    y,
                    symbol: String::new(),
                    width: 0,
                    continuation: true,
                    fg: Color::Default,
                    bg: Color::Default,
                    mods: Mods::default(),
                    underline_color: Color::Default,
                });
                x += 2;
            } else if cell.is_wide_continuation {
                cells.push(Cell::blank(x, y));
                x += 1;
            } else {
                cells.push(replay_cell(x, y, cell, 1, false));
                x += 1;
            }
        }
    }
    let (cursor_row, cursor_col) = snapshot.cursor;
    let cursor = replay_cursor(grid, cursor_row, cursor_col, cols, rows);
    Screen::validate(cols, rows, 0, 0, cells, cursor)
        .map_err(|e| ReplayError::ScreenBuild(e.to_string()))
}

fn replay_cell(x: u16, y: u16, cell: &SnapCell, width: u8, cont: bool) -> Cell {
    let symbol = if cell.text.is_empty() {
        " ".to_string()
    } else {
        cell.text.clone()
    };
    let attrs = cell.attributes;
    Cell {
        x,
        y,
        symbol,
        width,
        continuation: cont,
        fg: replay_color(cell.fg),
        bg: replay_color(cell.bg),
        mods: Mods {
            hidden: attrs.conceal,
            blink: attrs.slow_blink || attrs.rapid_blink,
            bold: attrs.bold,
            dim: attrs.dim,
            italic: attrs.italic,
            underline: attrs.underline,
            underline_style: replay_underline_style(cell.underline_style),
            strikethrough: attrs.strikethrough,
            reverse: attrs.inverse,
        },
        underline_color: replay_color(cell.underline_color),
    }
}

/// Map backend underline styles (set from SGR 4 / 4:1..4:5 / 24) to the
/// canonical style; the names agree one to one.
fn replay_underline_style(style: TermUnderline) -> UnderlineStyle {
    match style {
        TermUnderline::None => UnderlineStyle::None,
        TermUnderline::Single => UnderlineStyle::Single,
        TermUnderline::Double => UnderlineStyle::Double,
        TermUnderline::Curly => UnderlineStyle::Curly,
        TermUnderline::Dotted => UnderlineStyle::Dotted,
        TermUnderline::Dashed => UnderlineStyle::Dashed,
    }
}

fn replay_color(c: TermColor) -> Color {
    match c {
        TermColor::Default => Color::Default,
        TermColor::Idx(i) => Color::Indexed(i),
        TermColor::Rgb(r, g, b) => Color::Rgb(Rgb { r, g, b }),
    }
}

fn replay_cursor(grid: &DamageGrid, row: u16, col: u16, cols: u16, rows: u16) -> Cursor {
    let style = grid.cursor_style();
    let shape = match style {
        3 | 4 => CursorStyle::Underline,
        5 | 6 => CursorStyle::Bar,
        _ => CursorStyle::Block,
    };
    let visible = !grid.hide_cursor() && row < rows && col < cols;
    if !visible {
        return Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: shape,
            blinking: false,
        };
    }
    Cursor {
        x: col,
        y: row,
        visible: true,
        style: shape,
        blinking: matches!(style, 1 | 3 | 5),
    }
}
