use crate::screen::Screen;

/// Policy for a width-2 grapheme landing in the last column of a row, where
/// no room remains for its continuation cell (M07: never cut silently).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgePolicy {
    /// Replace the glyph with [`REPLACEMENT`] (U+FFFD, width 1) and record
    /// each clipped glyph in [`ScreenCapture::clipped`] plus a note. The
    /// screen stays valid and the substitution is visible in evidence.
    #[default]
    ClipWithReplacement,
    /// Fail with a [`ScreenError`](crate::screen::ScreenError) naming the glyph and its position.
    Error,
}

/// Record of one wide glyph clipped at a row end (grid-local coordinates,
/// original symbol preserved for evidence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClippedCell {
    pub x: u16,
    pub y: u16,
    pub symbol: String,
}

/// Substitute emitted by [`EdgePolicy::ClipWithReplacement`]: U+FFFD with
/// display width 1, keeping grid geometry valid.
pub const REPLACEMENT: &str = "�";

/// A validated [`Screen`] plus the record of how it was produced.
///
/// `Screen` itself carries no provenance, so the edge policy applied, every
/// clipped glyph, and any cursor adjustments are recorded here instead of
/// being applied silently (M07/M08).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenCapture {
    /// Validated grid; construction fails rather than producing invalid data.
    pub screen: Screen,
    /// Edge policy this capture was produced under.
    pub policy: EdgePolicy,
    /// Wide glyphs replaced at row ends (empty unless clipped).
    pub clipped: Vec<ClippedCell>,
    /// Human-readable notes: clip summaries, cursor adjustments.
    pub notes: Vec<String>,
}

impl ScreenCapture {
    #[must_use]
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    #[must_use]
    pub fn into_screen(self) -> Screen {
        self.screen
    }

    #[must_use]
    pub fn has_clips(&self) -> bool {
        !self.clipped.is_empty()
    }
}
