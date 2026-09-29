//! ANSI: normalized VT/SGR screen output regenerated from canonical state.
//!
//! [`ansi_normalized`] is a *normalized projection*, not the original raw
//! transcript: SGR runs are recomputed from canonical cells in one canonical
//! parameter order (modifiers, then fg, then bg, then underline color), a
//! full reset (`ESC[0m`) precedes every style change, and every row ends with
//! a reset plus `\n`. Two captures of the same canonical screen produce
//! byte-identical ANSI regardless of which byte sequences the application
//! originally emitted.
//!
//! This format gates style: ANSI-only changes (same text, new colors or
//! modifiers) move ANSI bytes while TXT stays still.

use tuiscotti_core::frame::Frame;

/// Render `frame` as normalized ANSI (see [`crate::render::ansi_dump`]).
#[must_use]
pub fn ansi_normalized(frame: &Frame) -> String {
    crate::render::ansi_dump(frame)
}

/// Fail unless `text` contains only normalized output: printable text, `\n`
/// row joins, `ESC[0m` resets, and `ESC[<params>m` SGR runs with canonical
/// numeric/`;`/`:` parameters. Any other escape sequence (OSC, cursor moves,
/// mode sets — raw-transcript residue) is rejected.
///
/// # Errors
///
/// Returns `FormatError` naming the first non-normalized sequence found.
pub fn assert_normalized_sgr(text: &str) -> Result<(), crate::formats::FormatError> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            i = check_escape(bytes, i)?;
        } else if b < 0x20 && b != b'\n' {
            return Err(crate::formats::FormatError(format!(
                "ANSI carries raw control byte 0x{b:02x} at offset {i}"
            )));
        } else {
            i += 1;
        }
    }
    Ok(())
}

/// Validate one escape sequence at `bytes[i] == ESC`; return the offset past it.
fn check_escape(bytes: &[u8], i: usize) -> Result<usize, crate::formats::FormatError> {
    let rest = &bytes[i..];
    if rest.len() < 3 || rest[1] != b'[' {
        return Err(crate::formats::FormatError(format!(
            "ANSI carries non-CSI escape at offset {i}"
        )));
    }
    let mut j = i + 2;
    while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';' || bytes[j] == b':') {
        j += 1;
    }
    if j >= bytes.len() || bytes[j] != b'm' {
        return Err(crate::formats::FormatError(format!(
            "ANSI carries non-SGR escape at offset {i}"
        )));
    }
    Ok(j + 1)
}
