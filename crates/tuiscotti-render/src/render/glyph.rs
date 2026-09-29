//! Glyph raster cache and low-level cell painting helpers.

use super::{FallbackGlyph, LoadedFont, MissingGlyph};
use tuiscotti_core::frame::Rgb;

/// Glyph rasters keyed by character and face, negative results included
/// (`None` = rasterized once to an empty bitmap — never re-rasterized).
pub(crate) type GlyphCache =
    std::collections::HashMap<GlyphKey, Option<(fontdue::Metrics, Vec<u8>)>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
    ch: char,
    face: FaceIdx,
}

/// The face a glyph was actually rasterized from. Rasterize size is fixed
/// per [`Renderer`](super::Renderer) (`font_px * scale`), so it is not part of the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FaceIdx {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    /// Index into `FontSet::fallbacks` (chains are short: < 256 faces).
    Fallback(u8),
}

pub(crate) fn face_idx(bold: bool, italic: bool) -> FaceIdx {
    match (bold, italic) {
        (true, true) => FaceIdx::BoldItalic,
        (true, false) => FaceIdx::Bold,
        (false, true) => FaceIdx::Italic,
        (false, false) => FaceIdx::Regular,
    }
}

pub(crate) fn blend(dst: &mut image::RgbImage, x: u32, y: u32, fg: Rgb, cov: u8) {
    if cov == 0 {
        return;
    }
    let (w, h) = (dst.width(), dst.height());
    if x >= w || y >= h {
        return;
    }
    let p = dst.get_pixel_mut(x, y);
    let a = u32::from(cov);
    p[0] = ((u32::from(fg.r) * a + u32::from(p[0]) * (255 - a)) / 255) as u8;
    p[1] = ((u32::from(fg.g) * a + u32::from(p[1]) * (255 - a)) / 255) as u8;
    p[2] = ((u32::from(fg.b) * a + u32::from(p[2]) * (255 - a)) / 255) as u8;
}

pub(crate) fn fill_rect(dst: &mut image::RgbImage, x: u32, y: u32, w: u32, h: u32, c: Rgb) {
    let (dw, dh) = (dst.width(), dst.height());
    for dy in 0..h {
        for dx in 0..w {
            let (px, py) = (x + dx, y + dy);
            if px < dw && py < dh {
                dst.put_pixel(px, py, image::Rgb([c.r, c.g, c.b]));
            }
        }
    }
}

/// Deterministic tofu box for missing glyphs (spans `span_px` wide). `u` is
/// the scale unit: insets are one unscaled pixel.
pub(crate) fn draw_tofu(
    dst: &mut image::RgbImage,
    x0: i32,
    top: i32,
    span_px: u32,
    h: u32,
    fg: Rgb,
    u: i32,
) {
    let w = (span_px as i32 - 2 * u).max(3 * u);
    for dx in 0..w {
        blend(dst, (x0 + u + dx).max(0) as u32, top.max(0) as u32, fg, 255);
        blend(
            dst,
            (x0 + u + dx).max(0) as u32,
            (top + h as i32 - u).max(0) as u32,
            fg,
            255,
        );
    }
    for dy in 0..h as i32 {
        blend(
            dst,
            (x0 + u).max(0) as u32,
            (top + dy).max(0) as u32,
            fg,
            255,
        );
        blend(
            dst,
            (x0 + u + w - u).max(0) as u32,
            (top + dy).max(0) as u32,
            fg,
            255,
        );
    }
}

/// Per-cell fidelity sinks threaded through [`draw_symbol`](super::draw_symbol) (the cursor
/// redraw passes `None`: it re-renders a cell already accounted for).
pub(crate) struct CellSinks<'a> {
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) missing: &'a mut Vec<MissingGlyph>,
    pub(crate) fallback: &'a mut Vec<FallbackGlyph>,
}

/// Default_Ignorable codepoints (variation selectors, ZWJ, …) carry no ink
/// of their own. They must not count as uncovered: a cell like `☕`+U+FE0F
/// would otherwise draw tofu on top of a real glyph (cmap-only coverage
/// treated the selector as a miss). Combining marks are NOT ignorable and
/// still overlay at the same origin.
pub(crate) fn is_default_ignorable(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{17B4}'
            | '\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFF8}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

/// Rasterize `c` from `face` once (negatives cached). Coverage is **ink**,
/// not cmap index: an empty outline (Nerd-Font placeholder, failed CFF,
/// zero bitmap) does not cover, so the chain can try the next face.
pub(crate) fn cached_raster<'a>(
    cache: &'a mut GlyphCache,
    face: &LoadedFont,
    idx: FaceIdx,
    c: char,
) -> Option<&'a (fontdue::Metrics, Vec<u8>)> {
    cache
        .entry(GlyphKey { ch: c, face: idx })
        .or_insert_with(|| {
            if face.font.lookup_glyph_index(c) == 0 {
                return None;
            }
            let (m, bmp) = face.font.rasterize(c, face.px);
            if m.width == 0 || m.height == 0 || bmp.iter().all(|&p| p == 0) {
                None
            } else {
                Some((m, bmp))
            }
        })
        .as_ref()
}
