//! Glyph raster cache and low-level cell painting helpers.

use super::{FallbackGlyph, GlyphMetrics, LoadedFont, MissingGlyph};
use std::collections::{BTreeMap, HashMap};
use tuiscotti_core::frame::Rgb;

/// Cap on cached `(char, face)` rasters per renderer. A screen holds at
/// most `cols × rows` cells but only hundreds of DISTINCT glyphs in
/// practice; 4096 covers CJK/symbol-heavy screens (including cached
/// negative results) with headroom. Each raster is at most
/// `(font_px × scale)²` bytes (tens of kilobytes), so the worst case stays
/// near tens of megabytes — and one renderer instance owns one cache, so
/// bulk gates cannot pile memory across renders.
pub(crate) const MAX_GLYPH_CACHE_ENTRIES: usize = 4096;

/// Glyph rasters keyed by character and face, negative results included
/// (`None` = rasterized once to an empty bitmap — never re-rasterized).
/// Least-recently-used eviction past [`MAX_GLYPH_CACHE_ENTRIES`]: every
/// entry carries a sequence number mirrored in an ordered index, so both
/// hits and evictions cost O(log n) with no stale-index cleanup.
#[derive(Debug)]
pub(crate) struct GlyphCache {
    map: HashMap<GlyphKey, GlyphSlot>,
    lru: BTreeMap<u64, GlyphKey>,
    next_seq: u64,
}

#[derive(Debug)]
struct GlyphSlot {
    value: Option<(GlyphMetrics, Vec<u8>)>,
    seq: u64,
}

impl GlyphCache {
    pub(crate) fn new() -> Self {
        Self {
            map: HashMap::new(),
            lru: BTreeMap::new(),
            next_seq: 0,
        }
    }

    /// Distinct `(char, face)` rasters currently cached (diagnostics).
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }

    /// Fetch a cached raster, refreshing its recency. A miss returns
    /// `None` and inserts nothing (unlike [`Self::get_or_insert_with`]).
    pub(crate) fn get(&mut self, key: GlyphKey) -> Option<&(GlyphMetrics, Vec<u8>)> {
        let seq = self.claim_seq();
        let slot = self.map.get_mut(&key)?;
        self.lru.remove(&slot.seq);
        slot.seq = seq;
        self.lru.insert(seq, key);
        slot.value.as_ref()
    }

    /// Fetch a cached raster, rasterizing once on miss. Both hits and
    /// fresh inserts refresh recency; past [`MAX_GLYPH_CACHE_ENTRIES`] the
    /// least-recently-used entry (hits included, negatives included) is
    /// evicted. The fill closure runs at most once per key per residency.
    pub(crate) fn get_or_insert_with(
        &mut self,
        key: GlyphKey,
        fill: impl FnOnce() -> Option<(GlyphMetrics, Vec<u8>)>,
    ) -> Option<&(GlyphMetrics, Vec<u8>)> {
        if self.map.contains_key(&key) {
            return self.get(key);
        }
        let seq = self.claim_seq();
        let value = fill();
        self.map.insert(key, GlyphSlot { value, seq });
        self.lru.insert(seq, key);
        while self.map.len() > MAX_GLYPH_CACHE_ENTRIES {
            let Some((_, victim)) = self.lru.pop_first() else {
                break;
            };
            self.map.remove(&victim);
        }
        self.map.get(&key).and_then(|slot| slot.value.as_ref())
    }

    fn claim_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        seq
    }
}

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

pub(crate) fn blend(dst: &mut image::RgbImage, px: u32, py: u32, fg: Rgb, cov: u8) {
    if cov == 0 {
        return;
    }
    let (dst_w, dst_h) = (dst.width(), dst.height());
    if px >= dst_w || py >= dst_h {
        return;
    }
    let pix = dst.get_pixel_mut(px, py);
    let alpha = u32::from(cov);
    // Bound: fg*alpha + bg*(255-alpha) <= 255*255, so /255 <= 255 — the
    // conversion always succeeds and the saturating fallback never fires.
    let mix = |f: u8, b: u8| {
        u8::try_from((u32::from(f) * alpha + u32::from(b) * (255 - alpha)) / 255).unwrap_or(u8::MAX)
    };
    pix[0] = mix(fg.r, pix[0]);
    pix[1] = mix(fg.g, pix[1]);
    pix[2] = mix(fg.b, pix[2]);
}

pub(crate) fn fill_rect(
    dst: &mut image::RgbImage,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    color: Rgb,
) {
    let (dst_w, dst_h) = (dst.width(), dst.height());
    for off_y in 0..height {
        for off_x in 0..width {
            let (px, py) = (left + off_x, top + off_y);
            if px < dst_w && py < dst_h {
                dst.put_pixel(px, py, image::Rgb([color.r, color.g, color.b]));
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
    let w = (span_px.cast_signed() - 2 * u).max(3 * u);
    for dx in 0..w {
        blend(
            dst,
            (x0 + u + dx).max(0).cast_unsigned(),
            top.max(0).cast_unsigned(),
            fg,
            255,
        );
        blend(
            dst,
            (x0 + u + dx).max(0).cast_unsigned(),
            (top + h.cast_signed() - u).max(0).cast_unsigned(),
            fg,
            255,
        );
    }
    for dy in 0..h.cast_signed() {
        blend(
            dst,
            (x0 + u).max(0).cast_unsigned(),
            (top + dy).max(0).cast_unsigned(),
            fg,
            255,
        );
        blend(
            dst,
            (x0 + u + w - u).max(0).cast_unsigned(),
            (top + dy).max(0).cast_unsigned(),
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

/// `Default_Ignorable` codepoints (variation selectors, ZWJ, …) carry no ink
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
) -> Option<&'a (GlyphMetrics, Vec<u8>)> {
    cache.get_or_insert_with(GlyphKey { ch: c, face: idx }, || {
        let (m, bmp) = face.rasterize(c)?;
        if m.width == 0 || m.height == 0 || bmp.iter().all(|&p| p == 0) {
            None
        } else {
            Some((m, bmp))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> GlyphMetrics {
        GlyphMetrics {
            width: 8,
            height: 8,
            xmin: 0,
            ymin: 0,
            advance_width: 8.0,
            advance_height: 8.0,
        }
    }

    fn keys(n: usize) -> Vec<GlyphKey> {
        (0..)
            .filter_map(char::from_u32)
            .take(n)
            .map(|ch| GlyphKey {
                ch,
                face: FaceIdx::Regular,
            })
            .collect()
    }

    #[test]
    fn evicts_least_recently_inserted_past_cap() {
        let mut cache = GlyphCache::new();
        assert_eq!(cache.len(), 0);
        let ks = keys(MAX_GLYPH_CACHE_ENTRIES + 1);
        for (i, k) in ks.iter().enumerate() {
            let tag = u8::try_from(i % 251).expect("tag fits");
            cache.get_or_insert_with(*k, || Some((metrics(), vec![tag])));
        }
        assert_eq!(cache.len(), MAX_GLYPH_CACHE_ENTRIES);
        assert!(cache.get(ks[0]).is_none(), "oldest must be evicted");
        // Survivors still agree with what was inserted.
        let last = cache.get(ks[MAX_GLYPH_CACHE_ENTRIES]).expect("newest kept");
        assert_eq!(
            last.1,
            vec![u8::try_from(MAX_GLYPH_CACHE_ENTRIES % 251).expect("tag")]
        );
        assert_eq!(cache.len(), MAX_GLYPH_CACHE_ENTRIES);
    }

    #[test]
    fn recent_hit_protects_from_eviction() {
        let mut cache = GlyphCache::new();
        let ks = keys(MAX_GLYPH_CACHE_ENTRIES + 1);
        for k in &ks[..MAX_GLYPH_CACHE_ENTRIES] {
            cache.get_or_insert_with(*k, || Some((metrics(), vec![7])));
        }
        assert!(cache.get(ks[0]).is_some(), "refresh oldest");
        cache.get_or_insert_with(ks[MAX_GLYPH_CACHE_ENTRIES], || Some((metrics(), vec![9])));
        assert_eq!(cache.len(), MAX_GLYPH_CACHE_ENTRIES);
        assert!(cache.get(ks[0]).is_some(), "refreshed entry survives");
        assert!(cache.get(ks[1]).is_none(), "next-oldest evicted instead");
    }

    #[test]
    fn negative_results_share_the_cap() {
        let mut cache = GlyphCache::new();
        let ks = keys(MAX_GLYPH_CACHE_ENTRIES + 1);
        for k in &ks {
            assert!(cache.get_or_insert_with(*k, || None).is_none());
        }
        assert_eq!(cache.len(), MAX_GLYPH_CACHE_ENTRIES);
    }
}
