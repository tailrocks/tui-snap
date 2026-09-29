use super::{MAX_DIM, Screen, ScreenError};
use crate::frame::{Cell, Cursor};

/// Reject zero/oversize dimensions and cell-count mismatches.
fn check_dimensions(cols: u16, rows: u16, cell_count: usize) -> Result<(), ScreenError> {
    let bad = |m: String| ScreenError(m);
    if cols == 0 || rows == 0 {
        return Err(bad("dimensions must be nonzero".to_string()));
    }
    if cols > MAX_DIM || rows > MAX_DIM {
        return Err(bad(format!(
            "dimensions {cols}x{rows} exceed max {MAX_DIM}"
        )));
    }
    if cell_count != cols as usize * rows as usize {
        return Err(bad(format!("cell count {cell_count} != {cols}x{rows}")));
    }
    Ok(())
}

/// Reject a cell whose stored coordinates disagree with its row-major index,
/// or whose width exceeds 2.
fn check_position(c: &Cell, i: usize, cols: u16) -> Result<(), ScreenError> {
    let bad = |m: String| ScreenError(m);
    let (ex, ey) = ((i % cols as usize) as u16, (i / cols as usize) as u16);
    if c.x != ex || c.y != ey {
        return Err(bad(format!(
            "cell {i} positioned at ({},{}) but stored at ({ex},{ey})",
            c.x, c.y
        )));
    }
    if c.width > 2 {
        return Err(bad(format!(
            "cell at ({},{}) has width {} (max 2)",
            c.x, c.y, c.width
        )));
    }
    Ok(())
}

/// Reject a continuation cell with a nonempty symbol or without a width-2
/// lead to its left.
fn check_continuation(cells: &[Cell], i: usize, c: &Cell) -> Result<(), ScreenError> {
    let bad = |m: String| ScreenError(m);
    if !c.symbol.is_empty() {
        return Err(bad(format!(
            "continuation at ({},{}) must have empty symbol",
            c.x, c.y
        )));
    }
    if c.x == 0 {
        return Err(bad(format!(
            "orphan continuation at ({},{}): no lead cell to its left",
            c.x, c.y
        )));
    }
    let lead = &cells[i - 1];
    if lead.width != 2 || lead.continuation {
        return Err(bad(format!(
            "orphan continuation at ({},{}): lead has width {} continuation={}",
            c.x, c.y, lead.width, lead.continuation
        )));
    }
    Ok(())
}

/// Reject a lead cell with an empty symbol, a row-overflowing wide cell, or
/// a wide cell missing its continuation.
fn check_lead(cells: &[Cell], i: usize, c: &Cell, cols: u16) -> Result<(), ScreenError> {
    let bad = |m: String| ScreenError(m);
    if c.symbol.is_empty() {
        return Err(bad(format!(
            "lead cell at ({},{}) must have a symbol",
            c.x, c.y
        )));
    }
    if c.width == 2 {
        if c.x + 1 >= cols {
            return Err(bad(format!("wide cell at ({},{}) overflows row", c.x, c.y)));
        }
        let next = &cells[i + 1];
        if next.width != 0 || !next.continuation {
            return Err(bad(format!(
                "wide cell at ({},{}) missing continuation",
                c.x, c.y
            )));
        }
    }
    Ok(())
}

impl Screen {
    /// Fallible validated constructor. `cells` must be row-major over
    /// `cols` x `rows` with grid-local coordinates.
    pub fn validate(
        cols: u16,
        rows: u16,
        ox: i32,
        oy: i32,
        cells: Vec<Cell>,
        cursor: Cursor,
    ) -> Result<Self, ScreenError> {
        check_dimensions(cols, rows, cells.len())?;
        for (i, c) in cells.iter().enumerate() {
            check_position(c, i, cols)?;
            match (c.width, c.continuation) {
                (0, true) => check_continuation(&cells, i, c)?,
                (1 | 2, false) => check_lead(&cells, i, c, cols)?,
                _ => {
                    return Err(ScreenError(format!(
                        "bad width/continuation at ({},{}): width={} continuation={}",
                        c.x, c.y, c.width, c.continuation
                    )));
                }
            }
        }
        if cursor.visible && (cursor.x >= cols || cursor.y >= rows) {
            return Err(ScreenError("visible cursor outside grid".to_string()));
        }
        Ok(Self {
            cols,
            rows,
            ox,
            oy,
            cells,
            cursor,
        })
    }
}
