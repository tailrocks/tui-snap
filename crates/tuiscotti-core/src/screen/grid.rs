use super::{MAX_DIM, Region, RegionPolicy, ScreenError};
use crate::frame::{Cell, Cursor, Frame};

// ---------------------------------------------------------------------------
// Screen
// ---------------------------------------------------------------------------

/// Immutable validated grid.
///
/// Dimensions (`cols`/`rows`) plus origin (`ox`/`oy`, the position of cell
/// (0,0) in a larger coordinate plane) plus a full row-major [`Cell`] grid and
/// cursor intent. Cells carry grid-local coordinates matching their row-major
/// index; the origin is positional metadata, not part of cell coordinates.
///
/// There are no PTY minimums: 1x1, single-column, and single-row screens are
/// valid (M04). Construction is fallible: [`Screen::validate`] rejects wrong
/// cell counts, out-of-range coordinates, widths above 2, continuations with
/// nonempty symbols, zero/oversize dimensions, and orphan continuations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Screen {
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) ox: i32,
    pub(crate) oy: i32,
    pub(crate) cells: Vec<Cell>,
    pub(crate) cursor: Cursor,
}

impl Screen {
    /// Validated import from a canonical [`Frame`] (M02). The frame is
    /// validated first; all source distinctions (colors, modifiers incl.
    /// hidden/blink, styled blanks, continuations, cursor) are preserved.
    /// Imported screens sit at origin (0,0). Also available as
    /// `TryFrom<&Frame>` for generic conversion sites.
    ///
    /// # Errors
    ///
    /// Returns [`ScreenError`] when the frame fails validation.
    pub fn from_frame(frame: &Frame) -> Result<Self, ScreenError> {
        frame
            .validate()
            .map_err(|e| ScreenError(format!("bad frame import: {e}")))?;
        Self::validate(
            frame.cols,
            frame.rows,
            0,
            0,
            frame.cells.clone(),
            frame.cursor,
        )
    }

    /// Blank screen at origin (0,0) with a hidden default cursor.
    ///
    /// # Panics
    ///
    /// Panics on zero/oversize dimensions like [`Frame::blank`].
    #[must_use]
    pub fn blank(cols: u16, rows: u16) -> Self {
        assert!(
            cols > 0 && rows > 0 && cols <= MAX_DIM && rows <= MAX_DIM,
            "blank screen dimensions out of range: {cols}x{rows}"
        );
        let mut cells = Vec::with_capacity(cols as usize * rows as usize);
        for y in 0..rows {
            for x in 0..cols {
                cells.push(Cell::blank(x, y));
            }
        }
        Self {
            cols,
            rows,
            ox: 0,
            oy: 0,
            cells,
            cursor: Cursor::default(),
        }
    }

    /// Grid width in columns.
    #[must_use]
    pub fn cols(&self) -> u16 {
        self.cols
    }

    /// Grid height in rows.
    #[must_use]
    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// Origin of cell (0,0) in the larger coordinate plane.
    #[must_use]
    pub fn origin(&self) -> (i32, i32) {
        (self.ox, self.oy)
    }

    /// Row-major cells, `rows` × `cols` entries.
    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Cursor state.
    #[must_use]
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// Cell at `(x, y)`, or `None` when out of bounds.
    #[must_use]
    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        if x < self.cols && y < self.rows {
            Some(&self.cells[y as usize * self.cols as usize + x as usize])
        } else {
            None
        }
    }

    /// Crop a sub-region (grid-local `x`, `y`, `cols`, `rows`) with geometry
    /// and origin preserved: the region's origin is this screen's origin plus
    /// the crop offset, and cells are re-indexed to region-local coordinates.
    ///
    /// Refuses to split a wide grapheme: if the left edge would cut a
    /// continuation cell, or the right edge would strand a wide lead without
    /// its continuation, returns an error naming the grapheme (M07).
    ///
    /// # Errors
    ///
    /// Returns [`ScreenError`] for zero dimensions, out-of-bounds rects, or
    /// crops that would split a wide grapheme.
    pub fn region(
        &self,
        x: u16,
        y: u16,
        cols: u16,
        rows: u16,
        policy: RegionPolicy,
    ) -> Result<Region, ScreenError> {
        let bad = |m: String| ScreenError(m);
        if cols == 0 || rows == 0 {
            return Err(bad("region dimensions must be nonzero".to_string()));
        }
        if u32::from(x) + u32::from(cols) > u32::from(self.cols)
            || u32::from(y) + u32::from(rows) > u32::from(self.rows)
        {
            return Err(bad(format!(
                "region ({x},{y}) {cols}x{rows} outside {}x{} screen",
                self.cols, self.rows
            )));
        }
        // Wide-grapheme split checks, one row at a time.
        for r in y..y + rows {
            let left = &self.cells[r as usize * self.cols as usize + x as usize];
            if left.continuation {
                let lead = &self.cells[r as usize * self.cols as usize + x as usize - 1];
                return Err(bad(format!(
                    "crop at ({x},{r}) splits wide grapheme {:?}: continuation without its lead",
                    lead.symbol
                )));
            }
            let right = &self.cells[r as usize * self.cols as usize + (x + cols - 1) as usize];
            if right.width == 2 && !right.continuation {
                return Err(bad(format!(
                    "crop at ({},{r}) splits wide grapheme {:?}: lead without its continuation",
                    x + cols - 1,
                    right.symbol
                )));
            }
        }
        let mut cells = Vec::with_capacity(cols as usize * rows as usize);
        for ry in 0..rows {
            for rx in 0..cols {
                let mut c =
                    self.cells[(y + ry) as usize * self.cols as usize + (x + rx) as usize].clone();
                c.x = rx;
                c.y = ry;
                cells.push(c);
            }
        }
        let mut cursor = self.cursor;
        if cursor.visible {
            if cursor.x >= x && cursor.x < x + cols && cursor.y >= y && cursor.y < y + rows {
                cursor.x -= x;
                cursor.y -= y;
            } else {
                cursor.visible = false;
            }
        }
        let screen = Self::validate(
            cols,
            rows,
            self.ox + i32::from(x),
            self.oy + i32::from(y),
            cells,
            cursor,
        )?;
        Ok(Region { screen, policy })
    }
}

impl TryFrom<&Frame> for Screen {
    type Error = ScreenError;

    /// Fallible conversion identical to [`Screen::from_frame`].
    fn try_from(frame: &Frame) -> Result<Self, Self::Error> {
        Self::from_frame(frame)
    }
}
