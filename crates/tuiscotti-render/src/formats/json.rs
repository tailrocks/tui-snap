//! Canonical JSON: complete versioned state and provenance.
//!
//! [`canonical_json`] serializes a [`Frame`] in its compact canonical form:
//! every cell (symbols, widths, continuations, colors, modifiers,
//! underline colors), cursor intent, and full provenance. The `version`
//! field pins the schema ([`CANONICAL_JSON_VERSION`]); imports reject any
//! other version explicitly via [`Frame::from_json`].
//!
//! Canonical JSON is the equality authority for state: it round-trips
//! losslessly ([`parse_canonical`]), carries concealed symbols (concealment
//! is not redaction), and excludes nothing gate-relevant. `provenance`
//! timestamps stay informational — [`Frame::digest`] and cell comparison
//! ignore them — but the provenance identity fields (tool, version,
//! profile, source) must be present ([`assert_provenance_complete`]).
//!
//! [`Frame`]: tuiscotti_core::frame::Frame

use tuiscotti_core::frame::{FRAME_VERSION, Frame, FrameError};

/// Canonical JSON schema version. Bumps only with [`FRAME_VERSION`].
pub const CANONICAL_JSON_VERSION: u8 = FRAME_VERSION;

/// Serialize `frame` as compact canonical JSON. The frame is validated
/// first; malformed frames are rejected, never serialized.
pub fn canonical_json(frame: &Frame) -> Result<String, FrameError> {
    frame.validate()?;
    Ok(frame.to_json())
}

/// Parse + validate canonical JSON (pretty or compact: whitespace is not
/// significant). Legacy bool-only underlines normalize to `Single`.
pub fn parse_canonical(text: &str) -> Result<Frame, FrameError> {
    Frame::from_json(text)
}

/// Fail unless the provenance identity fields that make a capture auditable
/// are all present. `created_unix` and `argv` may legitimately be
/// zero/empty (deterministic renders, redacted evidence).
pub fn assert_provenance_complete(frame: &Frame) -> Result<(), FrameError> {
    let p = &frame.provenance;
    for (field, value) in [
        ("tool", p.tool.as_str()),
        ("tool_version", p.tool_version.as_str()),
        ("profile", p.profile.as_str()),
        ("source", p.source.as_str()),
    ] {
        if value.is_empty() {
            return Err(FrameError(format!(
                "provenance.{field} is empty: capture is not auditable"
            )));
        }
    }
    Ok(())
}
