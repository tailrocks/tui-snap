use std::path::Path;
use super::*;


// ---------------------------------------------------------------------------
// Errors + limits
// ---------------------------------------------------------------------------

/// Compat import failure: explicit, never silent. Offsets are byte offsets of
/// the offending line's first byte in the source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatError {
    /// Filesystem failure (message carries path + cause).
    Io(String),
    /// Header/version failure (missing header, wrong version, bad dimensions).
    Version {
        /// Byte offset of the header line (always 0: header is line 1).
        offset: u64,
        /// What was wrong.
        msg: String,
    },
    /// Malformed event line.
    Content {
        /// Byte offset of the bad line's first byte.
        offset: u64,
        /// What was wrong.
        msg: String,
    },
    /// A bound from [`ImportLimits`] was exceeded. Fails; never truncates.
    TooLarge {
        /// Which bound (`line`, `events`, `bytes`).
        what: &'static str,
        /// The configured limit.
        limit: u64,
    },
}


impl std::fmt::Display for CompatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompatError::Io(e) => write!(f, "compat import I/O error: {e}"),
            CompatError::Version { offset, msg } => {
                write!(f, "compat import version error at byte {offset}: {msg}")
            }
            CompatError::Content { offset, msg } => {
                write!(f, "compat import content error at byte {offset}: {msg}")
            }
            CompatError::TooLarge { what, limit } => {
                write!(f, "compat import too large: {what} exceeds limit {limit}")
            }
        }
    }
}


impl std::error::Error for CompatError {}


/// Bounds for every importer in this module. Defaults are generous for real
/// traces but finite so hostile files fail fast instead of exhausting memory.
#[derive(Debug, Clone)]
pub struct ImportLimits {
    /// Max events (non-header lines carrying an entry) per file.
    pub max_events: usize,
    /// Max total source bytes read.
    pub max_total_bytes: u64,
    /// Max bytes per line (excluding the `\n`).
    pub max_line_bytes: usize,
}


impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_events: 100_000,
            max_total_bytes: 64 << 20,
            max_line_bytes: 4 << 20,
        }
    }
}


/// Read a source file with the total-bytes bound applied. Read-only: a single
/// `read`, no writes, no command execution anywhere in this module.
pub(crate) fn read_bounded(path: &Path, lim: &ImportLimits) -> Result<String, CompatError> {
    let len = std::fs::metadata(path)
        .map_err(|e| CompatError::Io(format!("stat {}: {e}", path.display())))?
        .len();
    if len > lim.max_total_bytes {
        return Err(CompatError::TooLarge {
            what: "bytes",
            limit: lim.max_total_bytes,
        });
    }
    let bytes = std::fs::read(path)
        .map_err(|e| CompatError::Io(format!("read {}: {e}", path.display())))?;
    if bytes.len() as u64 > lim.max_total_bytes {
        return Err(CompatError::TooLarge {
            what: "bytes",
            limit: lim.max_total_bytes,
        });
    }
    String::from_utf8(bytes).map_err(|e| CompatError::Content {
        offset: e.utf8_error().valid_up_to() as u64,
        msg: format!("source is not UTF-8: {e}"),
    })
}


/// Split into `(byte_offset, line)` pairs, skipping the empty segment after a
/// trailing newline. Offsets count the stripped `\n` (and `\r` of CRLF).
pub(crate) fn lines_with_offsets(text: &str) -> Vec<(u64, &str)> {
    let mut out = Vec::new();
    let mut off: u64 = 0;
    let mut rest = text;
    while !rest.is_empty() {
        let (line, adv) = match rest.find('\n') {
            Some(i) => (&rest[..i], i + 1),
            None => (rest, rest.len()),
        };
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !(line.is_empty() && adv == 0) {
            out.push((off, line));
        }
        off += adv as u64;
        rest = &rest[adv..];
    }
    out
}
