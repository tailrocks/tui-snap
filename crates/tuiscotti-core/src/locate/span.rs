/// One match: terminal cell coordinates plus provenance.
///
/// - Viewport spans: `(x, y)` is the grid-local start cell, `(end_x, end_y)`
///   the exclusive end (single-row matches have `end_y == y`).
/// - Scrollback spans: `scrollback` is true, `scrollback_index` is the line
///   index (oldest = 0), `x`/`end_x` are char offsets in that line, and `y` is
///   always 0 (there are no viewport rows to click).
/// - `origin` is the resolved screen's origin (region crops keep theirs, Q10).
/// - `width_cols` is display columns covered (wide leads count 2; wrapped
///   matches span rows, see docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Start column (viewport cell, or scrollback char offset).
    pub x: u16,
    /// Start row (always 0 for scrollback spans).
    pub y: u16,
    /// Exclusive end column.
    pub end_x: u16,
    /// End row (`end_y == y` for single-row matches).
    pub end_y: u16,
    /// Display columns covered (wide leads count 2).
    pub width_cols: u16,
    /// Origin of the resolved screen.
    pub origin: (i32, i32),
    /// Screen revision this span was resolved at.
    pub revision: u64,
    /// Matched text.
    pub text: String,
    /// True for scrollback (non-clickable) matches.
    pub scrollback: bool,
    /// Scrollback line index (oldest = 0); `None` for viewport spans.
    pub scrollback_index: Option<usize>,
}

impl Span {
    /// Total order key: scrollback (oldest first) before viewport (row-major).
    pub(crate) fn key(&self) -> (u8, usize, u16) {
        if self.scrollback {
            (0, self.scrollback_index.unwrap_or(0), self.x)
        } else {
            (1, self.y as usize, self.x)
        }
    }

    pub(crate) fn end_key(&self) -> (u8, usize, u16) {
        if self.scrollback {
            (0, self.scrollback_index.unwrap_or(0), self.end_x)
        } else {
            (1, self.end_y as usize, self.end_x)
        }
    }

    /// Click target: viewport start cell. Scrollback has none.
    #[must_use]
    pub fn click_point(&self) -> Option<(u16, u16)> {
        if self.scrollback {
            None
        } else {
            Some((self.x, self.y))
        }
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.scrollback {
            write!(
                f,
                "scrollback[{}] chars {}..{} {:?}",
                self.scrollback_index.unwrap_or(0),
                self.x,
                self.end_x,
                self.text
            )
        } else if self.end_y == self.y {
            write!(
                f,
                "({},{})..({},{}) {:?}",
                self.x, self.y, self.end_x, self.end_y, self.text
            )
        } else {
            write!(
                f,
                "({},{})..({},{}) wrapped {:?}",
                self.x, self.y, self.end_x, self.end_y, self.text
            )
        }
    }
}

/// Delivered action. Sinks receive exactly one of these per `click`/`submit`
/// call; readiness retries never touch the sink (Q05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Click at the target point.
    Click {
        /// Viewport column.
        x: u16,
        /// Viewport row.
        y: u16,
    },
    /// Submit (confirm) at the target point.
    Submit {
        /// Viewport column.
        x: u16,
        /// Viewport row.
        y: u16,
    },
}
