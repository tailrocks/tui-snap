//! TXT: plain Unicode text projection of canonical state.
//!
//! [`txt_projection`] renders a [`Frame`] as plain Unicode text: no escapes,
//! no color, no metadata. Explicit whitespace policy:
//!
//! - interior whitespace (including styled spaces) is preserved verbatim;
//! - trailing blanks are trimmed per row;
//! - rows join with `\n`; there is no trailing newline;
//! - continuation cells contribute nothing (the lead holds the symbol);
//! - concealed (`hidden`) cells contribute their source symbol — concealment
//!   is not redaction, exactly as in canonical JSON.
//!
//! TXT is the human-readable equality surface for *content*. Style-only
//! changes (bold, color, underline) do not move TXT bytes; use ANSI or
//! canonical JSON to gate those.
//!
//! [`Frame`]: tuiscotti_core::frame::Frame

use tuiscotti_core::frame::Frame;

/// Render `frame` as plain Unicode text under the documented policy.
#[must_use]
pub fn txt_projection(frame: &Frame) -> String {
    frame.text()
}

/// Render `frame` as one [`String`] per grid row (trailing blanks trimmed).
#[must_use]
pub fn txt_lines(frame: &Frame) -> Vec<String> {
    let text = txt_projection(frame);
    if frame.rows == 0 {
        return Vec::new();
    }
    text.split('\n').map(str::to_string).collect()
}

/// Fail when `text` carries escape bytes or control characters (other than
/// the `\n` row joins): TXT must stay plain.
///
/// # Errors
///
/// Returns `FormatError` naming the first control character found.
pub fn assert_no_escapes(text: &str) -> Result<(), crate::formats::FormatError> {
    for (i, c) in text.char_indices() {
        if c == '\n' {
            continue;
        }
        if c == '\u{1b}' || c.is_control() {
            return Err(crate::formats::FormatError(format!(
                "TXT carries control character U+{:04X} at offset {i}",
                c as u32
            )));
        }
    }
    Ok(())
}
