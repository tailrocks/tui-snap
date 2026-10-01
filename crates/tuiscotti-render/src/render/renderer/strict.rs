//! V05 strict missing-glyph policy check, split from [`super::Renderer`].

use crate::render::{MissingGlyph, RenderError};

/// V05 strict missing-glyph policy: fail with the exact uncovered
/// set instead of returning tofu. Placeholder mode (legacy behavior)
/// returns the tofu PNG plus the fidelity record.
pub(super) fn check_strict_missing(
    strict_missing: bool,
    missing: &[MissingGlyph],
) -> Result<(), RenderError> {
    if !strict_missing || missing.is_empty() {
        return Ok(());
    }
    let mut detail: Vec<String> = missing
        .iter()
        .map(|m| {
            format!(
                "({},{}) {:?} [{}]",
                m.x,
                m.y,
                m.symbol,
                m.codepoints.join(",")
            )
        })
        .collect();
    detail.sort();
    Err(RenderError(format!(
        "strict missing-glyph policy: {} uncovered cell(s): {}",
        missing.len(),
        detail.join("; ")
    )))
}
