//! Pinned rendering profile + vendored font assets.
//!
//! A [`Profile`] fixes everything that affects pixels: font bytes (pinned by
//! SHA-256, not by filename), size, cell geometry, palette defaults, image
//! scale, padding, and cursor policy. Reports record the profile name and the
//! font hash, so a renderer change is distinguishable from an app regression.
//!
//! ## Font licensing
//!
//! The default family is **`JetBrainsMono` Nerd Font Mono** (Regular / Bold /
//! Italic / `BoldItalic`) under the **SIL Open Font License 1.1**:
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

pub mod fonts;
pub mod legacy;
pub mod strict;
pub mod strict_types;

pub use fonts::{
    FallbackFace, FontFaces, VENDORED_CJK_FONT, VENDORED_CJK_FONT_SHA256, VENDORED_FACES,
    VENDORED_FALLBACK_FACES, VENDORED_FONT, VENDORED_FONT_BOLD, VENDORED_FONT_BOLD_ITALIC,
    VENDORED_FONT_ITALIC, VENDORED_SYMBOLS_FONT, VENDORED_SYMBOLS_FONT_SHA256,
    VENDORED_SYMBOLS2_FONT, VENDORED_SYMBOLS2_FONT_SHA256,
};
pub use legacy::{Profile, font_sha256};
pub use strict_types::{
    BlinkPhase, CursorPolicy, IndexedPalette, MissingGlyphPolicy, PalettePolicy, ProfileError,
    RENDERER_VERSION, RenderProfile, VENDORED_FONT_BOLD_ITALIC_SHA256, VENDORED_FONT_BOLD_SHA256,
    VENDORED_FONT_ITALIC_SHA256, VENDORED_FONT_SHA256,
};
