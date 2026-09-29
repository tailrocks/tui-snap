use std::sync::mpsc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{ClipboardType, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as VteColor, CursorShape, NamedColor};

use super::{ClipboardItem, ClipboardTarget, ReplayError, ReplayEvents};
use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, UnderlineStyle};
use tuiscotti_core::screen::Screen;

pub(crate) fn drain_replay_events<T: EventListener>(
    term: &mut Term<T>,
    event_rx: &mpsc::Receiver<Event>,
    events: &mut ReplayEvents,
) {
    let _ = term;
    while let Ok(event) = event_rx.try_recv() {
        match event {
            Event::Title(t) => events.title = Some(t),
            Event::ResetTitle => events.title = None,
            Event::Bell => events.bells += 1,
            Event::ClipboardStore(ty, text) => {
                let target = match ty {
                    ClipboardType::Clipboard => ClipboardTarget::Clipboard,
                    ClipboardType::Selection => ClipboardTarget::Selection,
                };
                events.clipboard.store(ClipboardItem { target, text });
            }
            // No PTY to answer: query replies are dropped, like the live
            // worker drops them once the writer is gone.
            _ => {}
        }
    }
}

/// Viewport grid + cursor, mirroring the live observation builder's mapping
/// rules (wide-char pairing, orphan spacers, color/cursor mapping).
pub(crate) fn build_replay_screen<T: EventListener>(
    term: &Term<T>,
    cols: u16,
    rows: u16,
) -> Result<Screen, ReplayError> {
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
            } else if flags
                .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
            {
                cells.push(Cell::blank(x, y));
                x += 1;
            } else {
                cells.push(replay_cell(x, y, cell, 1, false));
                x += 1;
            }
        }
    }
    let point = grid.cursor.point;
    let cursor = replay_cursor(term, point.line.0, point.column.0, cols, rows);
    Screen::validate(cols, rows, 0, 0, cells, cursor)
        .map_err(|e| ReplayError::ScreenBuild(e.to_string()))
}

fn replay_cell(
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
        fg: replay_color(cell.fg),
        bg: replay_color(cell.bg),
        mods: Mods {
            hidden: flags.contains(CellFlags::HIDDEN),
            blink: false,
            bold: flags.contains(CellFlags::BOLD),
            dim: flags.contains(CellFlags::DIM),
            italic: flags.contains(CellFlags::ITALIC),
            underline: flags.intersects(CellFlags::ALL_UNDERLINES),
            underline_style: replay_underline_style(flags),
            strikethrough: flags.contains(CellFlags::STRIKEOUT),
            reverse: flags.contains(CellFlags::INVERSE),
        },
        underline_color: cell.underline_color().map_or(Color::Default, replay_color),
    }
}

/// Map alacritty underline flags (set from SGR 4 / 4:0..4:5 / 24) to the
/// canonical style. The emulator holds at most one underline flag per cell
/// (each SGR 4:x clears the rest); the order below is defensive only.
fn replay_underline_style(flags: CellFlags) -> UnderlineStyle {
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

fn replay_color(c: VteColor) -> Color {
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

fn replay_cursor<T: EventListener>(
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
        && column < usize::from(cols);
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
