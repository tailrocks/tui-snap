//! Reusable [`Renderer`]: pinned faces, cached rasters, PNG/HTML.

use super::{
    Artifacts, FallbackGlyph, Fidelity, FontSet, GlyphCache, MissingGlyph, RenderError, Rendered,
    ansi_dump, frame_from_screen, html_document, load_font, verify_geometry,
};
use crate::profile::{BlinkPhase, FontFaces, MissingGlyphPolicy, Profile, RenderProfile};
use std::cell::RefCell;
use std::collections::HashMap;
use tuiscotti_core::{frame::Frame, screen::Screen};

mod cell;
mod strict;

// Thread-local shared renderers (F12): one `Renderer` per profile key,
// constructed once per thread, glyph caches shared across all renders on
// that thread. Threads never share instances (no lock contention, no
// `Sync` requirement); the map holds at most one entry per distinct
// profile a thread renders with.
thread_local! {
    static SHARED: RefCell<HashMap<String, Renderer>> = RefCell::new(HashMap::new());
}

/// Key for the legacy default gate (default profile + vendored faces +
/// default fallback chain).
const DEFAULT_KEY: &str = "legacy:tuiscotti-default";

/// True only for the exact vendored byte slices (identity, not content:
/// same static, same pointer). A caller passing different bytes with
/// identical content renders through a fresh instance — the safe
/// direction, since pins are verified at construction either way.
fn faces_are_vendored(faces: &FontFaces<'_>) -> bool {
    use crate::profile::{
        VENDORED_FONT, VENDORED_FONT_BOLD, VENDORED_FONT_BOLD_ITALIC, VENDORED_FONT_ITALIC,
    };
    fn same(a: &[u8], b: &[u8]) -> bool {
        a.len() == b.len() && a.as_ptr() == b.as_ptr()
    }
    same(faces.regular, VENDORED_FONT)
        && same(faces.bold, VENDORED_FONT_BOLD)
        && same(faces.italic, VENDORED_FONT_ITALIC)
        && same(faces.bold_italic, VENDORED_FONT_BOLD_ITALIC)
}

/// A reusable renderer: parses the pinned faces ONCE at construction (the
/// geometry pin is verified there too) and caches glyph rasters across
/// frames, so a bulk gate costs O(distinct glyphs) rasterizations instead of
/// 8 font parses plus a full re-rasterization per frame.
///
/// Threading: every method takes `&mut self`, so the borrow checker enforces
/// exclusive use — give each parallel test thread its own instance
/// (`thread_local!` is the convenient carrier), matching the per-thread
/// session confinement of the PTY layer.
#[derive(Debug)]
pub struct Renderer {
    profile: Profile,
    set: FontSet,
    glyphs: GlyphCache,
    /// V05 strict missing-glyph policy: fail instead of tofu. Legacy constructors leave
    /// this false (placeholder + fidelity record); [`Self::for_render_profile`] sets it.
    strict_missing: bool,
    /// V07 still sample phase for `mods.blink` cells (legacy: [`BlinkPhase::On`], frozen-visible).
    blink_phase: BlinkPhase,
}

impl Renderer {
    /// Load the faces and verify the geometry pin (once for all renders).
    /// The per-glyph fallback chain is the vendored default
    /// ([`crate::profile::VENDORED_FALLBACK_FACES`]); use
    /// [`Self::with_fallbacks`] to replace it.
    /// # Errors
    /// Returns `RenderError` when a face fails to load or the geometry pin breaks.
    pub fn new(profile: &Profile, faces: &FontFaces<'_>) -> Result<Self, RenderError> {
        Self::with_fallbacks(profile, faces, crate::profile::VENDORED_FALLBACK_FACES)
    }

    /// Like [`Self::new`] but with an explicit per-glyph fallback chain,
    /// tried in order after the primary family (pass `&[]` for
    /// primary-family-only rendering). Each face's bytes are verified
    /// against its pinned SHA-256 before parsing; a mismatch refuses to
    /// render. Primary geometry stays pinned to `faces.regular` regardless —
    /// fallback faces only fill coverage holes inside the pinned cell box.
    /// # Errors
    /// Returns `RenderError` on face load/verify failure or a broken geometry pin.
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
        let scale_f = f32::from(u16::try_from(profile.scale).unwrap_or(u16::MAX));
        let set = FontSet::load_with_fallbacks(faces, profile.font_px * scale_f, fallbacks)?;
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
    /// # Errors
    /// Returns `RenderError` when a face pin mismatches at render time.
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

    /// Run `f` against a renderer for `profile` + `faces`, reusing the
    /// thread-local shared instance when the request is exactly the
    /// default gate ([`Profile::is_default_gate`] plus the vendored face
    /// bytes, with the default fallback chain [`Self::new`] loads).
    /// Anything else renders through a fresh instance, so custom
    /// profiles, face overrides, and geometry pins behave exactly as
    /// before — only slower (one construction per call).
    ///
    /// The closure must not reenter `with_profile`/`with_strict` on the
    /// same thread (reentry fails closed with `RenderError`, never
    /// panics). `E` only needs `From<RenderError>` so construction
    /// failures convert into the caller's error type.
    ///
    /// # Errors
    ///
    /// Returns `E` when the renderer cannot be built or `f` fails.
    pub fn with_profile<T, E>(
        profile: &Profile,
        faces: &FontFaces<'_>,
        f: impl FnOnce(&mut Renderer) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<RenderError>,
    {
        if profile.is_default_gate() && faces_are_vendored(faces) {
            Self::with_shared(DEFAULT_KEY, Self::build_default, f)
        } else {
            f(&mut Self::new(profile, faces)?)
        }
    }

    /// Run `f` against a renderer for the strict profile `rp`, reusing
    /// the thread-local shared instance keyed by [`RenderProfile::hash`].
    /// Sound: the hash covers every pin, and the strict constructor
    /// verifies face bytes against those pins (fields are `pub(crate)`,
    /// so no unverified profile value can exist) — a key hit is
    /// byte-identical faces under byte-identical parameters. Same
    /// reentry rule as [`Self::with_profile`].
    ///
    /// # Errors
    ///
    /// Returns `E` when the renderer cannot be built or `f` fails.
    pub fn with_strict<T, E>(
        rp: &RenderProfile<'_>,
        f: impl FnOnce(&mut Renderer) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<RenderError>,
    {
        let key = format!("strict:{}", rp.hash());
        Self::with_shared(&key, || Self::for_render_profile(rp), f)
    }

    /// The shared default-gate renderer constructor (faces parsed + pins
    /// verified once per thread).
    fn build_default() -> Result<Self, RenderError> {
        Self::new(&Profile::default_profile(), &crate::profile::VENDORED_FACES)
    }

    /// Run `f` against the shared renderer for `key`, constructing it on
    /// first use. Reentrant or post-teardown access fails closed.
    fn with_shared<T, E>(
        key: &str,
        make: impl FnOnce() -> Result<Self, RenderError>,
        f: impl FnOnce(&mut Renderer) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<RenderError>,
    {
        SHARED
            .try_with(|cell| {
                let mut map = cell
                    .try_borrow_mut()
                    .map_err(|_| RenderError("reentrant shared-renderer use".to_string()))?;
                let renderer = match map.entry(key.to_string()) {
                    std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                    std::collections::hash_map::Entry::Vacant(e) => e.insert(make()?),
                };
                f(renderer)
            })
            .map_err(|_| RenderError("shared renderer unavailable".to_string()))?
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

    fn geom(&self) -> (u32, u32, u32, u32) {
        let p = &self.profile;
        let u = p.scale;
        (u, p.cell_w * u, p.cell_h * u, p.pad * u)
    }

    /// Render a validated frame to PNG bytes.
    /// # Errors
    /// Returns `RenderError` when the frame is invalid or PNG encoding fails.
    pub fn render_png(&mut self, frame: &Frame) -> Result<Vec<u8>, RenderError> {
        Ok(self.render(frame)?.png)
    }

    /// Render a validated [`Screen`] (V01): the screen is adapted to the
    /// render input losslessly ([`frame_from_screen`]) and run through the
    /// SAME engine as [`Self::render`] — one code path for saved, direct,
    /// and live screens. The screen origin is positional metadata and does
    /// not affect pixels; grid, cursor, colors, and modifiers do.
    /// # Errors
    /// Returns `RenderError` when the screen is invalid or PNG encoding fails.
    pub fn render_screen(&mut self, screen: &Screen) -> Result<Rendered, RenderError> {
        let frame = frame_from_screen(screen, &self.profile.name);
        self.render(&frame)
    }

    /// [`Self::render_screen`] returning PNG bytes only.
    /// # Errors
    /// Returns `RenderError` when the screen is invalid or PNG encoding fails.
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
    /// # Errors
    /// Returns `RenderError` when the frame is invalid or PNG encoding fails.
    pub fn render_html(&mut self, frame: &Frame, title: &str) -> Result<String, RenderError> {
        let rendered = self.render(frame)?;
        Ok(html_document(frame, &self.profile, title, &rendered.png))
    }

    /// Generate all four snapshot artifacts in one render pass (the PNG is
    /// rasterized once and shared by the HTML embed and the PNG artifact).
    /// # Errors
    /// Returns `RenderError` when the frame is invalid or PNG encoding fails.
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
    /// # Errors
    /// Returns `RenderError` when the frame is invalid or PNG encoding fails.
    pub fn render(&mut self, frame: &Frame) -> Result<Rendered, RenderError> {
        frame
            .validate()
            .map_err(|e| RenderError(format!("refusing to render: {e}")))?;
        let (u, cell_w, cell_h, pad) = self.geom();

        let w = u32::from(frame.cols) * cell_w + pad * 2;
        let h = u32::from(frame.rows) * cell_h + pad * 2;
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
        let fell_back: Vec<String> = self.set.fell_back.iter().map(ToString::to_string).collect();
        let fidelity = Fidelity {
            profile: self.profile.name.clone(),
            font_sha256: self.profile.font_sha256.clone(),
            font_desc: self.profile.font_desc.clone(),
            scale: u,
            approximate: !missing.is_empty() || !self.set.fell_back.is_empty(),
            faces_fell_back: fell_back,
            missing,
            fallback_glyphs,
        };
        Ok(Rendered { png: out, fidelity })
    }
}
