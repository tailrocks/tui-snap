//! Atomic observation builder: grid + cursor + palette + modes (R06).
//!
//! Runs only on the worker thread.

use std::time::{SystemTime, UNIX_EPOCH};

use termpane::DamageGrid;
use termpane::cell::{Color as TermColor, UnderlineStyle as TermUnderline};
use termpane::grid::{MouseProtocolEncoding, MouseProtocolMode};
use termpane::snapshot::SnapCell;
use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, UnderlineStyle};
use tuiscotti_core::screen::{
    CaptureProvenance, CaptureReason, Maybe, Observation, Screen, TermState,
};

use super::error::TuiError;
use super::worker::WorkerEventState;

/// Build one atomic observation: grid + cursor + palette + modes at the
/// worker's current state. Runs only on the worker thread.
pub(crate) fn build_observation(
    grid: &DamageGrid,
    events: &WorkerEventState,
    revision: u64,
    reason: CaptureReason,
    pid: Option<u32>,
    cols: u16,
    rows: u16,
) -> Result<Observation, TuiError> {
    let snapshot = grid.dump();
    let cells = collect_grid_cells(&snapshot.cells, cols, rows);

    let (cursor_row, cursor_col) = snapshot.cursor;
    let cursor = frame_cursor(grid, cursor_row, cursor_col, cols, rows);
    let screen = Screen::validate(cols, rows, 0, 0, cells, cursor)
        .map_err(|e| TuiError::Teardown(format!("built an invalid screen: {e}")))?;

    let mut modes = Vec::new();
    push_modes(grid, &mut modes);
    // Backend gap (O6): the emulator drops OSC 4, so no overrides are ever
    // observed; assertion helpers resolve every index to the nominal xterm
    // default. The empty list is that "no tracked overrides" state.
    let state = TermState {
        modes: Maybe::Known(modes),
        palette: Maybe::Known(Vec::new()),
        title: match &events.title {
            Some(t) => Maybe::Known(t.clone()),
            None => Maybe::Unknown,
        },
        bells: Maybe::Known(events.bells),
    };
    let provenance = CaptureProvenance::new(now_ms(), pid, None, 0);
    Ok(Observation::new(
        screen, revision, reason, state, provenance,
    ))
}

/// Row-major grid projection with wide-char pairing: a wide lead plus
/// its in-row continuation becomes a width-2 cell plus a continuation;
/// orphan continuations become blanks so the grid stays valid.
fn collect_grid_cells(rows: &[Vec<SnapCell>], cols: u16, grid_rows: u16) -> Vec<Cell> {
    let mut cells = Vec::with_capacity(cols as usize * grid_rows as usize);
    for y in 0..grid_rows {
        let row = &rows[y as usize];
        let mut x: u16 = 0;
        while x < cols {
            let cell = &row[x as usize];
            let paired = cell.is_wide
                && !cell.is_wide_continuation
                && x + 1 < cols
                && row[(x + 1) as usize].is_wide_continuation;
            if paired {
                cells.push(frame_cell(x, y, cell, 2, false));
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
                // Orphan continuation (its lead lives on another row or was
                // overwritten): render as a blank so the grid stays valid.
                cells.push(Cell::blank(x, y));
                x += 1;
            } else {
                // Narrow cell, or a wide lead with no in-row follower
                // (clamped to width 1: validity over width fidelity).
                cells.push(frame_cell(x, y, cell, 1, false));
                x += 1;
            }
        }
    }
    cells
}

fn frame_cell(x: u16, y: u16, cell: &SnapCell, width: u8, cont: bool) -> Cell {
    // Empty text is a blank (space); grapheme clusters arrive whole.
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
        fg: frame_color(cell.fg),
        bg: frame_color(cell.bg),
        mods: Mods {
            hidden: attrs.conceal,
            blink: attrs.slow_blink || attrs.rapid_blink,
            bold: attrs.bold,
            dim: attrs.dim,
            italic: attrs.italic,
            underline: attrs.underline,
            underline_style: frame_underline_style(cell.underline_style),
            strikethrough: attrs.strikethrough,
            // Overline (SGR 53) has no canonical field; same class of
            // intentional drop as before, the backend just tracks more
            // than the projection carries.
            reverse: attrs.inverse,
        },
        underline_color: frame_color(cell.underline_color),
    }
}

/// Map backend underline styles (set from SGR 4 / 4:1..4:5 / 24) to the
/// canonical style; the names agree one to one.
fn frame_underline_style(style: TermUnderline) -> UnderlineStyle {
    match style {
        TermUnderline::None => UnderlineStyle::None,
        TermUnderline::Single => UnderlineStyle::Single,
        TermUnderline::Double => UnderlineStyle::Double,
        TermUnderline::Curly => UnderlineStyle::Curly,
        TermUnderline::Dotted => UnderlineStyle::Dotted,
        TermUnderline::Dashed => UnderlineStyle::Dashed,
    }
}

fn frame_color(c: TermColor) -> Color {
    match c {
        TermColor::Default => Color::Default,
        TermColor::Idx(i) => Color::Indexed(i),
        TermColor::Rgb(r, g, b) => Color::Rgb(Rgb { r, g, b }),
    }
}

fn frame_cursor(grid: &DamageGrid, row: u16, col: u16, cols: u16, rows: u16) -> Cursor {
    // DECSCUSR: 0 default, 1 blinking block, 2 steady block, 3 blinking
    // underline, 4 steady underline, 5 blinking bar, 6 steady bar.
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

/// Map live backend mode state to DEC/private mode numbers.
fn push_modes(grid: &DamageGrid, out: &mut Vec<u16>) {
    if grid.application_cursor() {
        out.push(1);
    }
    // Backend gap (O5): modes 4 (IRM), 6 (DECOM), and 20 (LNM) are
    // absorbed untracked, so they never appear here.
    if grid.autowrap() {
        out.push(7);
    }
    if grid.application_keypad() {
        out.push(66);
    }
    match grid.mouse_protocol_mode() {
        MouseProtocolMode::None => {}
        MouseProtocolMode::Press => out.push(1000),
        MouseProtocolMode::PressRelease | MouseProtocolMode::ButtonMotion => out.push(1002),
        MouseProtocolMode::AnyEvent | MouseProtocolMode::AnyMotion => out.push(1003),
    }
    if grid.focus_events() {
        out.push(1004);
    }
    match grid.mouse_protocol_encoding() {
        // Urxvt (1015) is absorbed for parity: the old backend never
        // reported it, so reporting it now would be a new observable.
        MouseProtocolEncoding::Default | MouseProtocolEncoding::Urxvt => {}
        MouseProtocolEncoding::Utf8 => out.push(1005),
        MouseProtocolEncoding::Sgr => out.push(1006),
    }
    if grid.alternate_screen() {
        out.push(1049);
    }
    if grid.bracketed_paste() {
        out.push(2004);
    }
    if grid.kitty_kb_flags() != 0 {
        out.push(57399);
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| {
        u64::try_from(d.as_millis().min(u128::from(u64::MAX))).unwrap_or(u64::MAX)
    })
}
