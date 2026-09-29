//! Atomic observation builder: grid + cursor + palette + modes (R06).
//!
//! Runs only on the worker thread.

use std::time::{SystemTime, UNIX_EPOCH};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::vte::ansi::{Color as VteColor, CursorShape, NamedColor};
use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, UnderlineStyle};
use tuiscotti_core::screen::{
    CaptureProvenance, CaptureReason, Maybe, Observation, Screen, TermState,
};

use super::error::TuiError;
use super::worker::WorkerEventState;

/// Build one atomic observation: grid + cursor + palette + modes at the
/// worker's current state. Runs only on the worker thread.
pub(crate) fn build_observation<T: EventListener>(
    term: &Term<T>,
    events: &WorkerEventState,
    revision: u64,
    reason: CaptureReason,
    pid: Option<u32>,
    cols: u16,
    rows: u16,
) -> Result<Observation, TuiError> {
    let grid = term.grid();
    let cells = collect_grid_cells(term, cols, rows);

    let point = grid.cursor.point;
    let cursor = frame_cursor(term, point.line.0, point.column.0, cols, rows);
    let screen = Screen::validate(cols, rows, 0, 0, cells, cursor)
        .map_err(|e| TuiError::Teardown(format!("built an invalid screen: {e}")))?;

    let mut modes = Vec::new();
    push_modes(*term.mode(), &mut modes);
    let palette: Vec<(u8, Rgb)> = (0..256u16)
        .filter_map(|i| {
            term.colors()[i as usize].map(|c| {
                (
                    u8::try_from(i).unwrap_or(u8::MAX),
                    Rgb {
                        r: c.r,
                        g: c.g,
                        b: c.b,
                    },
                )
            })
        })
        .collect();
    let state = TermState {
        modes: Maybe::Known(modes),
        palette: Maybe::Known(palette),
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
/// its in-row spacer becomes a width-2 cell plus a continuation; orphan
/// spacers become blanks so the grid stays valid.
fn collect_grid_cells<T: EventListener>(term: &Term<T>, cols: u16, rows: u16) -> Vec<Cell> {
    let grid = term.grid();
    let mut cells = Vec::with_capacity(cols as usize * rows as usize);
    for y in 0..rows {
        let line = Line(i32::from(y));
        let mut x: u16 = 0;
        while x < cols {
            let cell = &grid[line][Column(x as usize)];
            let flags = cell.flags;
            if flags.contains(CellFlags::WIDE_CHAR)
                && x + 1 < cols
                && grid[line][Column(x as usize + 1)]
                    .flags
                    .contains(CellFlags::WIDE_CHAR_SPACER)
            {
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
            } else if flags.contains(CellFlags::WIDE_CHAR_SPACER)
                && x > 0
                && cells.last().is_some_and(|lead: &Cell| {
                    lead.width == 2 && !lead.continuation && lead.y == y && lead.x + 1 == x
                })
            {
                // Spacer already emitted with its lead above; unreachable in
                // practice, kept as the documented pairing rule.
                cells.push(Cell {
                    x,
                    y,
                    symbol: String::new(),
                    width: 0,
                    continuation: true,
                    fg: Color::Default,
                    bg: Color::Default,
                    mods: Mods::default(),
                    underline_color: Color::Default,
                });
                x += 1;
            } else if flags
                .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
            {
                // Orphan spacer (wrapped lead lives on another row): render
                // as a blank so the grid stays valid.
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

fn frame_cell(
    x: u16,
    y: u16,
    cell: &alacritty_terminal::term::cell::Cell,
    width: u8,
    cont: bool,
) -> Cell {
    let mut symbol = String::new();
    symbol.push(cell.c);
    if let Some(extra) = cell.zerowidth() {
        symbol.extend(extra.iter());
    }
    let flags = cell.flags;
    Cell {
        x,
        y,
        symbol,
        width,
        continuation: cont,
        fg: frame_color(cell.fg),
        bg: frame_color(cell.bg),
        mods: Mods {
            // Backend gap, honest: alacritty drops per-cell blink.
            hidden: flags.contains(CellFlags::HIDDEN),
            blink: false,
            bold: flags.contains(CellFlags::BOLD),
            dim: flags.contains(CellFlags::DIM),
            italic: flags.contains(CellFlags::ITALIC),
            underline: flags.intersects(CellFlags::ALL_UNDERLINES),
            underline_style: frame_underline_style(flags),
            strikethrough: flags.contains(CellFlags::STRIKEOUT),
            reverse: flags.contains(CellFlags::INVERSE),
        },
        underline_color: cell.underline_color().map_or(Color::Default, frame_color),
    }
}

/// Map alacritty underline flags (set from SGR 4 / 4:0..4:5 / 24) to the
/// canonical style. The emulator holds at most one underline flag per cell
/// (each SGR 4:x clears the rest); the order below is defensive only.
fn frame_underline_style(flags: CellFlags) -> UnderlineStyle {
    if flags.contains(CellFlags::DOUBLE_UNDERLINE) {
        UnderlineStyle::Double
    } else if flags.contains(CellFlags::UNDERCURL) {
        UnderlineStyle::Curly
    } else if flags.contains(CellFlags::DOTTED_UNDERLINE) {
        UnderlineStyle::Dotted
    } else if flags.contains(CellFlags::DASHED_UNDERLINE) {
        UnderlineStyle::Dashed
    } else if flags.contains(CellFlags::UNDERLINE) {
        UnderlineStyle::Single
    } else {
        UnderlineStyle::None
    }
}

fn frame_color(c: VteColor) -> Color {
    match c {
        VteColor::Named(n) => match n {
            NamedColor::Black | NamedColor::DimBlack => Color::Indexed(0),
            NamedColor::Red | NamedColor::DimRed => Color::Indexed(1),
            NamedColor::Green | NamedColor::DimGreen => Color::Indexed(2),
            NamedColor::Yellow | NamedColor::DimYellow => Color::Indexed(3),
            NamedColor::Blue | NamedColor::DimBlue => Color::Indexed(4),
            NamedColor::Magenta | NamedColor::DimMagenta => Color::Indexed(5),
            NamedColor::Cyan | NamedColor::DimCyan => Color::Indexed(6),
            NamedColor::White | NamedColor::DimWhite => Color::Indexed(7),
            NamedColor::BrightBlack => Color::Indexed(8),
            NamedColor::BrightRed => Color::Indexed(9),
            NamedColor::BrightGreen => Color::Indexed(10),
            NamedColor::BrightYellow => Color::Indexed(11),
            NamedColor::BrightBlue => Color::Indexed(12),
            NamedColor::BrightMagenta => Color::Indexed(13),
            NamedColor::BrightCyan => Color::Indexed(14),
            NamedColor::BrightWhite => Color::Indexed(15),
            NamedColor::Foreground
            | NamedColor::Background
            | NamedColor::Cursor
            | NamedColor::BrightForeground
            | NamedColor::DimForeground => Color::Default,
        },
        VteColor::Spec(rgb) => Color::Rgb(Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        }),
        VteColor::Indexed(i) => Color::Indexed(i),
    }
}

fn frame_cursor<T: EventListener>(
    term: &Term<T>,
    line: i32,
    column: usize,
    cols: u16,
    rows: u16,
) -> Cursor {
    let style = term.cursor_style();
    let shape = match style.shape {
        CursorShape::Block | CursorShape::HollowBlock | CursorShape::Hidden => CursorStyle::Block,
        CursorShape::Underline => CursorStyle::Underline,
        CursorShape::Beam => CursorStyle::Bar,
    };
    let visible = term.mode().contains(TermMode::SHOW_CURSOR)
        && !matches!(style.shape, CursorShape::Hidden)
        && line >= 0
        && line.cast_unsigned() < u32::from(rows)
        && column < cols as usize;
    if !visible {
        return Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: shape,
            blinking: style.blinking,
        };
    }
    Cursor {
        x: u16::try_from(column).unwrap_or(u16::MAX),
        y: u16::try_from(line).unwrap_or(u16::MAX),
        visible: true,
        style: shape,
        blinking: style.blinking,
    }
}

/// Map live `TermMode` bits to DEC/private mode numbers.
fn push_modes(mode: TermMode, out: &mut Vec<u16>) {
    let mut push = |flag: TermMode, n: u16| {
        if mode.contains(flag) {
            out.push(n);
        }
    };
    push(TermMode::APP_CURSOR, 1);
    push(TermMode::INSERT, 4);
    push(TermMode::ORIGIN, 6);
    push(TermMode::LINE_WRAP, 7);
    push(TermMode::LINE_FEED_NEW_LINE, 20);
    push(TermMode::APP_KEYPAD, 66);
    push(TermMode::MOUSE_REPORT_CLICK, 1000);
    push(TermMode::MOUSE_DRAG, 1002);
    push(TermMode::MOUSE_MOTION, 1003);
    push(TermMode::FOCUS_IN_OUT, 1004);
    push(TermMode::UTF8_MOUSE, 1005);
    push(TermMode::SGR_MOUSE, 1006);
    push(TermMode::ALT_SCREEN, 1049);
    push(TermMode::BRACKETED_PASTE, 2004);
    if mode.intersects(TermMode::KITTY_KEYBOARD_PROTOCOL) {
        out.push(57399);
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| {
        u64::try_from(d.as_millis().min(u128::from(u64::MAX))).unwrap_or(u64::MAX)
    })
}
