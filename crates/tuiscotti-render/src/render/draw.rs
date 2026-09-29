//! Symbol drawing through the styled/regular/fallback face chain.

use super::{
    CellSinks, FaceIdx, FallbackGlyph, FontSet, GlyphCache, LoadedFont, MissingGlyph, blend,
    cached_raster, draw_tofu, face_idx, is_default_ignorable,
};
use tuiscotti_core::frame::Rgb;

/// Draw one lead-cell symbol at pen origin. Combining scalars overlay at the
/// same origin (documented approximation of terminal combining behavior).
/// Face chain: styled face → regular face → fallback faces in chain order →
/// tofu (recorded in `sinks.missing`). A face covers a codepoint only when
/// it produces a non-empty bitmap — cmap-only hits with empty outlines fall
/// through (otherwise Nerd-Font placeholders / un-rasterizable CFF would
/// block Noto). Faux styles apply only when the regular face serves a cell
/// whose mods asked for a styled face. Fallback glyphs draw in their face's
/// own weight, centered horizontally in the cell span and clipped to the
/// cell rect (fallback faces have their own metrics; the primary cell grid
/// never moves). Rasters come from `cache` (per `(char, face)`, negatives
/// included) instead of re-rasterizing per cell.
pub(crate) fn draw_symbol(
    dst: &mut image::RgbImage,
    set: &FontSet,
    cache: &mut GlyphCache,
    symbol: &str,
    pen_x: i32,
    baseline: i32,
    span_px: u32,
    cell_top: i32,
    cell_h: u32,
    fg: Rgb,
    bold: bool,
    italic: bool,
    u: i32,
    mut sinks: Option<CellSinks<'_>>,
) {
    let styled = set.styled(bold, italic);
    let styled_idx = face_idx(bold, italic);
    let mut uncovered: Vec<char> = Vec::new();
    let mut served: Vec<(char, u8)> = Vec::new();
    for c in symbol.chars() {
        if is_default_ignorable(c) {
            continue;
        }
        match pick_face(set, cache, styled, styled_idx, c) {
            Pick::Styled => {
                draw_primary(
                    cache, dst, styled, styled_idx, c, pen_x, baseline, fg, false, false, u,
                );
            }
            Pick::Regular => {
                draw_primary(
                    cache,
                    dst,
                    &set.regular,
                    FaceIdx::Regular,
                    c,
                    pen_x,
                    baseline,
                    fg,
                    bold,
                    italic,
                    u,
                );
            }
            Pick::Missing => {
                uncovered.push(c);
            }
            Pick::Fallback(fi) => {
                if draw_fallback_glyph(
                    cache, dst, set, c, fi, pen_x, baseline, span_px, cell_top, cell_h, fg,
                ) {
                    served.push((c, fi as u8));
                } else {
                    uncovered.push(c);
                }
            }
        }
    }
    if !served.is_empty() {
        record_fallback_served(set, sinks.as_mut(), symbol, &served);
    }
    if uncovered.is_empty() {
        return;
    }
    draw_uncovered_tofu(
        dst, sinks, symbol, pen_x, cell_top, span_px, cell_h, fg, u, &uncovered,
    );
}

/// Face-chain pick for one scalar: styled → regular → fallbacks in order →
/// missing. Coverage = non-empty raster, not lookup_glyph_index != 0.
enum Pick {
    Styled,
    Regular,
    Fallback(usize),
    Missing,
}

fn pick_face(
    set: &FontSet,
    cache: &mut GlyphCache,
    styled: &LoadedFont,
    styled_idx: FaceIdx,
    c: char,
) -> Pick {
    if cached_raster(cache, styled, styled_idx, c).is_some() {
        Pick::Styled
    } else if cached_raster(cache, &set.regular, FaceIdx::Regular, c).is_some() {
        Pick::Regular
    } else if let Some(fi) = (0..set.fallbacks.len()).find(|&fi| {
        cached_raster(cache, &set.fallbacks[fi], FaceIdx::Fallback(fi as u8), c).is_some()
    }) {
        Pick::Fallback(fi)
    } else {
        Pick::Missing
    }
}

/// Blit one fallback-face glyph centered in the cell span, clipped to the
/// cell rect. Returns whether any ink landed (`false` reads as uncovered,
/// never as a silent blank).
fn draw_fallback_glyph(
    cache: &mut GlyphCache,
    dst: &mut image::RgbImage,
    set: &FontSet,
    c: char,
    fi: usize,
    pen_x: i32,
    baseline: i32,
    span_px: u32,
    cell_top: i32,
    cell_h: u32,
    fg: Rgb,
) -> bool {
    let idx = FaceIdx::Fallback(fi as u8);
    let Some((m, bmp)) = cached_raster(cache, &set.fallbacks[fi], idx, c).cloned() else {
        return false;
    };
    // Center the glyph's advance box in the cell span; clip ink
    // to the cell rect so fallback metrics never bleed into
    // neighboring cells.
    let origin_x = pen_x + ((span_px as f32 - m.advance_width) / 2.0).round() as i32;
    let top = baseline - (m.ymin + m.height as i32);
    let mut inked = false;
    for (i, &cov) in bmp.iter().enumerate() {
        if cov == 0 {
            continue;
        }
        let bx = (i % m.width) as i32;
        let by = (i / m.width) as i32;
        let dx = origin_x + m.xmin + bx;
        let dy = top + by;
        if dx < pen_x
            || dx >= pen_x + span_px as i32
            || dy < cell_top
            || dy >= cell_top + cell_h as i32
        {
            continue;
        }
        inked = true;
        blend(dst, dx as u32, dy as u32, fg, cov);
    }
    inked
}

fn record_fallback_served(
    set: &FontSet,
    sinks: Option<&mut CellSinks<'_>>,
    symbol: &str,
    served: &[(char, u8)],
) {
    if let Some(s) = sinks {
        let mut faces: Vec<String> = Vec::new();
        for (_, fi) in served {
            let desc = set.fallbacks[usize::from(*fi)].desc.clone();
            if !faces.contains(&desc) {
                faces.push(desc);
            }
        }
        s.fallback.push(FallbackGlyph {
            x: s.x,
            y: s.y,
            symbol: symbol.to_string(),
            codepoints: served
                .iter()
                .map(|(c, _)| format!("U+{:04X}", *c as u32))
                .collect(),
            faces,
        });
    }
}

/// Tofu placeholder for uncovered scalars plus the missing-glyph record.
fn draw_uncovered_tofu(
    dst: &mut image::RgbImage,
    sinks: Option<CellSinks<'_>>,
    symbol: &str,
    pen_x: i32,
    cell_top: i32,
    span_px: u32,
    cell_h: u32,
    fg: Rgb,
    u: i32,
    uncovered: &[char],
) {
    draw_tofu(
        dst,
        pen_x,
        cell_top + 2 * u,
        span_px,
        cell_h.saturating_sub(4 * u as u32),
        fg,
        u,
    );
    if let Some(s) = sinks {
        s.missing.push(MissingGlyph {
            x: s.x,
            y: s.y,
            symbol: symbol.to_string(),
            codepoints: uncovered
                .iter()
                .map(|c| format!("U+{:04X}", *c as u32))
                .collect(),
        });
    }
}

/// Draw one glyph from the primary family (styled or regular face, with the
/// faux double-strike / shear when the regular face serves a styled cell).
/// This path is byte-stable: fallback-chain changes never touch it.
pub(crate) fn draw_primary(
    cache: &mut GlyphCache,
    dst: &mut image::RgbImage,
    face: &LoadedFont,
    idx: FaceIdx,
    c: char,
    pen_x: i32,
    baseline: i32,
    fg: Rgb,
    faux_bold: bool,
    faux_italic: bool,
    u: i32,
) {
    let Some((m, bmp)) = cached_raster(cache, face, idx, c) else {
        return;
    };
    // ymin = offset of the bitmap's BOTTOM edge from the baseline, so the
    // top edge sits at baseline - (ymin + height).
    let top = baseline - (m.ymin + m.height as i32);
    for (i, &cov) in bmp.iter().enumerate() {
        if cov == 0 {
            continue;
        }
        let bx = (i % m.width) as i32;
        let by = (i / m.width) as i32;
        // Faux italic: shear top rows right (fallback only).
        let shear = if faux_italic {
            ((m.height as i32 - 1 - by) as f32 * 0.15) as i32
        } else {
            0
        };
        let dx = pen_x + m.xmin + bx + shear;
        let dy = top + by;
        if dx >= 0 && dy >= 0 {
            blend(dst, dx as u32, dy as u32, fg, cov);
            // Faux bold: double-strike one unscaled pixel right.
            if faux_bold {
                blend(dst, (dx + u) as u32, dy as u32, fg, cov);
            }
        }
    }
}
