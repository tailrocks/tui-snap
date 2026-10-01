use serde::{Deserialize, Serialize};

/// RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    /// Red channel (0-255).
    pub r: u8,
    /// Green channel (0-255).
    pub g: u8,
    /// Blue channel (0-255).
    pub b: u8,
}

impl Rgb {
    /// Direct RGB color from channels.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Lowercase `#rrggbb` hex form.
    #[must_use]
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Standard xterm palette entry (same table terminals use).
    #[must_use]
    pub fn from_indexed(n: u8) -> Self {
        const BASIC: [[u8; 3]; 16] = [
            [0, 0, 0],
            [205, 49, 49],
            [13, 188, 121],
            [229, 229, 16],
            [36, 114, 200],
            [188, 63, 188],
            [17, 168, 205],
            [229, 229, 229],
            [102, 102, 102],
            [241, 76, 76],
            [35, 209, 139],
            [245, 245, 67],
            [59, 142, 234],
            [214, 112, 214],
            [41, 184, 219],
            [255, 255, 255],
        ];
        if n < 16 {
            let c = BASIC[n as usize];
            return Self::new(c[0], c[1], c[2]);
        }
        if n < 232 {
            let n = n - 16;
            let conv = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            return Self::new(conv(n / 36), conv((n / 6) % 6), conv(n % 6));
        }
        let v = 8 + (n - 232) * 10;
        Self::new(v, v, v)
    }
}

/// A cell color: terminal default, palette index, or direct RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Color {
    /// Terminal default (context-resolved).
    #[default]
    Default,
    /// Palette entry 0-255.
    Indexed(u8),
    /// Direct RGB color.
    Rgb(Rgb),
}

impl Color {
    /// `true` for [`Color::Default`]. Used by `skip_serializing_if` so stored
    /// frames stay sparse.
    #[must_use]
    pub fn is_default(&self) -> bool {
        matches!(self, Color::Default)
    }
}
