//! Pinned rendering profile + vendored font assets.
//!
//! A [`Profile`] fixes everything that affects pixels: font bytes (pinned by
//! SHA-256, not by filename), size, cell geometry, palette defaults, image
//! scale, padding, and cursor policy. Reports record the profile name and the
//! font hash, so a renderer change is distinguishable from an app regression.
//!
//! ## Font licensing
//!
//! The default family is **JetBrainsMono Nerd Font Mono** (Regular / Bold /
//! Italic / BoldItalic) under the **SIL Open Font License 1.1**:
//! redistribution in this repository is allowed provided the license text
//! ships alongside — see `assets/fonts/LICENSE-JetBrainsMono.txt` and
//! `assets/fonts/FONTS.md`. Upstream: <https://www.jetbrains.com/lp/mono/>,
//! patch project: <https://github.com/ryanoasis/nerd-fonts>.
//! `assets/fonts/DejaVuSansMNerdFontMono-Regular.ttf` (Bitstream Vera
//! license, `LICENSE-DejaVuSansMono.txt`) is kept in place for reference but
//! is no longer the default.
//!
//! Coverage reality (documented, not hidden): the vendored family covers
//! ASCII, box drawing, block elements, Braille, and Nerd-Font icons. What it
//! does NOT cover is served by the vendored fallback chain
//! ([`VENDORED_FALLBACK_FACES`]): Noto Sans Symbols 2 / Noto Sans Symbols
//! subsets for symbol codepoints (★ ☕ ⚷ ◐ ❤ …) and a Noto Sans CJK JP subset
//! (kana, JIS X 0208 level-1 kanji incl. 東京, fullwidth forms). Glyphs no
//! face in the whole chain covers (color emoji, Hangul, JIS level-2 kanji)
//! still render as a deterministic tofu box AND are reported in the
//! `.png.fidelity.json` sidecar — never silently. Wide codepoints keep their
//! 2-cell advance via `unicode-width`, so geometry stays terminal-like even
//! when the glyph is absent. `--font-file` overrides the embedded family
//! (single face; faux styles; hash recorded; the fallback chain still
//! applies).

use sha2::{Digest, Sha256};

/// Vendored pinned font bytes (reproducible on any machine).
pub const VENDORED_FONT: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-Regular.ttf");
pub const VENDORED_FONT_BOLD: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-Bold.ttf");
pub const VENDORED_FONT_ITALIC: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-Italic.ttf");
pub const VENDORED_FONT_BOLD_ITALIC: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFontMono-BoldItalic.ttf");

/// The four faces of one pinned monospace family, selected by `cell.mods`
/// (bold → Bold, italic → Italic, both → BoldItalic). The regular face pins
/// the geometry and the recorded hash; a non-regular face that fails to
/// parse falls back to regular with the faux double-strike / shear.
#[derive(Debug, Clone, Copy)]
pub struct FontFaces<'a> {
    pub regular: &'a [u8],
    pub bold: &'a [u8],
    pub italic: &'a [u8],
    pub bold_italic: &'a [u8],
}

impl<'a> FontFaces<'a> {
    /// Every slot = the same bytes (single-face override: faux styles).
    pub const fn single(bytes: &'a [u8]) -> Self {
        Self {
            regular: bytes,
            bold: bytes,
            italic: bytes,
            bold_italic: bytes,
        }
    }
}

/// The default vendored family (JetBrainsMono Nerd Font Mono).
pub const VENDORED_FACES: FontFaces<'static> = FontFaces {
    regular: VENDORED_FONT,
    bold: VENDORED_FONT_BOLD,
    italic: VENDORED_FONT_ITALIC,
    bold_italic: VENDORED_FONT_BOLD_ITALIC,
};

/// Vendored per-glyph fallback faces: Noto subsets covering what the primary
/// family lacks (see `assets/fonts/FONTS.md`; subsets reproducible via
/// `tools/subset_fonts.py`, SIL OFL 1.1, `LICENSE-Noto.txt`).
pub const VENDORED_SYMBOLS2_FONT: &[u8] =
    include_bytes!("../assets/fonts/NotoSansSymbols2-subset.ttf");
/// SHA-256 of [`VENDORED_SYMBOLS2_FONT`], pinned at load.
pub const VENDORED_SYMBOLS2_FONT_SHA256: &str =
    "e1d177a40af910100eceb0e825331e55f0cfd005bc0f26087fd4e58fbe60e6c5";
pub const VENDORED_SYMBOLS_FONT: &[u8] =
    include_bytes!("../assets/fonts/NotoSansSymbols-subset.ttf");
/// SHA-256 of [`VENDORED_SYMBOLS_FONT`], pinned at load.
pub const VENDORED_SYMBOLS_FONT_SHA256: &str =
    "6f9cc93e71f8676361c5db286368be046e75d42c0b841afbf3f50da6bb0a2b8a";
pub const VENDORED_CJK_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSansCJKjp-subset.otf");
/// SHA-256 of [`VENDORED_CJK_FONT`], pinned at load.
pub const VENDORED_CJK_FONT_SHA256: &str =
    "777bee41f0c6076c00ad919384359a6e396b8822cf9056041fca8fcf2759d897";

/// One pinned fallback face: font bytes + expected SHA-256 + description.
/// The hash is verified when a [`crate::render::Renderer`] loads the chain; a
/// mismatch refuses to render (explicit, never silent). Fallback faces draw
/// in their own regular weight regardless of `cell.mods`, centered and
/// clipped inside the cell box the primary geometry pins.
#[derive(Debug, Clone, Copy)]
pub struct FallbackFace<'a> {
    pub bytes: &'a [u8],
    pub sha256: &'a str,
    pub desc: &'a str,
}

/// The default per-glyph fallback chain, tried in order after the primary
/// family: Noto Sans Symbols 2 (Geometric Shapes, Miscellaneous Symbols,
/// Dingbats, Miscellaneous Symbols and Arrows), Noto Sans Symbols (misc
/// symbols unique to v1, e.g. U+26B7 ⚷), Noto Sans CJK JP (kana, JIS X 0208
/// level-1 kanji, fullwidth forms). [`crate::render::Renderer::new`] loads
/// this chain; [`crate::render::Renderer::with_fallbacks`] replaces it.
pub const VENDORED_FALLBACK_FACES: &[FallbackFace<'static>] = &[
    FallbackFace {
        bytes: VENDORED_SYMBOLS2_FONT,
        sha256: VENDORED_SYMBOLS2_FONT_SHA256,
        desc: "vendored NotoSansSymbols2 subset (SIL OFL 1.1)",
    },
    FallbackFace {
        bytes: VENDORED_SYMBOLS_FONT,
        sha256: VENDORED_SYMBOLS_FONT_SHA256,
        desc: "vendored NotoSansSymbols subset (SIL OFL 1.1)",
    },
    FallbackFace {
        bytes: VENDORED_CJK_FONT,
        sha256: VENDORED_CJK_FONT_SHA256,
        desc: "vendored NotoSansCJKjp subset: kana, JIS X 0208 level-1 kanji, fullwidth forms (SIL OFL 1.1)",
    },
];

/// Rendering profile. [`Profile::default_profile`] is the reproducible gate.
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    /// Pixels per Em for glyph rasterization (before `scale`).
    pub font_px: f32,
    /// Cell geometry in output pixels (before `scale`).
    pub cell_w: u32,
    pub cell_h: u32,
    pub pad: u32,
    /// Integer rasterization scale: glyphs are rasterized at
    /// `font_px * scale` straight onto the final image (HiDPI crispness, no
    /// post upscale).
    pub scale: u32,
    /// Terminal defaults.
    pub default_fg: crate::frame::Rgb,
    pub default_bg: crate::frame::Rgb,
    /// Font identity actually used (vendored or override).
    pub font_sha256: String,
    pub font_desc: String,
    /// Cursor policy: frozen-visible block cursor. Blink phase is ignored by
    /// design so reruns are deterministic.
    pub cursor_visible: bool,
}

impl Profile {
    /// The reproducible gate profile. Geometry is measured from the vendored
    /// font at init (see [`crate::render::measure`]) and then pinned here as
    /// constants so a font change fails loudly instead of shifting pixels.
    #[must_use]
    pub fn default_profile() -> Self {
        Self {
            name: "tuisnap-default".to_string(),
            font_px: 16.0,
            cell_w: 10,
            cell_h: 21,
            pad: 12,
            scale: 2,
            default_fg: crate::frame::Rgb::new(0xd0, 0xd0, 0xd0),
            default_bg: crate::frame::Rgb::new(0x00, 0x00, 0x00),
            font_sha256: font_sha256(VENDORED_FONT),
            font_desc: "vendored JetBrainsMonoNerdFontMono-Regular (SIL OFL 1.1)".to_string(),
            cursor_visible: true,
        }
    }

    #[must_use]
    pub fn with_font_file(mut self, desc: String, bytes: &[u8]) -> Self {
        self.font_sha256 = font_sha256(bytes);
        self.font_desc = desc;
        self
    }

    /// A reusable [`crate::render::Renderer`] pinned to this profile: faces
    /// parsed once, glyph rasters cached across frames. Bulk gates
    /// (`Store::check_with`/`Store::report_with`) should go through one of
    /// these per thread instead of the one-shot free functions.
    pub fn renderer(
        &self,
        faces: &FontFaces<'_>,
    ) -> Result<crate::render::Renderer, crate::render::RenderError> {
        crate::render::Renderer::new(self, faces)
    }

    /// Image dimensions for a `cols`×`rows` frame.
    #[must_use]
    pub fn image_size(&self, cols: u16, rows: u16) -> (u32, u32) {
        (
            (cols as u32 * self.cell_w + self.pad * 2) * self.scale,
            (rows as u32 * self.cell_h + self.pad * 2) * self.scale,
        )
    }
}

#[must_use]
pub fn font_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

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
/// delegates to [`crate::frame::Frame::resolve_cell`], so there is exactly one
/// color path. A profile claiming any other table would need a second
/// resolver — rejected by construction (unknown strings fail strict build).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IndexedPalette {
    Xterm,
}

/// Palette policy: terminal defaults plus the indexed table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PalettePolicy {
    pub default_fg: crate::frame::Rgb,
    pub default_bg: crate::frame::Rgb,
    pub indexed: IndexedPalette,
}

impl PalettePolicy {
    /// The gate palette: light-gray on black, xterm indexed table.
    #[must_use]
    pub const fn xterm() -> Self {
        Self {
            default_fg: crate::frame::Rgb::new(0xd0, 0xd0, 0xd0),
            default_bg: crate::frame::Rgb::new(0x00, 0x00, 0x00),
            indexed: IndexedPalette::Xterm,
        }
    }

    /// Resolve a cell to (fg, bg), honoring reverse/dim. Single color path:
    /// delegates to [`crate::frame::Frame::resolve_cell`].
    #[must_use]
    pub fn resolve(&self, cell: &crate::frame::Cell) -> (crate::frame::Rgb, crate::frame::Rgb) {
        crate::frame::Frame::resolve_cell(cell, self.default_fg, self.default_bg)
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
    name: String,
    faces: FontFaces<'a>,
    face_hashes: [String; 4],
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

impl<'a> RenderProfile<'a> {
    /// Strict constructor: verifies every pin, substitutes nothing.
    ///
    /// `face_hashes` are the expected SHA-256 pins for
    /// (regular, bold, italic, bold-italic) in that order; each is checked
    /// against the corresponding bytes in `faces`. Each fallback face carries
    /// its own pin (checked too) and the chain order is preserved verbatim —
    /// reorderings change [`RenderProfile::hash`].
    #[allow(clippy::too_many_arguments)]
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
    #[must_use]
    pub fn vendored() -> RenderProfile<'static> {
        RenderProfile::strict(
            "tuisnap-default".to_string(),
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
        .expect("vendored pins must match vendored bytes")
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

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub fn faces(&self) -> &FontFaces<'a> {
        &self.faces
    }
    #[must_use]
    pub fn face_hashes(&self) -> &[String; 4] {
        &self.face_hashes
    }
    #[must_use]
    pub fn fallback_order(&self) -> &[FallbackFace<'a>] {
        &self.fallback_order
    }
    #[must_use]
    pub fn font_px(&self) -> f32 {
        self.font_px
    }
    #[must_use]
    pub fn cell_w(&self) -> u32 {
        self.cell_w
    }
    #[must_use]
    pub fn cell_h(&self) -> u32 {
        self.cell_h
    }
    #[must_use]
    pub fn pad(&self) -> u32 {
        self.pad
    }
    #[must_use]
    pub fn scale(&self) -> u32 {
        self.scale
    }
    #[must_use]
    pub fn palette(&self) -> &PalettePolicy {
        &self.palette
    }
    #[must_use]
    pub fn cursor(&self) -> CursorPolicy {
        self.cursor
    }
    #[must_use]
    pub fn blink_phase(&self) -> BlinkPhase {
        self.blink_phase
    }
    #[must_use]
    pub fn missing(&self) -> MissingGlyphPolicy {
        self.missing
    }
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
        h.update(b"tuisnap-render-profile/1\n");
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
        h.update(format!("{:?}", p.indexed).as_bytes());
        h.update(format!("{:?}", self.cursor).as_bytes());
        h.update(format!("{:?}", self.blink_phase).as_bytes());
        h.update(format!("{:?}", self.missing).as_bytes());
        h.update(self.renderer_version.to_le_bytes());
        let digest = h.finalize();
        let mut s = String::with_capacity(digest.len() * 2);
        for b in digest {
            s.push_str(&format!("{b:02x}"));
        }
        s
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
            (cols as u32 * self.cell_w + self.pad * 2) * self.scale,
            (rows as u32 * self.cell_h + self.pad * 2) * self.scale,
        )
    }
}
