//! Screen/frame adaptation, strict-profile renders, redaction.

use super::{RenderError, Rendered, Renderer};
use crate::profile::RenderProfile;
use tuiscotti_core::frame::Frame;
use tuiscotti_core::screen::Screen;

// ---------------------------------------------------------------------------
// Screen entry (V01) + strict one-shots (V05).
// ---------------------------------------------------------------------------

/// Adapt a validated [`Screen`] to the render input, losslessly: grid cells
/// (symbols, widths, continuations, colors, modifiers incl. hidden/blink)
/// and cursor intent are preserved verbatim. The screen origin is positional
/// metadata, not pixels, and is not carried over. Provenance is fixed and
/// deterministic (`created_unix = 0`, source `"screen"`) so identical
/// screens render identical bytes.
#[must_use]
pub fn frame_from_screen(screen: &Screen, profile_name: &str) -> Frame {
    frame_from_screen_with_source(screen, profile_name, "screen")
}

/// [`frame_from_screen`] with an explicit provenance source: the ONE
/// screen→frame adaptation; every caller funnels through here.
///
/// Source strings by caller (all other provenance fields are identical):
/// - `"screen"`: the render pipeline itself ([`frame_from_screen`], used by
///   [`Renderer::render_screen`](super::Renderer::render_screen) and redaction) —
///   the frame came straight from a live or replayed screen.
/// - `"tuiscotti-assert"`: `tuiscotti-insta` assertion evidence
///   (`tuiscotti::assert::frame_from_screen`) — the frame backs an Insta
///   snapshot gate, so the source names the asserting tool for provenance
///   audits rather than the generic screen origin.
#[must_use]
pub fn frame_from_screen_with_source(screen: &Screen, profile_name: &str, source: &str) -> Frame {
    Frame {
        version: tuiscotti_core::frame::FRAME_VERSION,
        cols: screen.cols(),
        rows: screen.rows(),
        cells: screen.cells().to_vec(),
        cursor: *screen.cursor(),
        provenance: tuiscotti_core::frame::Provenance {
            tool: "tuiscotti".to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            profile: profile_name.to_string(),
            source: source.to_string(),
            argv: Vec::new(),
            created_unix: 0,
        },
    }
}

/// Render a validated [`Screen`] under a strict [`RenderProfile`] (V01):
/// saved, direct, and live screens share this ONE engine with frame
/// rendering ([`Renderer::render`]). Strict missing policy fails on
/// uncovered glyphs; see [`MissingGlyphPolicy`](crate::profile::MissingGlyphPolicy).
///
/// One-shot convenience over the thread-local shared instance for `rp`
/// ([`Renderer::with_strict`]): faces parsed once per thread per profile,
/// glyph cache shared.
///
/// # Errors
///
/// Returns `RenderError` when the profile or screen is invalid.
pub fn render_screen(screen: &Screen, rp: &RenderProfile<'_>) -> Result<Rendered, RenderError> {
    Renderer::with_strict(rp, |r| r.render_screen(screen))
}

/// [`render_screen`] returning PNG bytes only.
///
/// # Errors
///
/// Returns `RenderError` when the profile or screen is invalid.
pub fn render_screen_png(screen: &Screen, rp: &RenderProfile<'_>) -> Result<Vec<u8>, RenderError> {
    Ok(render_screen(screen, rp)?.png)
}

/// Render a validated [`Frame`] under a strict [`RenderProfile`]: the same
/// engine as [`render_png_report`](super::render_png_report), plus the profile's missing-glyph policy
/// and blink sample phase. Same sharing as [`render_screen`].
///
/// # Errors
///
/// Returns `RenderError` when the profile or frame is invalid.
pub fn render_frame_strict(frame: &Frame, rp: &RenderProfile<'_>) -> Result<Rendered, RenderError> {
    Renderer::with_strict(rp, |r| r.render(frame))
}

// ---------------------------------------------------------------------------
// Concealment vs redaction (V06).
// ---------------------------------------------------------------------------

/// Concealment (`mods.hidden`) is NOT redaction: concealed glyphs are omitted
/// from PNG/SVG/HTML pixels, but their source symbols REMAIN in canonical
/// JSON, ANSI dumps, and plain text. Never capture real secrets expecting
/// concealment to protect them. [`redact_frame`]/[`redact_screen`] are the
/// separate, destructive API for evidence that must not carry content at all:
///
/// | API | PNG/HTML pixels | canonical JSON |
/// |---|---|---|
/// | `mods.hidden` (conceal) | omitted | PRESENT (source symbol) |
/// | [`redact_frame`] (redact) | block glyphs | block glyphs (destroyed) |
///
/// Redaction preserves grid geometry (widths/continuations), colors, and
/// cursor position — layout evidence survives, content does not. Modifiers
/// are cleared (nothing left to style), and `provenance.argv` is cleared:
/// launch arguments may carry secrets (defense-in-depth). The output
/// validates as a frame.
#[must_use]
pub fn redact_frame(frame: &Frame) -> Frame {
    let mut out = frame.clone();
    for c in &mut out.cells {
        if c.continuation {
            continue;
        }
        c.symbol = if c.width == 2 {
            "██".to_string()
        } else {
            "█".to_string()
        };
        c.mods = tuiscotti_core::frame::Mods::default();
    }
    out.provenance.argv.clear();
    out
}

/// [`redact_frame`] for [`Screen`]s: same destruction, origin preserved.
/// Fails only if the redacted grid would not validate (unreachable for
/// validated inputs — widths and continuations are untouched).
///
/// # Errors
///
/// Returns `ScreenError` when the redacted grid does not validate.
pub fn redact_screen(screen: &Screen) -> Result<Screen, tuiscotti_core::screen::ScreenError> {
    let frame = frame_from_screen(screen, "redacted");
    let redacted = redact_frame(&frame);
    let (ox, oy) = screen.origin();
    Screen::validate(
        redacted.cols,
        redacted.rows,
        ox,
        oy,
        redacted.cells,
        redacted.cursor,
    )
}
