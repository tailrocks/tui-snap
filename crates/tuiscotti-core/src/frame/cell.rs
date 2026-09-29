use super::{Color, Mods};
use serde::{Deserialize, Serialize};

/// One grid cell.
///
/// Wide graphemes occupy two columns: the lead cell carries
/// `width = 2` and the symbol, the follower carries `width = 0`,
/// `continuation = true`, and an empty symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub x: u16,
    pub y: u16,
    /// Visible grapheme cluster ("" for continuation cells and blanks-as-space).
    pub symbol: String,
    /// Display width in columns: 0 (continuation), 1, or 2 (wide).
    pub width: u8,
    pub continuation: bool,
    pub fg: Color,
    pub bg: Color,
    pub mods: Mods,
    /// Underline color (SGR 58; SGR 59 resets). `Default` follows the
    /// resolved foreground. Omitted from stored JSON when default.
    #[serde(default, skip_serializing_if = "Color::is_default")]
    pub underline_color: Color,
}

impl Cell {
    #[must_use]
    pub fn blank(x: u16, y: u16) -> Self {
        Self {
            x,
            y,
            symbol: " ".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Default,
            bg: Color::Default,
            mods: Mods::default(),
            underline_color: Color::Default,
        }
    }
}

/// Cursor visual style (blink phase is frozen as visible; see module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

/// Terminal cursor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
    pub style: CursorStyle,
    pub blinking: bool,
}
