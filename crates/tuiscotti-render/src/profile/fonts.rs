//! Pinned font assets: vendored family, fallback chain, hashes.

/// Vendored pinned font bytes (reproducible on any machine).
pub const VENDORED_FONT: &[u8] =
    include_bytes!("../../../../assets/fonts/JetBrainsMonoNerdFontMono-Regular.ttf");
/// Vendored bold face bytes.
pub const VENDORED_FONT_BOLD: &[u8] =
    include_bytes!("../../../../assets/fonts/JetBrainsMonoNerdFontMono-Bold.ttf");
/// Vendored italic face bytes.
pub const VENDORED_FONT_ITALIC: &[u8] =
    include_bytes!("../../../../assets/fonts/JetBrainsMonoNerdFontMono-Italic.ttf");
/// Vendored bold-italic face bytes.
pub const VENDORED_FONT_BOLD_ITALIC: &[u8] =
    include_bytes!("../../../../assets/fonts/JetBrainsMonoNerdFontMono-BoldItalic.ttf");

/// The four faces of one pinned monospace family, selected by `cell.mods`
/// (bold → Bold, italic → Italic, both → `BoldItalic`). The regular face pins
/// the geometry and the recorded hash; a non-regular face that fails to
/// parse falls back to regular with the faux double-strike / shear.
#[derive(Debug, Clone, Copy)]
pub struct FontFaces<'a> {
    /// Regular face bytes (geometry authority).
    pub regular: &'a [u8],
    /// Bold face bytes.
    pub bold: &'a [u8],
    /// Italic face bytes.
    pub italic: &'a [u8],
    /// Bold-italic face bytes.
    pub bold_italic: &'a [u8],
}

impl<'a> FontFaces<'a> {
    /// Every slot = the same bytes (single-face override: faux styles).
    #[must_use]
    pub const fn single(bytes: &'a [u8]) -> Self {
        Self {
            regular: bytes,
            bold: bytes,
            italic: bytes,
            bold_italic: bytes,
        }
    }
}

/// The default vendored family (`JetBrainsMono` Nerd Font Mono).
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
    include_bytes!("../../../../assets/fonts/NotoSansSymbols2-subset.ttf");
/// SHA-256 of [`VENDORED_SYMBOLS2_FONT`], pinned at load.
pub const VENDORED_SYMBOLS2_FONT_SHA256: &str =
    "e1d177a40af910100eceb0e825331e55f0cfd005bc0f26087fd4e58fbe60e6c5";
/// Vendored Noto Sans Symbols subset bytes.
pub const VENDORED_SYMBOLS_FONT: &[u8] =
    include_bytes!("../../../../assets/fonts/NotoSansSymbols-subset.ttf");
/// SHA-256 of [`VENDORED_SYMBOLS_FONT`], pinned at load.
pub const VENDORED_SYMBOLS_FONT_SHA256: &str =
    "6f9cc93e71f8676361c5db286368be046e75d42c0b841afbf3f50da6bb0a2b8a";
/// Vendored Noto Sans CJK JP subset bytes.
pub const VENDORED_CJK_FONT: &[u8] =
    include_bytes!("../../../../assets/fonts/NotoSansCJKjp-subset.otf");
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
    /// Face font bytes.
    pub bytes: &'a [u8],
    /// Expected SHA-256 pin of `bytes`.
    pub sha256: &'a str,
    /// Human-readable face identity.
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
