use crate::frame::{Cell, Color, Mods, UnderlineStyle};
use ratatui::buffer::Cell as RCell;
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

pub(crate) fn convert_color(c: ratatui::style::Color) -> Color {
    use ratatui::style::Color as C;
    match c {
        C::Reset => Color::Default,
        C::Black => Color::Indexed(0),
        C::Red => Color::Indexed(1),
        C::Green => Color::Indexed(2),
        C::Yellow => Color::Indexed(3),
        C::Blue => Color::Indexed(4),
        C::Magenta => Color::Indexed(5),
        C::Cyan => Color::Indexed(6),
        C::Gray => Color::Indexed(7),
        C::DarkGray => Color::Indexed(8),
        C::LightRed => Color::Indexed(9),
        C::LightGreen => Color::Indexed(10),
        C::LightYellow => Color::Indexed(11),
        C::LightBlue => Color::Indexed(12),
        C::LightMagenta => Color::Indexed(13),
        C::LightCyan => Color::Indexed(14),
        C::White => Color::Indexed(15),
        C::Indexed(i) => Color::Indexed(i),
        C::Rgb(r, g, b) => Color::Rgb(crate::frame::Rgb::new(r, g, b)),
    }
}

pub(crate) fn convert_mods(m: Modifier) -> Mods {
    Mods {
        hidden: m.contains(Modifier::HIDDEN),
        blink: m.intersects(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK),
        bold: m.contains(Modifier::BOLD),
        dim: m.contains(Modifier::DIM),
        italic: m.contains(Modifier::ITALIC),
        // Ratatui exposes no underline STYLE (only the UNDERLINED bit), so
        // any ratatui underline is Single; see the `screen_from_buffer` table.
        underline: m.contains(Modifier::UNDERLINED),
        underline_style: if m.contains(Modifier::UNDERLINED) {
            UnderlineStyle::Single
        } else {
            UnderlineStyle::None
        },
        strikethrough: m.contains(Modifier::CROSSED_OUT),
        reverse: m.contains(Modifier::REVERSED),
    }
}

/// Convert one buffer cell. Wide symbols (width 2) produce the lead cell;
/// the caller emits the continuation follower.
pub(crate) fn lead_cell(x: u16, y: u16, rc: &RCell) -> (Cell, u8) {
    let symbol = rc.symbol().to_string();
    let width = UnicodeWidthStr::width(symbol.as_str()).min(2) as u8;
    let width = width.max(1);
    (
        Cell {
            x,
            y,
            symbol,
            width,
            continuation: false,
            fg: convert_color(rc.fg),
            bg: convert_color(rc.bg),
            mods: convert_mods(rc.modifier),
            underline_color: convert_color(rc.underline_color),
        },
        width,
    )
}
