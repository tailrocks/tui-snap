//! Canonical frame schema (v3): the single artifact both capture paths share.
//!
//! ```text
//! fixture model + view state + viewport + theme ──▶ production Ratatui view ──▶ Frame
//! real executable ──▶ PTY + terminal-state engine ──▶ Frame
//! ```
//!
//! A [`Frame`] preserves grapheme content, cell positions and widths
//! (including wide-cell continuations), default/indexed/RGB colors, the
//! supported modifier set, cursor state, and provenance. It deliberately does
//! NOT preserve terminal-protocol details that do not affect the visible
//! grid (hyperlink targets, kitty image payloads, blink phase): those belong
//! in additional assertions, not in a screenshot contract.
//!
//! Import is strict: [`Frame::validate`] rejects malformed frames with an
//! explicit error instead of guessing.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Schema version. Bump on any incompatible change and migrate readers.
pub const FRAME_VERSION: u8 = 3;

/// Maximum viewport dimension accepted on import (DoS bound).
pub const MAX_DIM: u16 = 512;

/// RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

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
    #[default]
    Default,
    Indexed(u8),
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

/// Underline style (SGR 4 / 4:x; kitty/ITU numbering: `4:1` single through
/// `4:5` dashed, `4:0`/`24` cancel). Stored in [`Mods::underline_style`]
/// alongside the legacy [`Mods::underline`] bool; readers use
/// [`Mods::effective_underline_style`] so both agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
pub enum UnderlineStyle {
    /// No underline (default; omitted from stored JSON and snapshots).
    #[default]
    None,
    /// SGR 4 / 4:1.
    Single,
    /// SGR 4:2.
    Double,
    /// SGR 4:3 (undercurl).
    Curly,
    /// SGR 4:4.
    Dotted,
    /// SGR 4:5.
    Dashed,
}

impl UnderlineStyle {
    /// Any visible underline, regardless of style.
    #[must_use]
    pub fn is_some(self) -> bool {
        !matches!(self, UnderlineStyle::None)
    }

    /// No underline. Used by `skip_serializing_if` so stored frames stay sparse.
    #[must_use]
    pub fn is_none(&self) -> bool {
        matches!(self, UnderlineStyle::None)
    }

    /// Canonical token used by text/JSON projections (`None` has no token;
    /// callers omit it).
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            UnderlineStyle::None => "-",
            UnderlineStyle::Single => "underline",
            UnderlineStyle::Double => "double-underline",
            UnderlineStyle::Curly => "undercurl",
            UnderlineStyle::Dotted => "dotted-underline",
            UnderlineStyle::Dashed => "dashed-underline",
        }
    }
}

/// Cell modifiers retained in canonical data. Blink phase is frozen visible;
/// hidden glyphs are omitted by renderers but their source symbols remain in
/// canonical data. Concealment is not redaction: never capture real secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Mods {
    pub hidden: bool,
    pub blink: bool,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    /// Underline style refinement (SGR 4:x). Additive v3 field: missing in
    /// legacy files (defaults to `None`) and omitted from stored JSON when
    /// `None`, so only new data carries it. Producer invariant:
    /// `underline == underline_style.is_some()`; legacy `underline=true`
    /// cells with `None` read as Single via
    /// [`Mods::effective_underline_style`].
    #[serde(default, skip_serializing_if = "UnderlineStyle::is_none")]
    pub underline_style: UnderlineStyle,
    pub strikethrough: bool,
    pub reverse: bool,
}

impl Mods {
    /// Effective underline style: the explicit style when set, else Single
    /// for legacy `underline=true` cells, else None. All readers (canonical
    /// projections, renderer, SGR dump, assertions) use this so legacy and
    /// new data agree.
    #[must_use]
    pub fn effective_underline_style(self) -> UnderlineStyle {
        if self.underline_style.is_some() {
            self.underline_style
        } else if self.underline {
            UnderlineStyle::Single
        } else {
            UnderlineStyle::None
        }
    }
}

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

/// Where a frame came from. Recorded so reports are auditable; `created_unix`
/// is informational only and excluded from equality comparisons that must be
/// deterministic (use [`Frame::digest`] / cell comparison for gates).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub tool: String,
    pub tool_version: String,
    pub profile: String,
    pub source: String,
    pub argv: Vec<String>,
    pub created_unix: u64,
}

impl Provenance {
    #[must_use]
    pub fn now(profile: &str, source: &str, argv: Vec<String>) -> Self {
        Self {
            tool: "tuisnap".to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            profile: profile.to_string(),
            source: source.to_string(),
            argv,
            created_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }
}

/// The canonical frame: `rows` × `cols` cells in row-major order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub version: u8,
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Cell>,
    pub cursor: Cursor,
    pub provenance: Provenance,
}

/// Import/validation failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError(pub String);

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid frame: {}", self.0)
    }
}

impl std::error::Error for FrameError {}

impl Frame {
    #[must_use]
    pub fn blank(cols: u16, rows: u16, provenance: Provenance) -> Self {
        assert!(
            cols > 0 && rows > 0 && cols <= MAX_DIM && rows <= MAX_DIM,
            "blank frame dimensions out of range: {cols}x{rows}"
        );
        let mut cells = Vec::with_capacity(cols as usize * rows as usize);
        for y in 0..rows {
            for x in 0..cols {
                cells.push(Cell::blank(x, y));
            }
        }
        Self {
            version: FRAME_VERSION,
            cols,
            rows,
            cells,
            cursor: Cursor::default(),
            provenance,
        }
    }

    fn idx(&self, x: u16, y: u16) -> Option<usize> {
        if x < self.cols && y < self.rows {
            Some(y as usize * self.cols as usize + x as usize)
        } else {
            None
        }
    }

    pub fn set(&mut self, cell: Cell) {
        if let Some(i) = self.idx(cell.x, cell.y) {
            self.cells[i] = cell;
        }
    }

    #[must_use]
    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        self.idx(x, y).map(|i| &self.cells[i])
    }

    /// Strict validation for imports. Rejects: wrong version, zero/oversize
    /// dimensions, cell-count mismatch, out-of-order or out-of-bounds cells,
    /// bad widths, dangling continuations, empty lead symbols.
    pub fn validate(&self) -> Result<(), FrameError> {
        let bad = |m: &str| FrameError(m.to_string());
        if self.version != FRAME_VERSION {
            return Err(bad(&format!(
                "unsupported version {}, want {FRAME_VERSION}",
                self.version
            )));
        }
        if self.cols == 0 || self.rows == 0 {
            return Err(bad("dimensions must be nonzero"));
        }
        if self.cols > MAX_DIM || self.rows > MAX_DIM {
            return Err(bad(&format!(
                "dimensions {}x{} exceed max {MAX_DIM}",
                self.cols, self.rows
            )));
        }
        if self.cells.len() != self.cols as usize * self.rows as usize {
            return Err(bad(&format!(
                "cell count {} != {}x{}",
                self.cells.len(),
                self.cols,
                self.rows
            )));
        }
        for (i, c) in self.cells.iter().enumerate() {
            let (ex, ey) = (
                (i % self.cols as usize) as u16,
                (i / self.cols as usize) as u16,
            );
            if c.x != ex || c.y != ey {
                return Err(bad(&format!(
                    "cell {i} positioned at ({},{}) but stored at ({ex},{ey})",
                    c.x, c.y
                )));
            }
            match (c.width, c.continuation) {
                (0, true) => {
                    if c.x == 0 {
                        return Err(bad(&format!("continuation at row start ({},{})", c.x, c.y)));
                    }
                    let lead = &self.cells[i - 1];
                    if lead.width != 2 || lead.continuation {
                        return Err(bad(&format!("dangling continuation at ({},{})", c.x, c.y)));
                    }
                    if !c.symbol.is_empty() {
                        return Err(bad(&format!(
                            "continuation at ({},{}) must have empty symbol",
                            c.x, c.y
                        )));
                    }
                }
                (1 | 2, false) => {
                    if c.symbol.is_empty() {
                        return Err(bad(&format!(
                            "lead cell at ({},{}) must have a symbol",
                            c.x, c.y
                        )));
                    }
                    // Cross-check display width against unicode-width so a
                    // 1-cell glyph can never claim 2 cells (the renderer
                    // would span it wrongly and digests would lie).
                    let measured = unicode_width::UnicodeWidthStr::width(c.symbol.as_str());
                    // Combining sequences measure 1; anything wider than
                    // claimed, or a width-2 claim on a narrow symbol, is
                    // rejected. (East-Asian Ambiguous treatment follows
                    // unicode-width; see renderer docs.)
                    if c.width == 2 && measured != 2 {
                        return Err(bad(&format!(
                            "cell at ({},{}) claims width 2 for {:?} (measured {measured})",
                            c.x, c.y, c.symbol
                        )));
                    }
                    if c.width == 1 && measured > 1 {
                        return Err(bad(&format!(
                            "cell at ({},{}) claims width 1 for {:?} (measured {measured})",
                            c.x, c.y, c.symbol
                        )));
                    }
                    if c.width == 2 {
                        if c.x + 1 >= self.cols {
                            return Err(bad(&format!(
                                "wide cell at ({},{}) overflows row",
                                c.x, c.y
                            )));
                        }
                        let next = &self.cells[i + 1];
                        if next.width != 0 || !next.continuation {
                            return Err(bad(&format!(
                                "wide cell at ({},{}) missing continuation",
                                c.x, c.y
                            )));
                        }
                    }
                }
                _ => {
                    return Err(bad(&format!(
                        "bad width/continuation at ({},{}): width={} continuation={}",
                        c.x, c.y, c.width, c.continuation
                    )));
                }
            }
        }
        if self.cursor.visible && (self.cursor.x >= self.cols || self.cursor.y >= self.rows) {
            return Err(bad("visible cursor outside grid"));
        }
        Ok(())
    }

    /// Parse + validate canonical JSON. Legacy bool-only underlines
    /// (`underline=true` with no style key) normalize to the producer form
    /// (`Single`): in v3 the bool could only mean single, so this loses no
    /// information and keeps representation out of comparison — in memory,
    /// `underline == underline_style.is_some()` always holds.
    pub fn from_json(text: &str) -> Result<Self, FrameError> {
        let mut frame: Self =
            serde_json::from_str(text).map_err(|e| FrameError(format!("bad JSON: {e}")))?;
        frame.validate()?;
        for c in &mut frame.cells {
            if c.mods.underline && c.mods.underline_style.is_none() {
                c.mods.underline_style = UnderlineStyle::Single;
            }
        }
        Ok(frame)
    }

    /// Compact canonical JSON: the stored/transmitted form. Pretty-printing
    /// is for humans only and must never be a gate input (whitespace is not
    /// significant; parsers accept both).
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Frame is always serializable")
    }

    /// Pretty JSON for human display (report `<pre>`, debugging).
    #[must_use]
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("Frame is always serializable")
    }

    /// Plain text: rows joined with `\n`, trailing blanks trimmed per row.
    /// Continuation cells contribute nothing (the lead already holds the symbol).
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for y in 0..self.rows {
            if y > 0 {
                out.push('\n');
            }
            let mut row = String::new();
            for x in 0..self.cols {
                if let Some(c) = self.get(x, y) {
                    if !c.continuation {
                        row.push_str(&c.symbol);
                    }
                }
            }
            out.push_str(row.trim_end());
        }
        out
    }

    /// Deterministic content digest over cells + cursor (provenance excluded:
    /// timestamps must not break gates). 16-hex display via format!("{digest:016x}").
    #[must_use]
    pub fn digest(&self) -> u64 {
        const OFF: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0100_0000_01b3;
        fn mix(mut h: u64, bytes: &[u8]) -> u64 {
            for b in bytes {
                h ^= u64::from(*b);
                h = h.wrapping_mul(PRIME);
            }
            h
        }
        let mut h = OFF;
        h = mix(h, &[self.version]);
        h = mix(h, &self.cols.to_le_bytes());
        h = mix(h, &self.rows.to_le_bytes());
        for c in &self.cells {
            h = mix(h, &c.x.to_le_bytes());
            h = mix(h, &c.y.to_le_bytes());
            h = mix(h, c.symbol.as_bytes());
            h = mix(h, &[c.width, u8::from(c.continuation)]);
            h = mix(
                h,
                format!("{:?}|{:?}|{:?}|{:?}", c.fg, c.bg, c.mods, c.underline_color).as_bytes(),
            );
        }
        h = mix(
            h,
            format!(
                "{},{},{},{:?},{}",
                self.cursor.x,
                self.cursor.y,
                self.cursor.visible,
                self.cursor.style,
                self.cursor.blinking
            )
            .as_bytes(),
        );
        h
    }

    /// Exact cell comparison. Returns differing (x, y) positions; cursor
    /// differences are reported AT the cursor position, and callers must
    /// render cursor-aware summaries (see [`summarize_cursor`]): when only
    /// the cursor changed, the cells at that position compare equal.
    /// Dimensions must match; a dimension mismatch is an Err, not a diff.
    pub fn diff_cells(&self, other: &Self) -> Result<Vec<(u16, u16)>, FrameError> {
        if self.cols != other.cols || self.rows != other.rows {
            return Err(FrameError(format!(
                "dimension mismatch: {}x{} vs {}x{}",
                self.cols, self.rows, other.cols, other.rows
            )));
        }
        let mut out = Vec::new();
        for (a, b) in self.cells.iter().zip(other.cells.iter()) {
            if a.symbol != b.symbol
                || a.width != b.width
                || a.continuation != b.continuation
                || a.fg != b.fg
                || a.bg != b.bg
                || a.mods != b.mods
                || a.underline_color != b.underline_color
            {
                out.push((a.x, a.y));
            }
        }
        if self.cursor != other.cursor {
            out.push((other.cursor.x, other.cursor.y));
        }
        Ok(out)
    }

    /// Human-readable cursor summary for cursor-only diagnostics.
    #[must_use]
    pub fn summarize_cursor(c: &Cursor) -> String {
        format!(
            "cursor ({},{}) visible={} style={:?} blink={}",
            c.x, c.y, c.visible, c.style, c.blinking
        )
    }

    /// Downgrade a trailing wide lead (last column, no room for its
    /// continuation) to width 1. Converters call this when an emulator or
    /// buffer hands them a wide grapheme at the exact row end instead of
    /// wrapping it — without it the frame would fail validation.
    pub fn clamp_trailing_wide(&mut self) {
        if self.cols == 0 {
            return;
        }
        for y in 0..self.rows {
            let i = y as usize * self.cols as usize + (self.cols as usize - 1);
            if self.cells[i].width == 2 && !self.cells[i].continuation {
                self.cells[i].width = 1;
            }
        }
    }
    /// Returns (fg, bg) after reverse/underline-color/dim handling.
    #[must_use]
    pub fn resolve_cell(cell: &Cell, default_fg: Rgb, default_bg: Rgb) -> (Rgb, Rgb) {
        fn rgb(c: Color, dflt: Rgb) -> Rgb {
            match c {
                Color::Default => dflt,
                Color::Indexed(i) => Rgb::from_indexed(i),
                Color::Rgb(r) => r,
            }
        }
        let (mut fg, mut bg) = (rgb(cell.fg, default_fg), rgb(cell.bg, default_bg));
        if cell.mods.reverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.mods.dim {
            // 60% fg over bg (matches common terminal dim treatment).
            let mix = |f: u8, b: u8| ((u32::from(f) * 6 + u32::from(b) * 4) / 10) as u8;
            fg = Rgb::new(mix(fg.r, bg.r), mix(fg.g, bg.g), mix(fg.b, bg.b));
        }
        (fg, bg)
    }

    /// Provenance-keyed identity for logs (never a gate by itself).
    #[must_use]
    pub fn key(&self, name: &str) -> String {
        format!("{name} {} {}", self.cols, self.rows)
    }

    /// Deterministic reruns must produce identical digests AND identical PNG
    /// bytes under the same profile; see `render` tests.
    pub fn palette_map() -> BTreeMap<u8, Rgb> {
        (0..=255).map(|i| (i, Rgb::from_indexed(i))).collect()
    }
}
