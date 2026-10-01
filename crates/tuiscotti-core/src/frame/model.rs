use super::{Cell, Color, Cursor, FrameError, Provenance, Rgb, UnderlineStyle};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Schema version. Bump on any incompatible change and migrate readers.
pub const FRAME_VERSION: u8 = 3;

/// Maximum viewport dimension accepted on import (`DoS` bound).
pub const MAX_DIM: u16 = 512;

/// The canonical frame: `rows` × `cols` cells in row-major order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    /// Schema version; must equal [`FRAME_VERSION`].
    pub version: u8,
    /// Grid width in columns.
    pub cols: u16,
    /// Grid height in rows.
    pub rows: u16,
    /// Row-major cells, `rows` × `cols` entries.
    pub cells: Vec<Cell>,
    /// Cursor state.
    pub cursor: Cursor,
    /// Capture provenance (excluded from digests).
    pub provenance: Provenance,
}

impl Frame {
    /// Blank frame: all-space cells, hidden default cursor.
    ///
    /// # Panics
    ///
    /// Panics when either dimension is zero or exceeds [`MAX_DIM`].
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

    /// Store `cell` at its own `(x, y)`; out-of-bounds cells are ignored.
    pub fn set(&mut self, cell: Cell) {
        if let Some(i) = self.idx(cell.x, cell.y) {
            self.cells[i] = cell;
        }
    }

    /// Cell at `(x, y)`, or `None` when out of bounds.
    #[must_use]
    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        self.idx(x, y).map(|i| &self.cells[i])
    }

    /// Parse + validate canonical JSON. Legacy bool-only underlines
    /// (`underline=true` with no style key) normalize to the producer form
    /// (`Single`): in v3 the bool could only mean single, so this loses no
    /// information and keeps representation out of comparison — in memory,
    /// `underline == underline_style.is_some()` always holds.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError`] when the text is not valid JSON or fails
    /// [`Frame::validate`].
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
    ///
    /// Serialization is total: the derived impl covers only infallible
    /// shapes (no string-keyed maps) and the `String` writer cannot fail.
    /// The empty-string fallback is unreachable; it fails loudly at parse.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Pretty JSON for human display (report `<pre>`, debugging).
    /// Total like [`Frame::to_json`]: the fallback is unreachable.
    #[must_use]
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
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
                if let Some(c) = self.get(x, y)
                    && !c.continuation
                {
                    row.push_str(&c.symbol);
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
    /// render cursor-aware summaries (see [`Frame::summarize_cursor`]): when only
    /// the cursor changed, the cells at that position compare equal.
    /// Dimensions must match; a dimension mismatch is an Err, not a diff.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError`] when the two frames differ in dimensions.
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
    pub fn resolve_cell(cell: &Cell, foreground: Rgb, background: Rgb) -> (Rgb, Rgb) {
        fn rgb(c: Color, dflt: Rgb) -> Rgb {
            match c {
                Color::Default => dflt,
                Color::Indexed(i) => Rgb::from_indexed(i),
                Color::Rgb(r) => r,
            }
        }
        let (mut fg, mut bg) = (rgb(cell.fg, foreground), rgb(cell.bg, background));
        if cell.mods.reverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.mods.dim {
            // 60% fg over bg (matches common terminal dim treatment).
            // Weighted mean of bytes is at most 255, so this never saturates.
            let mix = |f: u8, b: u8| {
                u8::try_from((u32::from(f) * 6 + u32::from(b) * 4) / 10).unwrap_or(u8::MAX)
            };
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
    #[must_use]
    pub fn palette_map() -> BTreeMap<u8, Rgb> {
        (0..=255).map(|i| (i, Rgb::from_indexed(i))).collect()
    }
}
