//! Reusable [`Renderer`](super::Renderer): pinned faces, cached rasters, PNG/HTML.

use super::{
    Artifacts, CellSinks, FallbackGlyph, Fidelity, FontSet, GlyphCache, MissingGlyph, RenderError,
    Rendered, ansi_dump, blend, draw_symbol, fill_rect, frame_from_screen, html_document,
    load_font, verify_geometry,
};
use crate::profile::BlinkPhase;
use crate::profile::FontFaces;
use crate::profile::MissingGlyphPolicy;
use crate::profile::Profile;
use crate::profile::RenderProfile;
use tuiscotti_core::frame::Frame;
use tuiscotti_core::screen::Screen;

mod strict;

/// A reusable renderer: parses the pinned faces ONCE at construction (the
/// geometry pin is verified there too) and caches glyph rasters across
/// frames, so a bulk gate costs O(distinct glyphs) rasterizations instead of
/// 8 font parses plus a full re-rasterization per frame.
///
/// Threading: every method takes `&mut self`, so the borrow checker enforces
/// exclusive use — give each parallel test thread its own instance
/// (`thread_local!` is the convenient carrier), matching the per-thread
/// session confinement of the PTY layer.
pub struct Renderer {
    profile: Profile,
    set: FontSet,
    glyphs: GlyphCache,
    /// V05 strict missing-glyph policy: fail instead of tofu. Legacy
    /// constructors leave this false (placeholder + fidelity record, the
    /// long-standing behavior); [`Self::for_render_profile`] sets it from
    /// the strict profile.
    strict_missing: bool,
    /// V07 still sample phase for `mods.blink` cells. Legacy constructors
    /// sample [`BlinkPhase::On`] (frozen-visible, byte-identical to before).
    blink_phase: BlinkPhase,
}

impl Renderer {
    /// Load the faces and verify the geometry pin (once for all renders).
    /// The per-glyph fallback chain is the vendored default
    /// ([`crate::profile::VENDORED_FALLBACK_FACES`]); use
    /// [`Self::with_fallbacks`] to replace it.
    pub fn new(profile: &Profile, faces: &FontFaces<'_>) -> Result<Self, RenderError> {
        Self::with_fallbacks(profile, faces, crate::profile::VENDORED_FALLBACK_FACES)
    }

    /// Like [`Self::new`] but with an explicit per-glyph fallback chain,
    /// tried in order after the primary family (pass `&[]` for
    /// primary-family-only rendering). Each face's bytes are verified
    /// against its pinned SHA-256 before parsing; a mismatch refuses to
    /// render. Primary geometry stays pinned to `faces.regular` regardless —
    /// fallback faces only fill coverage holes inside the pinned cell box.
    pub fn with_fallbacks(
        profile: &Profile,
        faces: &FontFaces<'_>,
        fallbacks: &[crate::profile::FallbackFace<'_>],
    ) -> Result<Self, RenderError> {
        // Geometry pins are UNSCALED metrics; the gate compares against the
        // profile constants directly.
        let unscaled = load_font(faces.regular, profile.font_px)?;
        verify_geometry(&unscaled, profile)?;
        // HiDPI: rasterize glyphs at the final scale, no post upscale.
        let set =
            FontSet::load_with_fallbacks(faces, profile.font_px * profile.scale as f32, fallbacks)?;
        Ok(Self {
            profile: profile.clone(),
            set,
            glyphs: GlyphCache::new(),
            strict_missing: false,
            blink_phase: BlinkPhase::On,
        })
    }

    /// Build a renderer from a strict [`RenderProfile`] (V01/V05/V07): the
    /// SAME engine as [`Self::new`] (geometry pinned to the regular face,
    /// same fallback chain mechanics), plus the profile's missing-glyph
    /// policy and blink sample phase. The profile's hashes were already
    /// verified at strict construction; the chain is verified again at load
    /// (defense in depth — a face swap between the two still refuses).
    pub fn for_render_profile(rp: &RenderProfile<'_>) -> Result<Self, RenderError> {
        // Re-verify the primary pins at render time: the bytes are borrowed,
        // so a swap between strict construction and this call must still
        // refuse (fallback pins are re-verified inside `with_fallbacks`).
        let faces = *rp.faces();
        let slots = [
            ("regular", faces.regular, &rp.face_hashes()[0]),
            ("bold", faces.bold, &rp.face_hashes()[1]),
            ("italic", faces.italic, &rp.face_hashes()[2]),
            ("bold-italic", faces.bold_italic, &rp.face_hashes()[3]),
        ];
        for (label, bytes, pin) in slots {
            let actual = crate::profile::font_sha256(bytes);
            if actual != *pin {
                return Err(RenderError(format!(
                    "{label} face sha256 mismatch at render: pinned {pin}, got {actual} — refusing to render"
                )));
            }
        }
        let profile = rp.to_profile();
        let mut r = Self::with_fallbacks(&profile, &faces, rp.fallback_order())?;
        r.strict_missing = rp.missing() == MissingGlyphPolicy::Strict;
        r.blink_phase = rp.blink_phase();
        Ok(r)
    }

    /// The profile this renderer is pinned to.
    #[must_use]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Distinct `(char, face)` rasters currently cached (diagnostics).
    #[must_use]
    pub fn cached_glyphs(&self) -> usize {
        self.glyphs.len()
    }

    /// Render a validated frame to PNG bytes.
    pub fn render_png(&mut self, frame: &Frame) -> Result<Vec<u8>, RenderError> {
        Ok(self.render(frame)?.png)
    }

    /// Render a validated [`Screen`] (V01): the screen is adapted to the
    /// render input losslessly ([`frame_from_screen`]) and run through the
    /// SAME engine as [`Self::render`] — one code path for saved, direct,
    /// and live screens. The screen origin is positional metadata and does
    /// not affect pixels; grid, cursor, colors, and modifiers do.
    pub fn render_screen(&mut self, screen: &Screen) -> Result<Rendered, RenderError> {
        let frame = frame_from_screen(screen, &self.profile.name);
        self.render(&frame)
    }

    /// [`Self::render_screen`] returning PNG bytes only.
    pub fn render_screen_png(&mut self, screen: &Screen) -> Result<Vec<u8>, RenderError> {
        Ok(self.render_screen(screen)?.png)
    }

    /// Standalone colored HTML render of a frame: the authoritative PNG as
    /// the primary `<img>` (real glyphs, including CJK/symbols the viewer
    /// font would tofu), a selectable SVG overlay with transparent fills
    /// (viewer fonts, copy/select only), and the canonical frame JSON
    /// embedded in a `<script type="application/json">` for lossless re-import.
    ///
    /// The embedded JSON carries `provenance.created_unix = 0`: the timestamp
    /// is informational only (excluded from every gate by design), and
    /// zeroing it keeps the document byte-deterministic for identical
    /// screens. Every other field is preserved.
    pub fn render_html(&mut self, frame: &Frame, title: &str) -> Result<String, RenderError> {
        let rendered = self.render(frame)?;
        Ok(html_document(frame, &self.profile, title, &rendered.png))
    }

    /// Generate all four snapshot artifacts in one render pass (the PNG is
    /// rasterized once and shared by the HTML embed and the PNG artifact).
    pub fn render_artifacts(
        &mut self,
        frame: &Frame,
        title: &str,
    ) -> Result<Artifacts, RenderError> {
        let rendered = self.render(frame)?;
        Ok(Artifacts {
            ansi: ansi_dump(frame),
            txt: frame.text(),
            html: html_document(frame, &self.profile, title, &rendered.png),
            png: rendered.png,
            fidelity: rendered.fidelity,
        })
    }

    /// Render plus exact coverage accounting (see [`Fidelity`]).
    pub fn render(&mut self, frame: &Frame) -> Result<Rendered, RenderError> {
        frame
            .validate()
            .map_err(|e| RenderError(format!("refusing to render: {e}")))?;
        let u = self.profile.scale;
        let cell_w = self.profile.cell_w * u;
        let cell_h = self.profile.cell_h * u;
        let pad = self.profile.pad * u;
        let u_i = u as i32;

        let w = frame.cols as u32 * cell_w + pad * 2;
        let h = frame.rows as u32 * cell_h + pad * 2;
        let bg = self.profile.default_bg;
        let mut img = image::RgbImage::from_pixel(w, h, image::Rgb([bg.r, bg.g, bg.b]));
        let mut missing: Vec<MissingGlyph> = Vec::new();
        let mut fallback_glyphs: Vec<FallbackGlyph> = Vec::new();

        for y in 0..frame.rows {
            for x in 0..frame.cols {
                self.draw_cell(&mut img, frame, x, y, &mut missing, &mut fallback_glyphs);
            }
        }

        self.draw_cursor(&mut img, frame);
        strict::check_strict_missing(self.strict_missing, &missing)?;

        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|e| RenderError(format!("PNG encode: {e}")))?;
        let fidelity = Fidelity {
            profile: self.profile.name.clone(),
            font_sha256: self.profile.font_sha256.clone(),
            font_desc: self.profile.font_desc.clone(),
            scale: u,
            approximate: !missing.is_empty() || !self.set.fell_back.is_empty(),
            faces_fell_back: self.set.fell_back.iter().map(|s| s.to_string()).collect(),
            missing,
            fallback_glyphs,
        };
        Ok(Rendered { png: out, fidelity })
    }

    /// Paint one lead cell: background, sampled-blink/hidden gating, glyph,
    /// and text decorations. Continuation cells are skipped (the lead cell
    /// spans them).
    fn draw_cell(
        &mut self,
        mut img: &mut image::RgbImage,
        frame: &Frame,
        x: u16,
        y: u16,
        mut missing: &mut Vec<MissingGlyph>,
        mut fallback_glyphs: &mut Vec<FallbackGlyph>,
    ) {
        let Some(cell) = frame.get(x, y) else {
            return;
        };
        if cell.continuation {
            return;
        }
        let profile = &self.profile;
        let u = profile.scale;
        let cell_w = profile.cell_w * u;
        let cell_h = profile.cell_h * u;
        let pad = profile.pad * u;
        let u_i = u as i32;
        {
            let (fg, cbg) = Frame::resolve_cell(cell, profile.default_fg, profile.default_bg);
            let span = u32::from(cell.width.max(1)) * cell_w;
            let cx = pad + x as u32 * cell_w;
            let cy = pad + y as u32 * cell_h;
            if cbg != profile.default_bg {
                fill_rect(&mut img, cx, cy, span, cell_h, cbg);
            }
            // V07: a still samples one declared blink phase. Off-phase
            // blinking cells keep their background but draw no ink and no
            // text decorations — what a real terminal shows mid-blink.
            // Blink intent stays in canonical state; only the still is
            // sampled. Legacy renderers pin phase On (frozen-visible).
            if cell.mods.hidden || (cell.mods.blink && self.blink_phase == BlinkPhase::Off) {
                return;
            }
            let baseline = cy as i32 + self.set.regular.ascent.round() as i32;
            // Whitespace cells carry no glyph, but real terminals still
            // draw underline/strikethrough across them (the background is
            // already painted above) — decorations are not part of the
            // skipped glyph draw.
            if !cell.symbol.trim().is_empty() {
                draw_symbol(
                    &mut img,
                    &self.set,
                    &mut self.glyphs,
                    &cell.symbol,
                    cx as i32,
                    baseline,
                    span,
                    cy as i32,
                    cell_h,
                    fg,
                    cell.mods.bold,
                    cell.mods.italic,
                    u_i,
                    Some(CellSinks {
                        x,
                        y,
                        missing: &mut missing,
                        fallback: &mut fallback_glyphs,
                    }),
                );
            }
            if cell.mods.underline {
                // All styles render as one rule; only the color varies
                // (SGR 58) — double/curly/dotted/dashed geometry is a
                // known renderer limitation, tracked in canonical state.
                let ul = match cell.underline_color {
                    tuiscotti_core::frame::Color::Default => fg,
                    tuiscotti_core::frame::Color::Indexed(i) => {
                        tuiscotti_core::frame::Rgb::from_indexed(i)
                    }
                    tuiscotti_core::frame::Color::Rgb(r) => r,
                };
                let uy = (baseline + 2 * u_i).min((cy + cell_h - 1) as i32);
                let th = if cell.mods.bold { 2 * u } else { u };
                for t in 0..th {
                    for dx in 0..span {
                        blend(&mut img, cx + dx, (uy + t as i32) as u32, ul, 255);
                    }
                }
            }
            if cell.mods.strikethrough {
                let sy = baseline - (self.set.regular.ascent * 0.35) as i32;
                for t in 0..u {
                    for dx in 0..span {
                        blend(&mut img, cx + dx, (sy + t as i32).max(0) as u32, fg, 255);
                    }
                }
            }
        }
    }

    /// Block cursor: fill cell with fg, redraw glyph in bg (classic
    /// terminal). Underline/bar cursors draw rules instead.
    fn draw_cursor(&mut self, img: &mut image::RgbImage, frame: &Frame) {
        let profile = &self.profile;
        let u = profile.scale;
        let cell_w = profile.cell_w * u;
        let cell_h = profile.cell_h * u;
        let pad = profile.pad * u;
        let u_i = u as i32;
        if !(frame.cursor.visible && profile.cursor_visible) {
            return;
        }
        let mut cx = frame.cursor.x;
        if frame
            .get(cx, frame.cursor.y)
            .is_some_and(|c| c.continuation)
        {
            cx = cx.saturating_sub(1);
        }
        if let Some(cell) = frame.get(cx, frame.cursor.y) {
            let (fg, cbg) = Frame::resolve_cell(cell, profile.default_fg, profile.default_bg);
            let span = u32::from(cell.width.max(1)) * cell_w;
            let px = pad + cx as u32 * cell_w;
            let py = pad + frame.cursor.y as u32 * cell_h;
            let style = frame.cursor.style;
            match style {
                tuiscotti_core::frame::CursorStyle::Block => {
                    fill_rect(img, px, py, span, cell_h, fg);
                    if !cell.mods.hidden && !cell.symbol.trim().is_empty() {
                        let baseline = py as i32 + self.set.regular.ascent.round() as i32;
                        draw_symbol(
                            img,
                            &self.set,
                            &mut self.glyphs,
                            &cell.symbol,
                            px as i32,
                            baseline,
                            span,
                            py as i32,
                            cell_h,
                            cbg,
                            false,
                            false,
                            u_i,
                            None,
                        );
                    }
                }
                tuiscotti_core::frame::CursorStyle::Underline => {
                    let uy = (py + cell_h - 2 * u) as i32;
                    for t in 0..2 * u {
                        for dx in 0..span {
                            blend(img, px + dx, (uy + t as i32) as u32, fg, 255);
                        }
                    }
                }
                tuiscotti_core::frame::CursorStyle::Bar => {
                    for dx in 0..2 * u {
                        for dy in 0..cell_h {
                            blend(img, px + dx, py + dy, fg, 255);
                        }
                    }
                }
            }
        }
    }
}
