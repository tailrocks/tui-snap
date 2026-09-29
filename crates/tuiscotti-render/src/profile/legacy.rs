//! Legacy [`Profile`](super::Profile): geometry, palette, cursor policy.

use super::{FontFaces, VENDORED_FONT};
use sha2::{Digest, Sha256};

/// Rendering profile. [`Profile::default_profile`] is the reproducible gate.
#[derive(Debug, Clone)]
pub struct Profile {
    /// Profile name recorded in reports and bundle manifests.
    pub name: String,
    /// Pixels per Em for glyph rasterization (before `scale`).
    pub font_px: f32,
    /// Cell geometry in output pixels (before `scale`).
    pub cell_w: u32,
    /// Cell height in output pixels (before `scale`).
    pub cell_h: u32,
    /// Image padding in output pixels (before `scale`).
    pub pad: u32,
    /// Integer rasterization scale: glyphs are rasterized at
    /// `font_px * scale` straight onto the final image (`HiDPI` crispness, no
    /// post upscale).
    pub scale: u32,
    /// Terminal default foreground.
    pub default_fg: tuiscotti_core::frame::Rgb,
    /// Terminal default background.
    pub default_bg: tuiscotti_core::frame::Rgb,
    /// Font identity actually used (vendored or override).
    pub font_sha256: String,
    /// Human-readable identity of the regular face.
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
            default_fg: tuiscotti_core::frame::Rgb::new(0xd0, 0xd0, 0xd0),
            default_bg: tuiscotti_core::frame::Rgb::new(0x00, 0x00, 0x00),
            font_sha256: font_sha256(VENDORED_FONT),
            font_desc: "vendored JetBrainsMonoNerdFontMono-Regular (SIL OFL 1.1)".to_string(),
            cursor_visible: true,
        }
    }

    /// Single-face override: every slot renders `bytes` with faux styles.
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
    ///
    /// # Errors
    ///
    /// Returns `RenderError` when a face fails to load or the geometry pin breaks.
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
            (u32::from(cols) * self.cell_w + self.pad * 2) * self.scale,
            (u32::from(rows) * self.cell_h + self.pad * 2) * self.scale,
        )
    }
}

/// SHA-256 of font bytes as lowercase hex (face identity pins).
#[must_use]
pub fn font_sha256(bytes: &[u8]) -> String {
    crate::hex_bytes(&Sha256::digest(bytes))
}
