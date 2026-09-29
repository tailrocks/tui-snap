//! Piped-output projections: raw child bytes → accountable text.
//!
//! Piped captures (`--print` / `--emit-raw` fixture runs, CLI stdout) arrive
//! as raw bytes that may be truncated, may contain invalid UTF-8, and are
//! never canonical state. [`pipe_projection`] decodes them with explicit
//! accounting instead of silent loss:
//!
//! - every maximal invalid byte sequence becomes one U+FFFD and increments
//!   [`PipeArtifact::replacements`] (never silently dropped, never an error
//!   by itself);
//! - input longer than `max_bytes` is cut at a character boundary and sets
//!   [`PipeArtifact::truncated`] (never a silent cut, never a split scalar);
//! - [`PipeArtifact::id`] identifies the capture: SHA-256 over the raw input
//!   bytes, so pipe captures are identifiable generations too.
//!
//! [`pipe_strict`] is the companion that rejects invalid UTF-8 outright
//! with the offending byte offset. Pipe text is diagnostic content: it never
//! stands in for canonical equality.

use sha2::{Digest, Sha256};

/// Piped-output decode failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeError {
    /// Byte offset of the problem (`None` for whole-input rejections).
    pub offset: Option<usize>,
    /// Human-readable cause.
    pub message: String,
}

impl std::fmt::Display for PipeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.offset {
            Some(o) => write!(f, "pipe decode error at byte {o}: {}", self.message),
            None => write!(f, "pipe decode error: {}", self.message),
        }
    }
}

impl std::error::Error for PipeError {}

/// One decoded pipe capture with its exact loss accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeArtifact {
    /// Identifiable generation: hex SHA-256 over the raw input bytes.
    pub id: String,
    /// Decoded text (U+FFFD per invalid sequence, cut at a char boundary).
    pub text: String,
    /// Raw input length in bytes.
    pub input_bytes: usize,
    /// Raw bytes actually decoded (before the truncation cut).
    pub kept_bytes: usize,
    /// Invalid sequences replaced with U+FFFD.
    pub replacements: usize,
    /// True when `max_bytes` cut the input.
    pub truncated: bool,
}

impl PipeArtifact {
    /// True when the text is not the complete input: replacements or cut.
    #[must_use]
    pub fn lossy(&self) -> bool {
        self.replacements > 0 || self.truncated
    }
}

/// Decode `input` lossily with exact accounting. `max_bytes` must be
/// nonzero; over-long input is cut back to a character boundary.
pub fn pipe_projection(input: &[u8], max_bytes: usize) -> Result<PipeArtifact, PipeError> {
    if max_bytes == 0 {
        return Err(PipeError {
            offset: None,
            message: "max_bytes must be nonzero".to_string(),
        });
    }
    let mut kept = input.len().min(max_bytes);
    let truncated = input.len() > max_bytes;
    // Back the cut off to a character boundary (at most 3 bytes): the byte
    // AT the cut must not be a UTF-8 continuation byte. Invalid sequences
    // elsewhere are the decoder's job (U+FFFD + accounting), not the cut's.
    while kept > 0 && kept < input.len() && (input[kept] & 0xC0) == 0x80 {
        kept -= 1;
    }
    let mut text = String::new();
    let mut replacements = 0;
    for chunk in input[..kept].utf8_chunks() {
        text.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            text.push('\u{FFFD}');
            replacements += 1;
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(input);
    Ok(PipeArtifact {
        id: hex_bytes(&hasher.finalize()),
        text,
        input_bytes: input.len(),
        kept_bytes: kept,
        replacements,
        truncated,
    })
}

/// Decode `input` strictly: any invalid UTF-8 is an error naming the byte
/// offset of the first bad sequence.
pub fn pipe_strict(input: &[u8]) -> Result<String, PipeError> {
    match std::str::from_utf8(input) {
        Ok(s) => Ok(s.to_string()),
        Err(e) => Err(PipeError {
            offset: Some(e.valid_up_to()),
            message: format!("invalid UTF-8 (error length {:?})", e.error_len()),
        }),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
