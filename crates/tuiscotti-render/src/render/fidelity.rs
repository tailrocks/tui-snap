//! Coverage accounting: [`Fidelity`](super::Fidelity), [`Rendered`](super::Rendered), [`Artifacts`](super::Artifacts).

use serde::Serialize;

/// One cell whose glyph(s) no face in the chain covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingGlyph {
    pub x: u16,
    pub y: u16,
    pub symbol: String,
    /// Uncovered codepoints, formatted `U+26B7`.
    pub codepoints: Vec<String>,
}

/// One cell whose glyph(s) the primary family did not cover but a fallback
/// face rendered as real ink (see [`Fidelity::fallback_glyphs`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FallbackGlyph {
    pub x: u16,
    pub y: u16,
    pub symbol: String,
    /// Fallback-served codepoints, formatted `U+26B7`.
    pub codepoints: Vec<String>,
    /// Fallback face(s) that rendered them, in chain order.
    pub faces: Vec<String>,
}

/// Exact coverage accounting for one rendered frame — written next to PNG
/// outputs as `<name>.png.fidelity.json`. `approximate` is true when any
/// glyph is missing or any styled face fell back (mirroring the legacy
/// sidecar's purpose: a PNG is labelled approximate when it must be).
#[derive(Debug, Clone, Serialize)]
pub struct Fidelity {
    pub profile: String,
    pub font_sha256: String,
    pub font_desc: String,
    pub scale: u32,
    pub approximate: bool,
    /// Styled faces that failed to parse and fell back to regular.
    pub faces_fell_back: Vec<String>,
    pub missing: Vec<MissingGlyph>,
    /// Cells a fallback face rendered (omitted from the JSON when empty, so
    /// sidecars of primary-covered frames stay byte-stable).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fallback_glyphs: Vec<FallbackGlyph>,
}

impl Fidelity {
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self)
            .unwrap_or_else(|e| unreachable!("Fidelity is plain serializable data: {e}"))
    }
}

/// A rendered PNG plus its fidelity record.
pub struct Rendered {
    pub png: Vec<u8>,
    pub fidelity: Fidelity,
}

/// The four snapshot artifacts of one frame, generated in one render pass:
/// the normalized SGR dump ([`ansi_dump`](super::ansi_dump)), plain text ([`Frame::text`](tuiscotti_core::frame::Frame::text)), the
/// standalone HTML view ([`Renderer::render_html`](super::Renderer::render_html)) and the authoritative
/// PNG. Bytes are deterministic for identical frames (the HTML embed
/// normalizes the provenance timestamp — see [`Renderer::render_html`](super::Renderer::render_html)).
pub struct Artifacts {
    /// Colored terminal text (normalized SGR dump).
    pub ansi: String,
    /// Plain black-and-white text.
    pub txt: String,
    /// Standalone colored HTML render.
    pub html: String,
    /// Colored image (authoritative pixel-gate evidence).
    pub png: Vec<u8>,
    /// Coverage accounting of the PNG render.
    pub fidelity: Fidelity,
}
