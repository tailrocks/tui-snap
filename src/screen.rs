//! M1 validated screen/observation model (backlog M01, M02, M04, M07-regions, M08).
//!
//! - [`Screen`]: immutable validated grid with dimensions AND origin.
//! - [`Observation`]: one atomic capture (owned screen, revision, reason,
//!   terminal state, informational provenance). Equality/hash cover only the
//!   approval-relevant subset: provenance (timestamps/PIDs/paths) is excluded.
//! - [`Region`]: a cropped screen with geometry/origin preserved and the
//!   [`RegionPolicy`] recorded. Crops that would split a wide grapheme fail.
//!
//! Cell content reuses `crate::frame` types (`Cell`, `Color`, `Mods`, `Cursor`,
//! `Rgb`); they are not duplicated here. This module adds `Hash` impls for
//! those types so observations hash deterministically.

use crate::frame::{Cell, Cursor, Frame, Rgb};
use std::hash::{Hash, Hasher};

// ---------------------------------------------------------------------------
// Hash impls for frame types (same crate, so these overlap-free impls live here
// to keep `frame.rs` untouched).
// ---------------------------------------------------------------------------

impl Hash for Rgb {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.r.hash(state);
        self.g.hash(state);
        self.b.hash(state);
    }
}

impl Hash for crate::frame::Color {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            crate::frame::Color::Default => {}
            crate::frame::Color::Indexed(n) => n.hash(state),
            crate::frame::Color::Rgb(rgb) => rgb.hash(state),
        }
    }
}

impl Hash for crate::frame::Mods {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hidden.hash(state);
        self.blink.hash(state);
        self.bold.hash(state);
        self.dim.hash(state);
        self.italic.hash(state);
        self.underline.hash(state);
        self.underline_style.hash(state);
        self.strikethrough.hash(state);
        self.reverse.hash(state);
    }
}

impl Hash for Cell {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x.hash(state);
        self.y.hash(state);
        self.symbol.hash(state);
        self.width.hash(state);
        self.continuation.hash(state);
        self.fg.hash(state);
        self.bg.hash(state);
        self.mods.hash(state);
        self.underline_color.hash(state);
    }
}

impl Hash for crate::frame::CursorStyle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
    }
}

impl Hash for Cursor {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x.hash(state);
        self.y.hash(state);
        self.visible.hash(state);
        self.style.hash(state);
        self.blinking.hash(state);
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Screen/region/observation construction failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenError(pub String);

impl std::fmt::Display for ScreenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid screen: {}", self.0)
    }
}

impl std::error::Error for ScreenError {}

/// Maximum screen dimension (mirrors `frame::MAX_DIM`; static views carry no
/// PTY minimums, so 1x1 and 1-column/1-row screens are valid).
pub const MAX_DIM: u16 = crate::frame::MAX_DIM;

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
    cols: u16,
    rows: u16,
    ox: i32,
    oy: i32,
    cells: Vec<Cell>,
    cursor: Cursor,
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
        let bad = |m: String| ScreenError(m);
        if cols == 0 || rows == 0 {
            return Err(bad("dimensions must be nonzero".to_string()));
        }
        if cols > MAX_DIM || rows > MAX_DIM {
            return Err(bad(format!(
                "dimensions {cols}x{rows} exceed max {MAX_DIM}"
            )));
        }
        if cells.len() != cols as usize * rows as usize {
            return Err(bad(format!("cell count {} != {cols}x{rows}", cells.len())));
        }
        for (i, c) in cells.iter().enumerate() {
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
            match (c.width, c.continuation) {
                (0, true) => {
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
                }
                (1 | 2, false) => {
                    if c.symbol.is_empty() {
                        return Err(bad(format!(
                            "lead cell at ({},{}) must have a symbol",
                            c.x, c.y
                        )));
                    }
                    if c.width == 2 {
                        if c.x + 1 >= cols {
                            return Err(bad(format!(
                                "wide cell at ({},{}) overflows row",
                                c.x, c.y
                            )));
                        }
                        let next = &cells[i + 1];
                        if next.width != 0 || !next.continuation {
                            return Err(bad(format!(
                                "wide cell at ({},{}) missing continuation",
                                c.x, c.y
                            )));
                        }
                    }
                }
                _ => {
                    return Err(bad(format!(
                        "bad width/continuation at ({},{}): width={} continuation={}",
                        c.x, c.y, c.width, c.continuation
                    )));
                }
            }
        }
        if cursor.visible && (cursor.x >= cols || cursor.y >= rows) {
            return Err(bad("visible cursor outside grid".to_string()));
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

    /// Validated import from a canonical [`Frame`] (M02). The frame is
    /// validated first; all source distinctions (colors, modifiers incl.
    /// hidden/blink, styled blanks, continuations, cursor) are preserved.
    /// Imported screens sit at origin (0,0).
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

    /// Blank screen at origin (0,0) with a hidden default cursor. Panics on
    /// zero/oversize dimensions like [`Frame::blank`].
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

    #[must_use]
    pub fn cols(&self) -> u16 {
        self.cols
    }

    #[must_use]
    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// Origin of cell (0,0) in the larger coordinate plane.
    #[must_use]
    pub fn origin(&self) -> (i32, i32) {
        (self.ox, self.oy)
    }

    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    #[must_use]
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

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
        if x as u32 + cols as u32 > self.cols as u32 || y as u32 + rows as u32 > self.rows as u32 {
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

// ---------------------------------------------------------------------------
// Maybe: Unknown/Unsupported are never false/empty/default.
// ---------------------------------------------------------------------------

/// Tri-state terminal observation: a required-but-unavailable property is
/// [`Maybe::Unknown`] (not yet observed) or [`Maybe::Unsupported`] (backend
/// cannot provide it) — never `false`, empty, or default-conflated.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Maybe<T> {
    Known(T),
    Unknown,
    Unsupported,
}

impl<T> Maybe<T> {
    #[must_use]
    pub fn known(&self) -> Option<&T> {
        match self {
            Maybe::Known(v) => Some(v),
            Maybe::Unknown | Maybe::Unsupported => None,
        }
    }

    #[must_use]
    pub fn is_known(&self) -> bool {
        matches!(self, Maybe::Known(_))
    }
}

// ---------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------

/// Why a capture was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureReason {
    Initial,
    Poll,
    Input,
    Resize,
    Exit,
    Manual,
}

/// Observed terminal state beyond the grid. Every field is [`Maybe`]-wrapped:
/// unknown and unsupported backends stay visible instead of collapsing to
/// empty/default values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TermState {
    /// Set DEC/private mode numbers.
    pub modes: Maybe<Vec<u16>>,
    /// Palette overrides as (index, color) pairs.
    pub palette: Maybe<Vec<(u8, Rgb)>>,
    /// Window/icon title.
    pub title: Maybe<String>,
    /// Bell count since session start.
    pub bells: Maybe<u64>,
}

impl Default for TermState {
    fn default() -> Self {
        Self {
            modes: Maybe::Unknown,
            palette: Maybe::Unknown,
            title: Maybe::Unknown,
            bells: Maybe::Unknown,
        }
    }
}

/// Informational capture provenance: timestamps, PIDs, paths, attempt
/// counters. Recorded for auditability; EXCLUDED from [`Observation`]
/// equality and hashing (M08) so runtime metadata never becomes an
/// approval key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureProvenance {
    pub captured_unix_ms: u64,
    pub pid: Option<u32>,
    pub source_path: Option<String>,
    pub attempt: u64,
}

impl CaptureProvenance {
    #[must_use]
    pub fn new(
        captured_unix_ms: u64,
        pid: Option<u32>,
        source_path: Option<String>,
        attempt: u64,
    ) -> Self {
        Self {
            captured_unix_ms,
            pid,
            source_path,
            attempt,
        }
    }
}

/// One atomic capture: an owned [`Screen`] plus revision, capture reason,
/// terminal state, and informational provenance.
///
/// `PartialEq`/`Hash` are manual over the approval-relevant subset
/// (`screen`, `revision`, `reason`, `state`); [`CaptureProvenance`] is
/// excluded so identical captures compare equal despite different
/// timestamps, PIDs, paths, or attempt counters (M08).
#[derive(Debug, Clone)]
pub struct Observation {
    pub screen: Screen,
    pub revision: u64,
    pub reason: CaptureReason,
    pub state: TermState,
    pub provenance: CaptureProvenance,
}

impl Observation {
    #[must_use]
    pub fn new(
        screen: Screen,
        revision: u64,
        reason: CaptureReason,
        state: TermState,
        provenance: CaptureProvenance,
    ) -> Self {
        Self {
            screen,
            revision,
            reason,
            state,
            provenance,
        }
    }
}

impl PartialEq for Observation {
    fn eq(&self, other: &Self) -> bool {
        self.screen == other.screen
            && self.revision == other.revision
            && self.reason == other.reason
            && self.state == other.state
        // provenance deliberately excluded (M08)
    }
}

impl Eq for Observation {}

impl Hash for Observation {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.screen.hash(state);
        self.revision.hash(state);
        self.reason.hash(state);
        self.state.hash(state);
        // provenance deliberately excluded (M08)
    }
}

// ---------------------------------------------------------------------------
// Region
// ---------------------------------------------------------------------------

/// How a [`Region`] treats cells outside its bounds. Recorded in evidence;
/// geometry is always preserved (M07).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionPolicy {
    /// Cells outside the region are clipped away.
    Clip,
    /// Cells outside the region are masked (M1 records the policy; content
    /// masking itself is a later milestone).
    Mask,
}

/// A cropped screen with geometry/origin preserved and its [`RegionPolicy`]
/// recorded. See [`Screen::region`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Region {
    screen: Screen,
    policy: RegionPolicy,
}

impl Region {
    #[must_use]
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    #[must_use]
    pub fn policy(&self) -> RegionPolicy {
        self.policy
    }
}
