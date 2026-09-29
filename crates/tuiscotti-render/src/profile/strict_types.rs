//! Strict profile types: version pins, policies, [`RenderProfile`](super::RenderProfile).

use super::{FallbackFace, FontFaces};

// ---------------------------------------------------------------------------
// Strict render profile (backlog V01, V02, V05, V07): everything that affects
// pixels, pinned and hash-verified at construction.
// ---------------------------------------------------------------------------

/// Renderer version: bumped on ANY change to rasterization, layout, or export
/// bytes. Part of every cache key and bundle manifest, so a renderer change
/// can never read as an app regression (V08).
pub const RENDERER_VERSION: u32 = 1;

/// SHA-256 pins of the four vendored styled faces (V02: explicit font packs —
/// every style/fallback face hashed, no system scan in deterministic mode).
pub const VENDORED_FONT_SHA256: &str =
    "f2a5ea6cfab397445ffab00c0370927b66d61e560a05db5db271b42006381c1a";
pub const VENDORED_FONT_BOLD_SHA256: &str =
    "bfcf9a917276ffc058867d87cbc8a5b2f1ab0f4b710e9170dc02763ccb80bd4b";
pub const VENDORED_FONT_ITALIC_SHA256: &str =
    "31efd6ead98746f5b0afa1ee6dba60267ad48db36428360bee327bec10621f97";
pub const VENDORED_FONT_BOLD_ITALIC_SHA256: &str =
    "9dba502e00e35209f6ed2a151c7376c051657b067cdebbc6e52d06cb9002cf31";

/// Still-image sample of a blinking cell (V07). Blink *intent* (`mods.blink`)
/// is preserved in canonical state; a still PNG/SVG cannot show motion, so it
/// samples one declared phase. The phase is part of the profile hash, so
/// opposite-phase renders never share a cache entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BlinkPhase {
    /// Blinking glyphs drawn (frozen-visible, the legacy still behavior).
    On,
    /// Blinking glyphs omitted: background kept, ink and text decorations
    /// skipped — what a real terminal shows in the off half-cycle.
    Off,
}

/// Cursor policy for still renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CursorPolicy {
    /// Draw the cursor when the frame/screen marks it visible.
    Show,
    /// Never draw the cursor (deterministic cursor-free evidence).
    Hide,
}

/// Indexed-color table policy. Only the xterm table ships: resolution
/// delegates to [`tuiscotti_core::frame::Frame::resolve_cell`], so there is exactly one
/// color path. A profile claiming any other table would need a second
/// resolver — rejected by construction (unknown strings fail strict build).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IndexedPalette {
    Xterm,
}

/// Palette policy: terminal defaults plus the indexed table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PalettePolicy {
    pub default_fg: tuiscotti_core::frame::Rgb,
    pub default_bg: tuiscotti_core::frame::Rgb,
    pub indexed: IndexedPalette,
}

impl PalettePolicy {
    /// The gate palette: light-gray on black, xterm indexed table.
    #[must_use]
    pub const fn xterm() -> Self {
        Self {
            default_fg: tuiscotti_core::frame::Rgb::new(0xd0, 0xd0, 0xd0),
            default_bg: tuiscotti_core::frame::Rgb::new(0x00, 0x00, 0x00),
            indexed: IndexedPalette::Xterm,
        }
    }

    /// Resolve a cell to (fg, bg), honoring reverse/dim. Single color path:
    /// delegates to [`tuiscotti_core::frame::Frame::resolve_cell`].
    #[must_use]
    pub fn resolve(
        &self,
        cell: &tuiscotti_core::frame::Cell,
    ) -> (tuiscotti_core::frame::Rgb, tuiscotti_core::frame::Rgb) {
        tuiscotti_core::frame::Frame::resolve_cell(cell, self.default_fg, self.default_bg)
    }
}

/// Missing-glyph policy (V05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MissingGlyphPolicy {
    /// Any glyph no pinned face covers fails the render with an explicit
    /// error listing codepoints and positions. The default for visual
    /// approval: tofu must never silently stand in for real ink.
    Strict,
    /// Draw the deterministic tofu placeholder AND report every occurrence in
    /// [`crate::render::Fidelity`]. Explicit opt-in only; placeholder output
    /// is never described as faithful (`approximate` is set).
    Placeholder,
}

/// Strict render profile: pinned face bytes + hashes (all four styles plus the
/// ordered fallback chain), geometry, scale, palette/cursor policy, missing
/// policy, blink sample phase, renderer version.
///
/// Construction is strict ([`RenderProfile::strict`]): every face hash is
/// verified against its bytes, geometry/scale are range-checked, and the
/// renderer version must equal [`RENDERER_VERSION`]. There is deliberately NO
/// system-font scan, NO network fetch, and NO synthetic substitution (no faux
/// face silently standing in for a pinned one): a pin failure is an error,
/// never a quiet fallback.
#[derive(Debug, Clone)]
pub struct RenderProfile<'a> {
    pub(crate) name: String,
    pub(crate) faces: FontFaces<'a>,
    pub(crate) face_hashes: [String; 4],
    pub(crate) fallback_order: Vec<FallbackFace<'a>>,
    pub(crate) font_px: f32,
    pub(crate) cell_w: u32,
    pub(crate) cell_h: u32,
    pub(crate) pad: u32,
    pub(crate) scale: u32,
    pub(crate) palette: PalettePolicy,
    pub(crate) cursor: CursorPolicy,
    pub(crate) blink_phase: BlinkPhase,
    pub(crate) missing: MissingGlyphPolicy,
    pub(crate) renderer_version: u32,
}

/// Strict-construction failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileError(pub String);

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid render profile: {}", self.0)
    }
}

impl std::error::Error for ProfileError {}
