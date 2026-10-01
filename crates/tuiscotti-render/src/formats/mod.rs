//! G5 six-format contracts: one capture, six distinct projections.
//!
//! Every screen capture exports all six formats plus one identifiable
//! [`Generation`]:
//!
//! | Format | Module | Equality role |
//! |---|---|---|
//! | ASCII | [`ascii`] | 7-bit diagnostic only — lossy, never an equality input |
//! | TXT | [`txt`] | plain-Unicode content equality |
//! | ANSI | [`ansi`] | normalized SGR: content + style equality |
//! | PNG | [`png`] | independent opaque-RGB pixel evidence |
//! | HTML | [`html`] | escaped static offline evidence, no JavaScript |
//! | Canonical JSON | [`json`] | versioned state + provenance: the authority |
//!
//! ASCII is not ANSI: both are preserved side by side and neither is ever
//! silently interpreted as the other. Piped byte output (not screen state)
//! projects through [`pipe`] with its own loss accounting.
//!
//! [`capture_all`] exports every format from one frame in one render pass
//! (the PNG rasterizes once and the HTML embeds those exact bytes), so a
//! bundle can never mix generations internally. [`generations_match`] /
//! [`require_same_generation`] detect bundles from different captures
//! mixed downstream.

pub mod ansi;
pub mod ascii;
pub mod html;
pub mod json;
pub mod pipe;
pub mod png;
pub mod txt;

pub use ansi::{ansi_normalized, assert_normalized_sgr};
pub use ascii::{AsciiArtifact, AsciiSubstitution, ascii_projection, assert_seven_bit};
pub use html::{assert_static_offline, html_static};
pub use json::{CANONICAL_JSON_VERSION, canonical_json, parse_canonical};
pub use pipe::{PipeArtifact, PipeError, pipe_projection, pipe_strict};
pub use png::{PngInfo, assert_opaque_rgb, changed_pixels};
pub use txt::{assert_no_escapes, txt_projection};

use crate::profile::RENDERER_VERSION;
use crate::render::{RenderError, Renderer};
use sha2::{Digest, Sha256};
use tuiscotti_core::frame::Frame;

/// Format-contract failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError(pub String);

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "format error: {}", self.0)
    }
}

impl std::error::Error for FormatError {}

/// Format-contract family version. Bumps when any projection policy changes.
pub const FORMATS_VERSION: &str = "g5.1";

/// One identifiable capture generation: the frame digest plus the exact
/// renderer/profile/format versions that produced it. Two bundles share a
/// generation only when all of these agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    /// Hex SHA-256 over formats version + renderer version + profile name
    /// + canonical frame bytes. Stable and deterministic.
    pub id: String,
    /// [`Frame::digest`] of the captured frame (provenance excluded).
    pub frame_digest: u64,
    /// [`RENDERER_VERSION`] of the producing renderer.
    pub renderer_version: u32,
    /// Profile name the pixels were rendered under.
    pub profile: String,
}

impl Generation {
    /// Corpus key for logs and approvals (`<id16> <digest16> <profile>`).
    #[must_use]
    pub fn key(&self) -> String {
        format!(
            "{} {:016x} {}",
            self.id.chars().take(16).collect::<String>(),
            self.frame_digest,
            self.profile
        )
    }
}

/// Identify the generation of `frame` rendered under `profile_name`.
#[must_use]
pub fn generation_for(frame: &Frame, profile_name: &str) -> Generation {
    let mut hasher = Sha256::new();
    hasher.update(FORMATS_VERSION.as_bytes());
    hasher.update(RENDERER_VERSION.to_le_bytes());
    hasher.update(profile_name.as_bytes());
    hasher.update(frame.to_json().as_bytes());
    let digest = hasher.finalize();
    Generation {
        id: crate::hex_bytes(&digest),
        frame_digest: frame.digest(),
        renderer_version: RENDERER_VERSION,
        profile: profile_name.to_string(),
    }
}

/// True only when both generations are the same capture lineage.
#[must_use]
pub fn generations_match(a: &Generation, b: &Generation) -> bool {
    a == b
}

/// Fail when `a` and `b` are not the same generation — the mixed-generation
/// guard for downstream artifact assembly.
///
/// # Errors
///
/// Returns `FormatError` when the generations differ.
pub fn require_same_generation(a: &Generation, b: &Generation) -> Result<(), FormatError> {
    if generations_match(a, b) {
        Ok(())
    } else {
        Err(FormatError(format!(
            "mixed generations: {} vs {}",
            a.key(),
            b.key()
        )))
    }
}

/// All six formats of one capture, sharing one [`Generation`].
#[derive(Debug, Clone)]
pub struct CaptureBundle {
    /// The single generation every artifact below belongs to.
    pub generation: Generation,
    /// 7-bit diagnostic projection (loss-accounted).
    pub ascii: AsciiArtifact,
    /// Plain Unicode text.
    pub txt: String,
    /// Normalized SGR screen output.
    pub ansi: String,
    /// Opaque RGB pixel evidence.
    pub png: Vec<u8>,
    /// Static offline HTML embedding [`CaptureBundle::png`].
    pub html: String,
    /// Compact canonical JSON.
    pub json: String,
}

/// Export every format of `frame` in one render pass: the PNG rasterizes
/// once, the HTML embeds those exact bytes, and all six artifacts share one
/// [`Generation`]. The frame is validated before anything renders.
///
/// # Errors
///
/// Returns `RenderError` when the frame is invalid or the render fails.
pub fn capture_all(
    renderer: &mut Renderer,
    frame: &Frame,
    title: &str,
) -> Result<CaptureBundle, RenderError> {
    frame
        .validate()
        .map_err(|e| RenderError(format!("refusing to capture: {e}")))?;
    let profile_name = renderer.profile().name.clone();
    let still = renderer.render(frame)?;
    let generation = generation_for(frame, &profile_name);
    let profile = renderer.profile().clone();
    Ok(CaptureBundle {
        generation: generation.clone(),
        ascii: ascii_projection(frame),
        txt: txt_projection(frame),
        ansi: ansi_normalized(frame),
        png: still.png.clone(),
        html: html_static(frame, &profile, title, Some(&still.png), &generation.id),
        json: frame.to_json(),
    })
}
