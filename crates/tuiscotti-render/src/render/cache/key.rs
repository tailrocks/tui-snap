//! Canonical fingerprint encodings and typed cache keys.
//!
//! Moved out of `cache.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use sha2::{Digest, Sha256};
use tuiscotti_core::frame::{Color, CursorStyle, Mods, Rgb, UnderlineStyle};
use tuiscotti_core::screen::Screen;

use super::super::RenderError;
use crate::profile::RenderProfile;

// ---------------------------------------------------------------------------
// Canonical field encodings (explicit bytes, never `Debug`).
// ---------------------------------------------------------------------------

/// Fingerprint format version. Bumped whenever the pre-image changes (v2:
/// underline color/style included, all `Debug`-fed fields canonically
/// encoded). Old `.cache` files carry the v1 header and are rejected by
/// [`RenderCache::get`](super::RenderCache::get), never served.
pub const CACHE_FINGERPRINT_VERSION: u32 = 2;

fn update_len_prefixed(h: &mut Sha256, bytes: &[u8]) {
    h.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
    h.update(bytes);
}

fn update_color(h: &mut Sha256, color: Color) {
    match color {
        Color::Default => h.update([0x00]),
        Color::Indexed(i) => h.update([0x01, i]),
        Color::Rgb(Rgb { r, g, b }) => h.update([0x02, r, g, b]),
    }
}

fn update_mods(h: &mut Sha256, mods: Mods) {
    let mut flags: u16 = 0;
    flags |= u16::from(mods.hidden);
    flags |= u16::from(mods.blink) << 1;
    flags |= u16::from(mods.bold) << 2;
    flags |= u16::from(mods.dim) << 3;
    flags |= u16::from(mods.italic) << 4;
    flags |= u16::from(mods.underline) << 5;
    flags |= u16::from(mods.strikethrough) << 6;
    flags |= u16::from(mods.reverse) << 7;
    let style: u16 = match mods.underline_style {
        UnderlineStyle::None => 0,
        UnderlineStyle::Single => 1,
        UnderlineStyle::Double => 2,
        UnderlineStyle::Curly => 3,
        UnderlineStyle::Dotted => 4,
        UnderlineStyle::Dashed => 5,
    };
    h.update((flags | (style << 8)).to_le_bytes());
}

fn cursor_style_byte(style: CursorStyle) -> u8 {
    match style {
        CursorStyle::Block => 0,
        CursorStyle::Underline => 1,
        CursorStyle::Bar => 2,
    }
}

/// Deterministic content hash of a screen's approval-relevant state (dims,
/// cells incl. underline color/style, cursor — no provenance, no origin: the
/// screen→frame adaptation drops both, so they cannot affect pixels). One
/// input of the cache key.
#[must_use]
pub fn screen_content_hash(screen: &Screen) -> String {
    let mut h = Sha256::new();
    h.update(b"tuiscotti-screen/2\n");
    h.update(screen.cols().to_le_bytes());
    h.update(screen.rows().to_le_bytes());
    for c in screen.cells() {
        h.update(c.x.to_le_bytes());
        h.update(c.y.to_le_bytes());
        update_len_prefixed(&mut h, c.symbol.as_bytes());
        h.update([c.width, u8::from(c.continuation)]);
        update_color(&mut h, c.fg);
        update_color(&mut h, c.bg);
        update_mods(&mut h, c.mods);
        update_color(&mut h, c.underline_color);
    }
    let cur = screen.cursor();
    h.update(cur.x.to_le_bytes());
    h.update(cur.y.to_le_bytes());
    h.update([u8::from(cur.visible), cursor_style_byte(cur.style)]);
    h.update([u8::from(cur.blinking)]);
    let digest = h.finalize();
    crate::hex_bytes(&digest)
}

fn update_profile(h: &mut Sha256, rp: &RenderProfile<'_>) {
    use crate::profile::{BlinkPhase, CursorPolicy, IndexedPalette, MissingGlyphPolicy};
    h.update(b"tuiscotti-render-profile-fingerprint/2\n");
    update_len_prefixed(h, rp.name().as_bytes());
    for pin in rp.face_hashes() {
        update_len_prefixed(h, pin.as_bytes());
    }
    // Ordered fallback chain: pins stand in for the bytes (strict
    // construction verified bytes-against-pin, and the renderer re-verifies
    // at render time), but ORDER and identity both participate.
    let order = rp.fallback_order();
    h.update(u32::try_from(order.len()).unwrap_or(u32::MAX).to_le_bytes());
    for f in order {
        update_len_prefixed(h, f.sha256.as_bytes());
        update_len_prefixed(h, f.desc.as_bytes());
    }
    h.update(rp.font_px().to_bits().to_le_bytes());
    h.update(rp.cell_w().to_le_bytes());
    h.update(rp.cell_h().to_le_bytes());
    h.update(rp.pad().to_le_bytes());
    h.update(rp.scale().to_le_bytes());
    let p = rp.palette();
    h.update([p.default_fg.r, p.default_fg.g, p.default_fg.b]);
    h.update([p.default_bg.r, p.default_bg.g, p.default_bg.b]);
    let indexed: u8 = match p.indexed {
        IndexedPalette::Xterm => 0,
    };
    let cursor: u8 = match rp.cursor() {
        CursorPolicy::Show => 0,
        CursorPolicy::Hide => 1,
    };
    let blink: u8 = match rp.blink_phase() {
        BlinkPhase::On => 0,
        BlinkPhase::Off => 1,
    };
    let missing: u8 = match rp.missing() {
        MissingGlyphPolicy::Strict => 0,
        MissingGlyphPolicy::Placeholder => 1,
    };
    h.update([indexed, cursor, blink, missing]);
    h.update(rp.renderer_version().to_le_bytes());
}

// ---------------------------------------------------------------------------
// Typed cache keys.
// ---------------------------------------------------------------------------

/// Validated content-addressed cache key: 64 lowercase hex chars (32 raw
/// bytes). The ONLY way to name a cache entry — raw strings never reach path
/// joins, so traversal is impossible by construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    hex: String,
    raw: [u8; 32],
}

impl CacheKey {
    /// Canonical key for a screen under a strict profile: SHA-256 over the
    /// screen content hash and the full canonical profile encoding above,
    /// domain-separated and fingerprinted at
    /// [`CACHE_FINGERPRINT_VERSION`].
    #[must_use]
    pub fn for_screen(screen: &Screen, rp: &RenderProfile<'_>) -> Self {
        let mut h = Sha256::new();
        h.update(b"tuiscotti-render-cache/2\n");
        h.update(CACHE_FINGERPRINT_VERSION.to_le_bytes());
        h.update(screen_content_hash(screen).as_bytes());
        h.update(b"\n");
        update_profile(&mut h, rp);
        let digest = h.finalize();
        let mut raw = [0u8; 32];
        raw.copy_from_slice(&digest);
        Self {
            hex: crate::hex_bytes(&digest),
            raw,
        }
    }

    /// Parse an externally supplied key (directory scan, test fixture).
    /// Strict: exactly 64 lowercase hex chars, nothing else.
    ///
    /// # Errors
    ///
    /// Returns `RenderError` when `s` is not 64 lowercase hex chars.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        if s.len() != 64
            || !s
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(RenderError(format!(
                "invalid cache key: expected 64 lowercase hex chars, got {s:?}"
            )));
        }
        let mut raw = [0u8; 32];
        let (chunks, _) = s.as_bytes().as_chunks::<2>();
        for (i, chunk) in chunks.iter().enumerate() {
            raw[i] = (hex_val(chunk[0]) << 4) | hex_val(chunk[1]);
        }
        Ok(Self {
            hex: s.to_string(),
            raw,
        })
    }

    /// Lowercase hex form (the entry file stem).
    #[must_use]
    pub fn hex(&self) -> &str {
        &self.hex
    }

    /// Raw key bytes (bound into the entry header).
    #[must_use]
    pub fn raw(&self) -> &[u8; 32] {
        &self.raw
    }

    /// Entry file name: `<hex>.cache`. Safe to join onto the cache dir: the
    /// stem is validated hex, so it cannot escape.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.cache", self.hex)
    }
}

impl std::fmt::Display for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.hex)
    }
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        _ => 0,
    }
}
