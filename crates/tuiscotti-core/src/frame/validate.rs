use super::{Cell, FRAME_VERSION, Frame, FrameError, MAX_DIM};

impl Frame {
    /// Strict validation for imports. Rejects: wrong version, zero/oversize
    /// dimensions, cell-count mismatch, out-of-order or out-of-bounds cells,
    /// bad widths, dangling continuations, empty lead symbols.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError`] describing the first defect found.
    pub fn validate(&self) -> Result<(), FrameError> {
        self.check_header()?;
        for (i, c) in self.cells.iter().enumerate() {
            self.check_position(c, i)?;
            match (c.width, c.continuation) {
                (0, true) => self.check_continuation(i, c)?,
                (1 | 2, false) => self.check_lead(i, c)?,
                _ => {
                    return Err(FrameError(format!(
                        "bad width/continuation at ({},{}): width={} continuation={}",
                        c.x, c.y, c.width, c.continuation
                    )));
                }
            }
        }
        if self.cursor.visible && (self.cursor.x >= self.cols || self.cursor.y >= self.rows) {
            return Err(FrameError("visible cursor outside grid".to_string()));
        }
        Ok(())
    }

    /// Reject wrong versions, zero/oversize dimensions, cell-count mismatches.
    fn check_header(&self) -> Result<(), FrameError> {
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
        Ok(())
    }

    /// Reject a cell whose stored coordinates disagree with its row-major index.
    fn check_position(&self, c: &Cell, i: usize) -> Result<(), FrameError> {
        let bad = |m: &str| FrameError(m.to_string());
        // Validated dims bound both below `MAX_DIM`, so this is infallible.
        let ex =
            u16::try_from(i % self.cols as usize).map_err(|_| bad("cell index overflows u16"))?;
        let ey =
            u16::try_from(i / self.cols as usize).map_err(|_| bad("cell index overflows u16"))?;
        if c.x != ex || c.y != ey {
            return Err(bad(&format!(
                "cell {i} positioned at ({},{}) but stored at ({ex},{ey})",
                c.x, c.y
            )));
        }
        Ok(())
    }

    /// Reject a row-start continuation, a dangling continuation, or one with a
    /// nonempty symbol.
    fn check_continuation(&self, i: usize, c: &Cell) -> Result<(), FrameError> {
        let bad = |m: &str| FrameError(m.to_string());
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
        Ok(())
    }

    /// Reject an empty lead symbol, a width claim disagreeing with
    /// unicode-width, a row-overflowing wide cell, or a wide cell missing its
    /// continuation.
    fn check_lead(&self, i: usize, c: &Cell) -> Result<(), FrameError> {
        let bad = |m: &str| FrameError(m.to_string());
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
        Ok(())
    }
}
