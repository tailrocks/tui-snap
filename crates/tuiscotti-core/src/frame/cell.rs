use super::{Color, Mods};
use serde::{Deserialize, Serialize};

/// One grid cell.
///
/// Wide graphemes occupy two columns: the lead cell carries
/// `width = 2` and the symbol, the follower carries `width = 0`,
/// `continuation = true`, and an empty symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// Grid column (0-based, grid-local).
    pub x: u16,
    /// Grid row (0-based, grid-local).
    pub y: u16,
    /// Visible grapheme cluster ("" for continuation cells and blanks-as-space).
    pub symbol: String,
    /// Display width in columns: 0 (continuation), 1, or 2 (wide).
    pub width: u8,
    /// Follower of a wide lead: empty symbol, carries the lead's style.
    pub continuation: bool,
    /// Foreground color.
    pub fg: Color,
    /// Background color.
    pub bg: Color,
    /// Cell modifiers.
    pub mods: Mods,
    /// Underline color (SGR 58; SGR 59 resets). `Default` follows the
    /// resolved foreground. Omitted from stored JSON when default.
    #[serde(default, skip_serializing_if = "Color::is_default")]
    pub underline_color: Color,
}

impl Cell {
    /// Blank cell at `(x, y)`: space symbol, default colors, no modifiers.
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
    /// Full-cell block (default).
    #[default]
    Block,
    /// Underscore cursor.
    Underline,
    /// Vertical bar cursor.
    Bar,
}

/// Terminal cursor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cursor {
    /// Grid column (0-based, grid-local).
    pub x: u16,
    /// Grid row (0-based, grid-local).
    pub y: u16,
    /// Whether the cursor is shown.
    pub visible: bool,
    /// Cursor shape.
    pub style: CursorStyle,
    /// Blink intent (stills freeze the phase as visible).
    pub blinking: bool,
}
