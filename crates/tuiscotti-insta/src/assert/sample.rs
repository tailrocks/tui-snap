//! One rendered sample: canonical text, PNG, and Insta review settings.

use super::{
    AttemptIdentity, BundlePayload, EvidenceId, Location, PNG_SNAPSHOT_SUFFIX, SnapshotIdentity,
    active_snapshot_suffix, current_test_name, description_for, generation_id, png_tag_generation,
    render_identity, sample_binding, write_bundle_in,
};
use crate::insta_proto::PngPixelComparator;
use std::path::{Path, PathBuf};
use tuiscotti_core::frame::Frame;
use tuiscotti_core::screen::Screen;
use tuiscotti_core::screen::canonical_string;
use tuiscotti_render::diff::AlphaPolicy;
use tuiscotti_render::profile::{
    FontFaces, Profile, VENDORED_FACES, VENDORED_FONT_BOLD_ITALIC_SHA256,
    VENDORED_FONT_BOLD_SHA256, VENDORED_FONT_ITALIC_SHA256, VENDORED_FONT_SHA256, font_sha256,
};
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
    /// Styled canonical state ([`tuiscotti_core::screen::canonical_string`]).
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
/// from a single [`Renderer`] pass over the default profile and vendored faces
/// (through the thread-local shared instance: faces parsed once per thread,
/// glyph cache shared across samples). The primary faces are hash-verified
/// against the vendored pins before rendering (the renderer re-verifies the
/// fallback chain at load); a swapped primary face refuses loudly instead of
/// shifting pixels.
///
/// # Errors
///
/// Returns [`AssertError::Render`] when a face pin mismatches or the pinned
/// renderer refuses the frame.
pub fn render_sample(screen: &Screen) -> Result<Sample, AssertError> {
    render_sample_with_faces(screen, &VENDORED_FACES)
}

/// [`render_sample`] over explicit primary faces: every face is verified
/// against its vendored SHA-256 pin first, so only the pinned family renders
/// on the Insta path.
///
/// # Errors
///
/// Returns [`AssertError::Render`] when a face pin mismatches or the pinned
/// renderer refuses the frame.
pub fn render_sample_with_faces(
    screen: &Screen,
    faces: &FontFaces<'_>,
) -> Result<Sample, AssertError> {
    verify_primary_pins(faces)?;
    let frame = frame_from_screen(screen);
    let profile = Profile::default_profile();
    let artifacts =
        Renderer::with_profile(&profile, faces, |r| r.render_artifacts(&frame, "tuiscotti"))?;
    Ok(Sample {
        canonical: canonical_string(screen),
        ansi: artifacts.ansi,
        txt: artifacts.txt,
        html: artifacts.html,
        png: artifacts.png,
    })
}

/// Verify the four primary faces against the vendored pins (regular, bold,
/// italic, bold-italic): a pin mismatch refuses to render, never silently
/// substitutes.
fn verify_primary_pins(faces: &FontFaces<'_>) -> Result<(), AssertError> {
    let slots = [
        ("regular", faces.regular, VENDORED_FONT_SHA256),
        ("bold", faces.bold, VENDORED_FONT_BOLD_SHA256),
        ("italic", faces.italic, VENDORED_FONT_ITALIC_SHA256),
        (
            "bold-italic",
            faces.bold_italic,
            VENDORED_FONT_BOLD_ITALIC_SHA256,
        ),
    ];
    for (label, bytes, pin) in slots {
        let actual = font_sha256(bytes);
        if actual != pin {
            return Err(AssertError::Render(format!(
                "{label} face sha256 mismatch: pinned {pin}, got {actual} — refusing to render"
            )));
        }
    }
    Ok(())
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
    let canonical = canonical_string(screen);
    let generation = generation_id(&canonical);
    (canonical, generation)
}

/// One prepared screenshot sample: canonical state plus the binding-tagged
/// PNG, with the full candidate bundle already on disk (macro backend).
#[doc(hidden)]
#[derive(Debug)]
pub struct PreparedScreenshot {
    /// Styled canonical state.
    pub canonical: String,
    /// Compound sample binding ([`sample_binding`]) carried by both
    /// artifacts' descriptions and the PNG `tEXt` chunk.
    pub binding: String,
    /// Binding-tagged PNG bytes.
    pub png: Vec<u8>,
    /// Published candidate-bundle directory (for failure diagnostics).
    pub bundle_dir: PathBuf,
}

/// Render one sample and publish the full candidate bundle BEFORE any
/// failure (macro backend for [`crate::assert_screenshot!`]). `package` is
/// the caller's `env!("CARGO_PKG_NAME")`. Panics with context when
/// rendering or bundle publication fails.
#[doc(hidden)]
#[must_use]
#[expect(
    clippy::panic,
    reason = "assert-macro backend panics by contract, like std assert"
)]
pub fn prepare_screenshot(
    name: &str,
    screen: &Screen,
    evidence_root: &Path,
    package: &str,
    identity: &SnapshotIdentity,
) -> PreparedScreenshot {
    let sample = render_sample(screen).unwrap_or_else(|e| {
        panic!("tuiscotti assert_screenshot!({name:?}): cannot render sample: {e}")
    });
    let render = render_identity();
    let binding = sample_binding(&sample.canonical, &render, &sample.png);
    let generation = generation_id(&sample.canonical);
    let png = png_tag_generation(&sample.png, &binding);
    let id = EvidenceId {
        package: package.to_string(),
        test: current_test_name(),
        scenario: name.to_string(),
        variant: active_snapshot_suffix(),
        attempt: AttemptIdentity::from_env(),
    };
    let payload = BundlePayload {
        sample: &sample,
        png_tagged: &png,
        binding: &binding,
        generation: &generation,
        render_identity: &render,
    };
    let bundle_dir = write_bundle_in(evidence_root, &id, identity, &payload).unwrap_or_else(|e| {
        panic!("tuiscotti assert_screenshot!({name:?}): cannot write evidence: {e}")
    });
    PreparedScreenshot {
        canonical: sample.canonical,
        binding,
        png,
        bundle_dir,
    }
}

/// Aggregate the compound assertion outcome into one failure message
/// (macro backend): both Insta assertions already ran (a canonical failure
/// never suppresses the PNG pending), and `gate` is the strict consistency
/// verdict over the resolved identity. Returns `None` when everything
/// passed, else the combined message naming the candidate bundle.
#[doc(hidden)]
#[must_use]
pub fn aggregate_compound_result(
    name: &str,
    canonical: &std::thread::Result<()>,
    png: &std::thread::Result<()>,
    gate: &Result<(), super::ConsistencyError>,
    bundle_dir: &Path,
) -> Option<String> {
    let mut failures = Vec::new();
    if let Err(payload) = canonical {
        failures.push(format!(
            "canonical snapshot failed: {}",
            panic_message(payload)
        ));
    }
    if let Err(payload) = png {
        failures.push(format!("png snapshot failed: {}", panic_message(payload)));
    }
    if let Err(e) = gate {
        failures.push(format!("compound gate failed: {e}"));
    }
    if failures.is_empty() {
        None
    } else {
        Some(format!(
            "tuiscotti assert_screenshot!({name:?}): {}\n(candidate bundle: {})",
            failures.join("; "),
            bundle_dir.display()
        ))
    }
}

/// Best-effort rendering of a caught panic payload.
#[must_use]
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "<non-string panic>".to_string()
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
