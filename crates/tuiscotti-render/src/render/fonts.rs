//! Font loading: [`LoadedFont`](super::LoadedFont), [`FontSet`](super::FontSet), geometry pins.

use super::RenderError;
use crate::profile::FontFaces;
use crate::profile::Profile;
use fontdue::{Font, FontSettings};

/// A loaded raster font with line metrics.
pub struct LoadedFont {
    pub(crate) font: Font,
    /// Pixels above baseline.
    pub ascent: f32,
    /// Pixels below baseline (nonnegative).
    pub descent: f32,
    pub px: f32,
    /// Human-readable face identity (fallback faces: the pinned description).
    pub desc: String,
}

/// Load + measure a font.
pub fn load_font(bytes: &[u8], px: f32) -> Result<LoadedFont, RenderError> {
    let font = Font::from_bytes(bytes, FontSettings::default())
        .map_err(|e| RenderError(format!("cannot parse font: {e}")))?;
    let lm = font
        .horizontal_line_metrics(px)
        .ok_or_else(|| RenderError("font has no horizontal metrics".to_string()))?;
    Ok(LoadedFont {
        font,
        ascent: lm.ascent,
        descent: lm.descent.abs(),
        px,
        desc: String::new(),
    })
}

/// Measured advance of `M` and line height at profile size.
pub fn measure(loaded: &LoadedFont) -> (f32, f32) {
    let adv = loaded.font.rasterize('M', loaded.px).0.advance_width;
    (adv, loaded.ascent + loaded.descent)
}

/// Fail unless the font measures exactly like the profile pins.
/// Call before every gate render.
pub fn verify_geometry(loaded: &LoadedFont, profile: &Profile) -> Result<(), RenderError> {
    let (adv, line_h) = measure(loaded);
    if adv.round() as u32 != profile.cell_w || line_h.round() as u32 != profile.cell_h {
        return Err(RenderError(format!(
            "font/geometry pin broken: measured advance {adv:.2} line {line_h:.2}, profile pins {}x{} — refusing to render",
            profile.cell_w, profile.cell_h
        )));
    }
    Ok(())
}

/// The four faces of one family, loaded at one pixel size, plus the per-glyph
/// fallback chain. Faces share the regular face's baseline (cell grid
/// authority). A non-regular face that fails to parse falls back to the
/// regular face and is named in `fell_back` (the faux styles then return for
/// it). Fallback faces are coverage-only: they serve single glyphs the
/// primary family lacks, centered and clipped inside the primary cell box;
/// they never move the cell grid.
pub struct FontSet {
    pub regular: LoadedFont,
    pub bold: LoadedFont,
    pub italic: LoadedFont,
    pub bold_italic: LoadedFont,
    /// Non-regular faces that failed to parse and fell back to regular.
    pub fell_back: Vec<&'static str>,
    /// Per-glyph fallback chain, tried in order after the primary family.
    pub fallbacks: Vec<LoadedFont>,
}

impl FontSet {
    pub fn load(faces: &FontFaces<'_>, px: f32) -> Result<Self, RenderError> {
        Self::load_with_fallbacks(faces, px, &[])
    }

    /// Load the styled family plus a pinned fallback chain. Each fallback
    /// face's bytes are verified against its pinned SHA-256 before parsing;
    /// a hash mismatch or an unparsable face fails the load (explicit, never
    /// silent — a swapped/corrupt font must read as a renderer change).
    pub fn load_with_fallbacks(
        faces: &FontFaces<'_>,
        px: f32,
        fallbacks: &[crate::profile::FallbackFace<'_>],
    ) -> Result<Self, RenderError> {
        let regular = load_font(faces.regular, px)?;
        let mut fell_back = Vec::new();
        let mut face = |bytes: &[u8], name: &'static str| -> Result<LoadedFont, RenderError> {
            match load_font(bytes, px) {
                Ok(f) => Ok(f),
                Err(_) => {
                    fell_back.push(name);
                    load_font(faces.regular, px)
                }
            }
        };
        let mut loaded_fallbacks = Vec::with_capacity(fallbacks.len());
        if fallbacks.len() > u8::MAX as usize {
            return Err(RenderError(format!(
                "fallback chain too long: {} faces (max 255)",
                fallbacks.len()
            )));
        }
        for f in fallbacks {
            let actual = crate::profile::font_sha256(f.bytes);
            if actual != f.sha256 {
                return Err(RenderError(format!(
                    "fallback face '{}' sha256 mismatch: pinned {}, got {actual} — refusing to render",
                    f.desc, f.sha256
                )));
            }
            let mut lf = load_font(f.bytes, px)
                .map_err(|e| RenderError(format!("fallback face '{}': {e}", f.desc)))?;
            lf.desc = f.desc.to_string();
            loaded_fallbacks.push(lf);
        }
        Ok(Self {
            bold: face(faces.bold, "bold")?,
            italic: face(faces.italic, "italic")?,
            bold_italic: face(faces.bold_italic, "bold_italic")?,
            regular,
            fell_back,
            fallbacks: loaded_fallbacks,
        })
    }

    /// The face `cell.mods` selects, before per-glyph coverage fallback.
    pub(crate) fn styled(&self, bold: bool, italic: bool) -> &LoadedFont {
        match (bold, italic) {
            (true, true) => &self.bold_italic,
            (true, false) => &self.bold,
            (false, true) => &self.italic,
            (false, false) => &self.regular,
        }
    }
}
