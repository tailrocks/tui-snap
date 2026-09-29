//! Font loading: [`LoadedFont`](super::LoadedFont), [`FontSet`](super::FontSet), geometry pins.
//!
//! Raster backend is `swash` (Fontations/skrifa outlines, unhinted): the
//! previous backend (`fontdue`) pulled the unmaintained `ttf-parser`
//! (RUSTSEC-2026-0192) plus a Zlib-only `foldhash`, both rejected by
//! `cargo deny`. Unhinted swash rasters match the old backend's geometry
//! exactly (same `xmin`/`ymin`/dims/advance per glyph; only antialiasing
//! gradations differ), so the profile cell pins are unchanged.

use super::RenderError;
use crate::profile::FontFaces;
use crate::profile::Profile;

/// Glyph bitmap metrics in the shape the blitters consume: `xmin` is the
/// bitmap's left edge relative to the pen, `ymin` the bitmap's BOTTOM edge
/// relative to the baseline in y-up coordinates (so the top edge sits at
/// `baseline - (ymin + height)`), `width`/`height` the bitmap dims, advances
/// in pixels at raster size.
#[derive(Debug, Clone, Copy)]
pub struct GlyphMetrics {
    /// Bitmap width in pixels.
    pub width: usize,
    /// Bitmap height in pixels.
    pub height: usize,
    /// Bitmap left edge relative to the pen.
    pub xmin: i32,
    /// Bitmap bottom edge relative to the baseline (y-up).
    pub ymin: i32,
    /// Horizontal advance in pixels at raster size.
    pub advance_width: f32,
    /// Vertical advance in pixels at raster size.
    pub advance_height: f32,
}

/// A loaded raster font with line metrics.
pub struct LoadedFont {
    /// Owned font bytes (`swash::FontRef` borrows; validated at load).
    data: Vec<u8>,
    /// Pixels above baseline.
    pub ascent: f32,
    /// Pixels below baseline (nonnegative).
    pub descent: f32,
    pub px: f32,
    /// Human-readable face identity (fallback faces: the pinned description).
    pub desc: String,
}

/// Outline-only render sources: embedded bitmaps and color outlines are
/// never served (a color/bitmap-only glyph reads as uncovered, never as a
/// silent blank or a miscolored mask).
const SOURCES: &[swash::scale::Source] = &[swash::scale::Source::Outline];

impl LoadedFont {
    fn font_ref(&self) -> Option<swash::FontRef<'_>> {
        swash::FontRef::from_index(&self.data, 0)
    }

    /// Scaled horizontal advance of `c` in pixels. Coverage-independent (a
    /// cmap miss still yields the `.notdef` advance), for geometry pins.
    pub(crate) fn advance_width(&self, c: char) -> f32 {
        let Some(font) = self.font_ref() else {
            return 0.0;
        };
        let upm = font.metrics(&[]).units_per_em;
        if upm == 0 {
            return 0.0;
        }
        let id = font.charmap().map(c);
        font.glyph_metrics(&[]).advance_width(id) * self.px / f32::from(upm)
    }

    /// Rasterize `c` at face size: `None` when the face lacks the glyph (cmap
    /// miss — including the `.notdef` trap) or has no scalable outline for
    /// it; otherwise the 8-bit alpha mask with blitter-ready metrics.
    /// Callers still apply the ink check: an empty outline does not cover.
    pub(crate) fn rasterize(&self, c: char) -> Option<(GlyphMetrics, Vec<u8>)> {
        let font = self.font_ref()?;
        let id = font.charmap().map(c);
        if id == 0 {
            return None;
        }
        let mut ctx = swash::scale::ScaleContext::new();
        let mut scaler = ctx.builder(font).size(self.px).hint(false).build();
        let image = swash::scale::Render::new(SOURCES).render(&mut scaler, id)?;
        if !matches!(image.content, swash::scale::image::Content::Mask) {
            return None;
        }
        let p = &image.placement;
        let (w, h) = (p.width as usize, p.height as usize);
        if w == 0 || h == 0 || image.data.len() != w * h {
            return None;
        }
        let upm = font.metrics(&[]).units_per_em;
        let scale = self.px / f32::from(upm.max(1));
        let gm = font.glyph_metrics(&[]);
        Some((
            GlyphMetrics {
                width: w,
                height: h,
                xmin: p.left,
                ymin: p.top - p.height as i32,
                advance_width: gm.advance_width(id) * scale,
                advance_height: gm.advance_height(id) * scale,
            },
            image.data,
        ))
    }
}

/// Load + measure a font.
pub fn load_font(bytes: &[u8], px: f32) -> Result<LoadedFont, RenderError> {
    let font = swash::FontRef::from_index(bytes, 0)
        .ok_or_else(|| RenderError("cannot parse font: swash rejected the bytes".to_string()))?;
    let m = font.metrics(&[]);
    if m.units_per_em == 0 {
        return Err(RenderError("font has no horizontal metrics".to_string()));
    }
    let scale = px / f32::from(m.units_per_em);
    Ok(LoadedFont {
        data: bytes.to_vec(),
        ascent: m.ascent * scale,
        descent: (m.descent * scale).abs(),
        px,
        desc: String::new(),
    })
}

/// Measured advance of `M` and line height at profile size.
pub fn measure(loaded: &LoadedFont) -> (f32, f32) {
    (loaded.advance_width('M'), loaded.ascent + loaded.descent)
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
