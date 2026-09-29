//! One rendered sample: canonical text, PNG, and Insta review settings.

use super::{
    Location, PNG_SNAPSHOT_SUFFIX, description_for, generation_id, png_tag_generation,
    write_evidence_in,
};
use crate::insta_proto::{PngPixelComparator, insta_string};
use std::path::Path;
use tuiscotti_core::frame::Frame;
use tuiscotti_core::screen::Screen;
use tuiscotti_render::diff::AlphaPolicy;
use tuiscotti_render::profile::{Profile, VENDORED_FACES};
use tuiscotti_render::render::Renderer;

/// Deterministic [`Frame`] from a [`Screen`] for rendering evidence.
///
/// Cells and cursor are preserved exactly; provenance is fixed with
/// `created_unix = 0` so every derived artifact is byte-deterministic for
/// identical screens (the timestamp is informational and excluded from gates).
///
/// Delegates to the canonical screen→frame adaptation in
/// `tuiscotti_render` with the assert profile override (`"tuiscotti-default"`)
/// and the assert source (`"tuiscotti-assert"`, naming the asserting tool for
/// provenance audits — the render pipeline itself passes `"screen"`).
#[must_use]
pub fn frame_from_screen(screen: &Screen) -> Frame {
    tuiscotti_render::render::frame::frame_from_screen_with_source(
        screen,
        "tuiscotti-default",
        "tuiscotti-assert",
    )
}

/// One visual sample: canonical state plus all four rendered artifacts.
#[derive(Debug, Clone)]
pub struct Sample {
    /// Styled canonical state ([`crate::insta_proto::insta_string`]).
    pub canonical: String,
    /// Normalized SGR dump.
    pub ansi: String,
    /// Plain text.
    pub txt: String,
    /// Standalone HTML render (embeds the PNG + frame JSON).
    pub html: String,
    /// Authoritative PNG (untagged; see [`png_tag_generation`]).
    pub png: Vec<u8>,
}

/// Render/IO failure from sample rendering, evidence writing, or export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertError {
    /// The pinned renderer refused the frame.
    Render(String),
    /// Filesystem failure (path context included).
    Io(String),
}

impl std::fmt::Display for AssertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssertError::Render(e) => write!(f, "render error: {e}"),
            AssertError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for AssertError {}

impl From<tuiscotti_render::render::RenderError> for AssertError {
    fn from(e: tuiscotti_render::render::RenderError) -> Self {
        AssertError::Render(e.to_string())
    }
}

/// Render one sample from a screen: canonical projection plus all four artifacts
/// from a single [`Renderer`] pass over the default profile and vendored faces.
///
/// # Errors
///
/// Returns [`AssertError::Render`] when the pinned renderer refuses the frame.
pub fn render_sample(screen: &Screen) -> Result<Sample, AssertError> {
    let frame = frame_from_screen(screen);
    let profile = Profile::default_profile();
    let mut renderer = Renderer::new(&profile, &VENDORED_FACES)?;
    let artifacts = renderer.render_artifacts(&frame, "tuiscotti")?;
    Ok(Sample {
        canonical: insta_string(screen),
        ansi: artifacts.ansi,
        txt: artifacts.txt,
        html: artifacts.html,
        png: artifacts.png,
    })
}

/// Comparator helper for PNG snapshots: decoded-pixel equality under an explicit
/// [`AlphaPolicy`] (text snapshots keep stock Insta semantics).
#[must_use]
pub fn png_comparator(alpha: AlphaPolicy) -> PngPixelComparator {
    PngPixelComparator::new(alpha)
}

/// Scoped evolving settings for the facade macros (macro backend).
///
/// Derives from [`insta::Settings::clone_current`] so an outer
/// `snapshot_suffix` (parameterized tests) is honored, then overrides the
/// snapshot path, module-prepend, and the generation/render description. The
/// macros `bind` these around the caller-expanded Insta assertion, so nothing
/// leaks outward.
#[doc(hidden)]
#[must_use]
pub fn snapshot_settings(
    snapshot_dir: &Path,
    location: Location,
    generation: &str,
) -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(snapshot_dir);
    settings.set_prepend_module_to_snapshot(false);
    settings.set_description(description_for(generation, location));
    settings
}

/// Canonical text plus its content-derived generation (macro backend for
/// [`crate::assert_snapshot!`]).
#[doc(hidden)]
#[must_use]
pub fn prepare_snapshot(screen: &Screen) -> (String, String) {
    let canonical = insta_string(screen);
    let generation = generation_id(&canonical);
    (canonical, generation)
}

/// One prepared screenshot sample: canonical state plus the generation-tagged
/// PNG, with candidate evidence already on disk (macro backend).
#[doc(hidden)]
#[derive(Debug)]
pub struct PreparedScreenshot {
    /// Styled canonical state.
    pub canonical: String,
    /// Content-derived generation binding both artifacts.
    pub generation: String,
    /// Generation-tagged PNG bytes.
    pub png: Vec<u8>,
}

/// Render one sample and write candidate evidence BEFORE any failure (macro
/// backend for [`crate::assert_screenshot!`]). Panics with context when rendering or
/// evidence writing fails.
#[doc(hidden)]
#[must_use]
#[expect(
    clippy::panic,
    reason = "assert-macro backend panics by contract, like std assert"
)]
pub fn prepare_screenshot(name: &str, screen: &Screen, evidence_dir: &Path) -> PreparedScreenshot {
    let sample = render_sample(screen).unwrap_or_else(|e| {
        panic!("tuiscotti assert_screenshot!({name:?}): cannot render sample: {e}")
    });
    let generation = generation_id(&sample.canonical);
    let png = png_tag_generation(&sample.png, &generation);
    write_evidence_in(evidence_dir, name, &sample, &png).unwrap_or_else(|e| {
        panic!("tuiscotti assert_screenshot!({name:?}): cannot write evidence: {e}")
    });
    PreparedScreenshot {
        canonical: sample.canonical,
        generation,
        png,
    }
}

/// PNG snapshot base for a screenshot name: `<name>-img` (macro backend).
#[doc(hidden)]
#[must_use]
pub fn png_snapshot_base(name: &str) -> String {
    format!("{name}{PNG_SNAPSHOT_SUFFIX}")
}

/// Decoded-pixel PNG comparator under the screenshot alpha policy
/// ([`AlphaPolicy::StraightRgba`]) (macro backend).
#[doc(hidden)]
#[must_use]
pub fn screenshot_png_comparator() -> PngPixelComparator {
    png_comparator(AlphaPolicy::StraightRgba)
}
