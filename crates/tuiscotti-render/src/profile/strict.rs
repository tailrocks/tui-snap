//! Strict [`RenderProfile`] construction and hashing.

use super::{
    BlinkPhase, CursorPolicy, FallbackFace, FontFaces, IndexedPalette, MissingGlyphPolicy,
    PalettePolicy, Profile, ProfileError, RENDERER_VERSION, RenderProfile, VENDORED_FACES,
    VENDORED_FALLBACK_FACES, VENDORED_FONT_BOLD_ITALIC_SHA256, VENDORED_FONT_BOLD_SHA256,
    VENDORED_FONT_ITALIC_SHA256, VENDORED_FONT_SHA256, font_sha256,
};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

impl<'a> RenderProfile<'a> {
    /// Strict constructor: verifies every pin, substitutes nothing.
    ///
    /// `face_hashes` are the expected SHA-256 pins for
    /// (regular, bold, italic, bold-italic) in that order; each is checked
    /// against the corresponding bytes in `faces`. Each fallback face carries
    /// its own pin (checked too) and the chain order is preserved verbatim —
    /// reorderings change [`RenderProfile::hash`].
    ///
    /// # Errors
    ///
    /// Returns `ProfileError` when any pin, range, or version check fails.
    #[expect(
        clippy::too_many_arguments,
        reason = "strict constructor takes every pin explicitly; a builder would hide required pins"
    )]
    pub fn strict(
        name: String,
        faces: FontFaces<'a>,
        face_hashes: [&str; 4],
        fallback_order: Vec<FallbackFace<'a>>,
        font_px: f32,
        cell_w: u32,
        cell_h: u32,
        pad: u32,
        scale: u32,
        palette: PalettePolicy,
        cursor: CursorPolicy,
        blink_phase: BlinkPhase,
        missing: MissingGlyphPolicy,
        renderer_version: u32,
    ) -> Result<Self, ProfileError> {
        if renderer_version != RENDERER_VERSION {
            return Err(ProfileError(format!(
                "renderer version {renderer_version} != pinned {RENDERER_VERSION}"
            )));
        }
        if !font_px.is_finite() || font_px <= 0.0 {
            return Err(ProfileError(format!(
                "font_px must be positive finite, got {font_px}"
            )));
        }
        if cell_w == 0 || cell_h == 0 {
            return Err(ProfileError(format!(
                "cell geometry must be nonzero, got {cell_w}x{cell_h}"
            )));
        }
        if scale == 0 {
            return Err(ProfileError("scale must be nonzero".to_string()));
        }
        let slots = [
            ("regular", faces.regular, face_hashes[0]),
            ("bold", faces.bold, face_hashes[1]),
            ("italic", faces.italic, face_hashes[2]),
            ("bold-italic", faces.bold_italic, face_hashes[3]),
        ];
        for (label, bytes, pin) in slots {
            let actual = font_sha256(bytes);
            if actual != pin {
                return Err(ProfileError(format!(
                    "{label} face sha256 mismatch: pinned {pin}, got {actual} — refusing profile"
                )));
            }
        }
        for f in &fallback_order {
            let actual = font_sha256(f.bytes);
            if actual != f.sha256 {
                return Err(ProfileError(format!(
                    "fallback face '{}' sha256 mismatch: pinned {}, got {actual} — refusing profile",
                    f.desc, f.sha256
                )));
            }
        }
        Ok(Self {
            name,
            faces,
            face_hashes: face_hashes.map(str::to_string),
            fallback_order,
            font_px,
            cell_w,
            cell_h,
            pad,
            scale,
            palette,
            cursor,
            blink_phase,
            missing,
            renderer_version,
        })
    }

    /// The reproducible gate profile: vendored family + vendored fallback
    /// chain, strict missing policy, blink sampled on, cursor shown.
    /// Panics only if the vendored pins disagree with the vendored bytes
    /// (a build-time inconsistency, not a runtime condition).
    ///
    /// The strict construction (pin verification over ~11MB of static font
    /// bytes) runs once per process: every input is a `&'static` asset, so a
    /// cached verified value is identical to a fresh one. [`RenderProfile::strict`]
    /// itself always re-verifies caller bytes and is never cached.
    #[must_use]
    pub fn vendored() -> RenderProfile<'static> {
        static VENDORED: OnceLock<RenderProfile<'static>> = OnceLock::new();
        VENDORED
            .get_or_init(|| {
                RenderProfile::strict(
                    "tuiscotti-default".to_string(),
                    VENDORED_FACES,
                    [
                        VENDORED_FONT_SHA256,
                        VENDORED_FONT_BOLD_SHA256,
                        VENDORED_FONT_ITALIC_SHA256,
                        VENDORED_FONT_BOLD_ITALIC_SHA256,
                    ],
                    VENDORED_FALLBACK_FACES.to_vec(),
                    16.0,
                    10,
                    21,
                    12,
                    2,
                    PalettePolicy::xterm(),
                    CursorPolicy::Show,
                    BlinkPhase::On,
                    MissingGlyphPolicy::Strict,
                    RENDERER_VERSION,
                )
                .unwrap_or_else(|e| unreachable!("vendored pins must match vendored bytes: {e}"))
            })
            .clone()
    }

    /// Same profile sampling the other blink phase (V07 stills).
    #[must_use]
    pub fn with_phase(&self, phase: BlinkPhase) -> Self {
        let mut c = self.clone();
        c.blink_phase = phase;
        c
    }

    /// Same profile with a different missing-glyph policy (V05).
    #[must_use]
    pub fn with_missing(&self, missing: MissingGlyphPolicy) -> Self {
        let mut c = self.clone();
        c.missing = missing;
        c
    }

    /// Same profile with a different cursor policy.
    #[must_use]
    pub fn with_cursor(&self, cursor: CursorPolicy) -> Self {
        let mut c = self.clone();
        c.cursor = cursor;
        c
    }

    /// Profile name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Pinned styled-face bytes.
    #[must_use]
    pub fn faces(&self) -> &FontFaces<'a> {
        &self.faces
    }
    /// Pinned styled-face SHA-256 hashes, in `faces` order.
    #[must_use]
    pub fn face_hashes(&self) -> &[String; 4] {
        &self.face_hashes
    }
    /// Ordered per-glyph fallback chain.
    #[must_use]
    pub fn fallback_order(&self) -> &[FallbackFace<'a>] {
        &self.fallback_order
    }
    /// Pixels per Em for glyph rasterization (before `scale`).
    #[must_use]
    pub fn font_px(&self) -> f32 {
        self.font_px
    }
    /// Cell width in output pixels (before `scale`).
    #[must_use]
    pub fn cell_w(&self) -> u32 {
        self.cell_w
    }
    /// Cell height in output pixels (before `scale`).
    #[must_use]
    pub fn cell_h(&self) -> u32 {
        self.cell_h
    }
    /// Image padding in output pixels (before `scale`).
    #[must_use]
    pub fn pad(&self) -> u32 {
        self.pad
    }
    /// Integer rasterization scale.
    #[must_use]
    pub fn scale(&self) -> u32 {
        self.scale
    }
    /// Palette policy.
    #[must_use]
    pub fn palette(&self) -> &PalettePolicy {
        &self.palette
    }
    /// Cursor policy for still renders.
    #[must_use]
    pub fn cursor(&self) -> CursorPolicy {
        self.cursor
    }
    /// Blink sample phase for still renders.
    #[must_use]
    pub fn blink_phase(&self) -> BlinkPhase {
        self.blink_phase
    }
    /// Missing-glyph policy.
    #[must_use]
    pub fn missing(&self) -> MissingGlyphPolicy {
        self.missing
    }
    /// Pinned renderer version.
    #[must_use]
    pub fn renderer_version(&self) -> u32 {
        self.renderer_version
    }

    /// Content hash over every pin (face hashes, fallback hashes IN ORDER,
    /// geometry, scale, palette, cursor, blink phase, missing policy,
    /// renderer version). Part of every render-cache key (V08).
    #[must_use]
    pub fn hash(&self) -> String {
        let mut h = Sha256::new();
        h.update(b"tuiscotti-render-profile/1\n");
        h.update(self.name.as_bytes());
        h.update(b"\n");
        for pin in &self.face_hashes {
            h.update(pin.as_bytes());
            h.update(b"\n");
        }
        for f in &self.fallback_order {
            h.update(f.sha256.as_bytes());
            h.update(b"|");
            h.update(f.desc.as_bytes());
            h.update(b"\n");
        }
        h.update(self.font_px.to_le_bytes());
        h.update(self.cell_w.to_le_bytes());
        h.update(self.cell_h.to_le_bytes());
        h.update(self.pad.to_le_bytes());
        h.update(self.scale.to_le_bytes());
        let p = &self.palette;
        h.update([p.default_fg.r, p.default_fg.g, p.default_fg.b]);
        h.update([p.default_bg.r, p.default_bg.g, p.default_bg.b]);
        h.update(indexed_policy_bytes(p.indexed));
        h.update(cursor_policy_bytes(self.cursor));
        h.update(blink_phase_bytes(self.blink_phase));
        h.update(missing_policy_bytes(self.missing));
        h.update(self.renderer_version.to_le_bytes());
        let digest = h.finalize();
        crate::hex_bytes(&digest)
    }

    /// The legacy [`Profile`] this strict profile pins (same geometry, scale,
    /// palette defaults, cursor policy). The renderer runs ONE engine over
    /// this; strictness (hash verification, missing policy, blink sampling)
    /// wraps that engine, never forks it.
    #[must_use]
    pub fn to_profile(&self) -> Profile {
        Profile {
            name: self.name.clone(),
            font_px: self.font_px,
            cell_w: self.cell_w,
            cell_h: self.cell_h,
            pad: self.pad,
            scale: self.scale,
            default_fg: self.palette.default_fg,
            default_bg: self.palette.default_bg,
            font_sha256: self.face_hashes[0].clone(),
            font_desc: format!("strict profile '{}' (regular face pin)", self.name),
            cursor_visible: self.cursor == CursorPolicy::Show,
        }
    }

    /// Image dimensions for a `cols`×`rows` grid.
    #[must_use]
    pub fn image_size(&self, cols: u16, rows: u16) -> (u32, u32) {
        (
            (u32::from(cols) * self.cell_w + self.pad * 2) * self.scale,
            (u32::from(rows) * self.cell_h + self.pad * 2) * self.scale,
        )
    }
}

/// Explicit pre-image bytes for the policy enums hashed by
/// [`RenderProfile::hash`]. Each arm feeds byte-for-byte what `{:?}`
/// rendered before (the enums are fieldless with derived `Debug`, so the
/// rendering is the variant name): digests are unchanged, only the
/// per-hash `format!` allocations are gone. Exhaustive matches keep new
/// variants from silently reusing another arm's bytes.
fn indexed_policy_bytes(indexed: IndexedPalette) -> &'static [u8] {
    match indexed {
        IndexedPalette::Xterm => b"Xterm",
    }
}

/// Explicit pre-image bytes for [`CursorPolicy`] (see [`indexed_policy_bytes`]).
fn cursor_policy_bytes(cursor: CursorPolicy) -> &'static [u8] {
    match cursor {
        CursorPolicy::Show => b"Show",
        CursorPolicy::Hide => b"Hide",
    }
}

/// Explicit pre-image bytes for [`BlinkPhase`] (see [`indexed_policy_bytes`]).
fn blink_phase_bytes(phase: BlinkPhase) -> &'static [u8] {
    match phase {
        BlinkPhase::On => b"On",
        BlinkPhase::Off => b"Off",
    }
}

/// Explicit pre-image bytes for [`MissingGlyphPolicy`] (see [`indexed_policy_bytes`]).
fn missing_policy_bytes(missing: MissingGlyphPolicy) -> &'static [u8] {
    match missing {
        MissingGlyphPolicy::Strict => b"Strict",
        MissingGlyphPolicy::Placeholder => b"Placeholder",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_bytes_match_debug_rendering() {
        // Guards the no-alloc refactor: if a `Debug` impl ever stops
        // rendering as the bare variant name, the digest would silently
        // change — this fails first.
        assert_eq!(indexed_policy_bytes(IndexedPalette::Xterm), b"Xterm");
        assert_eq!(
            indexed_policy_bytes(IndexedPalette::Xterm),
            format!("{:?}", IndexedPalette::Xterm).as_bytes()
        );
        for cursor in [CursorPolicy::Show, CursorPolicy::Hide] {
            assert_eq!(
                cursor_policy_bytes(cursor),
                format!("{cursor:?}").as_bytes()
            );
        }
        for phase in [BlinkPhase::On, BlinkPhase::Off] {
            assert_eq!(blink_phase_bytes(phase), format!("{phase:?}").as_bytes());
        }
        for missing in [MissingGlyphPolicy::Strict, MissingGlyphPolicy::Placeholder] {
            assert_eq!(
                missing_policy_bytes(missing),
                format!("{missing:?}").as_bytes()
            );
        }
    }

    #[test]
    fn vendored_is_cached_and_verified() {
        // Same value across calls (the second serves the cache), and the
        // pins still describe the vendored bytes exactly.
        let a = RenderProfile::vendored();
        let b = RenderProfile::vendored();
        assert_eq!(a.hash(), b.hash());
        assert_eq!(
            a.face_hashes(),
            &[
                VENDORED_FONT_SHA256.to_string(),
                VENDORED_FONT_BOLD_SHA256.to_string(),
                VENDORED_FONT_ITALIC_SHA256.to_string(),
                VENDORED_FONT_BOLD_ITALIC_SHA256.to_string(),
            ]
        );
        for (bytes, pin) in [
            (a.faces().regular, VENDORED_FONT_SHA256),
            (a.faces().bold, VENDORED_FONT_BOLD_SHA256),
            (a.faces().italic, VENDORED_FONT_ITALIC_SHA256),
            (a.faces().bold_italic, VENDORED_FONT_BOLD_ITALIC_SHA256),
        ] {
            assert_eq!(font_sha256(bytes), pin);
        }
    }
}
