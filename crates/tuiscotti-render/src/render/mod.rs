//! Pinned-profile rendering: canonical [`Frame`](tuiscotti_core::frame::Frame) → PNG / SVG / ANSI / text.
//!
//! The PNG path rasterizes **real glyphs** with `swash` from pinned font
//! bytes — never placeholder blocks. Glyphs are rasterized at the FINAL scale
//! (`font_px * scale`) straight onto the output image, so HiDPI output keeps
//! real coverage gradations instead of nearest-neighbor 2×2
//! blocks. [`verify_geometry`] fails loudly if the regular face's measured
//! advance/line-height drifts from the profile constants, so a font change
//! reads as a renderer change, not an app regression.
//!
//! Fidelity contract (measured, terminal-like — NOT pixel-identity with any
//! particular terminal emulator):
//! - layout from frame widths (wide = 2 cells, continuation = 0); CJK keeps
//!   2-cell geometry even when the glyph is missing (tofu fallback);
//! - bold / italic / bold-italic use the REAL faces of the pinned family
//!   (faux double-strike / shear survive only as fallback when a face fails
//!   to load or the family is a single-face override);
//! - per-glyph face chain: styled face → regular face → vendored fallback
//!   faces ([`crate::profile::VENDORED_FALLBACK_FACES`], sha256-pinned Noto
//!   subsets) → tofu; a face covers a codepoint only when it rasterizes a
//!   non-empty bitmap (cmap index is not enough); fallback glyphs are
//!   centered and clipped inside the primary cell box and drawn in the
//!   fallback face's own weight; default-ignorable codepoints (VS16, ZWJ)
//!   never tofu;
//! - underline / strikethrough drawn at fixed offsets from the baseline,
//!   including across whitespace cells (as real terminals do);
//! - blink frozen as visible; concealed glyphs omitted (see [`tuiscotti_core::frame`]);
//! - glyphs no face in the chain covers draw a deterministic tofu box AND are
//!   reported in the [`Fidelity`] record (the `.png.fidelity.json` sidecar),
//!   and glyphs served by a fallback face are recorded there too — exact
//!   reporting, never silent tofu.

pub mod bundle;
pub mod cache;
pub mod draw;
pub mod fidelity;
pub mod fonts;
pub mod frame;
pub mod glyph;
pub mod renderer;
pub mod svg;
pub mod text;

pub use bundle::{BundleManifest, ContractBytes, check_contract_bytes};
pub use cache::{RenderCache, render_cache_disabled, screen_content_hash, set_no_cache_override};
pub(crate) use draw::{draw_primary, draw_symbol};
pub use fidelity::{Artifacts, FallbackGlyph, Fidelity, MissingGlyph, Rendered};
pub use fonts::{FontSet, GlyphMetrics, LoadedFont, load_font, measure, verify_geometry};
pub use frame::{
    frame_from_screen, redact_frame, redact_screen, render_frame_strict, render_screen,
    render_screen_png,
};
pub(crate) use glyph::{
    CellSinks, FaceIdx, GlyphCache, GlyphKey, blend, cached_raster, draw_tofu, face_idx, fill_rect,
    is_default_ignorable,
};
pub use renderer::Renderer;
pub(crate) use svg::html_document;
pub use svg::{
    escape_html, escape_html_attr, escape_json_for_script, render_svg, render_svg_phased,
};
pub use text::{ansi_dump, render_png, render_png_report};

/// Import/render failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderError(pub String);

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "render error: {}", self.0)
    }
}

impl std::error::Error for RenderError {}

#[cfg(test)]
mod tests {
    use crate::profile::Profile;
    use crate::render::*;
    use tuiscotti_core::frame::{Frame, Provenance};

    #[test]
    fn html_alt_escapes_quote_breakout() {
        let frame = Frame::blank(
            2,
            1,
            Provenance {
                tool: "tuisnap".into(),
                tool_version: "test".into(),
                profile: "tuisnap-default".into(),
                source: "test".into(),
                argv: vec![],
                created_unix: 0,
            },
        );
        let html = html_document(&frame, &Profile::default_profile(), r#"x" onload="#, b"");
        assert!(html.contains(r#"alt="x&quot; onload=""#), "{html}");
        assert!(
            !html.contains(r#"alt="x" onload="#),
            "raw attribute breakout: {html}"
        );
    }
}
