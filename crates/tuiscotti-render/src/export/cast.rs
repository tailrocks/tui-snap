//! Asciinema v2 `.cast` export.

use super::{CastPolicy, ExportError};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// JSON string escaping (hand-rolled: fixed output, no dependency surface)
// ---------------------------------------------------------------------------

fn json_escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub(crate) fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    json_escape(s, &mut out);
    out
}

// ---------------------------------------------------------------------------
// asciinema v2 cast (A06)
// ---------------------------------------------------------------------------

/// Fixed output file name inside the target directory.
pub const CAST_FILE_NAME: &str = "session.cast";

/// Write an asciinema v2 `.cast` document: `frames` are
/// `(Screen-rendered text, seconds since the previous frame)` pairs and
/// `cols`/`rows` are the terminal dimensions for the pinned header.
///
/// Event times are cumulative (`dt` sums) printed with fixed `{:.6}`
/// precision; the header timestamp is pinned 0 (see [`CastPolicy`]). Same
/// input → byte-identical file. Creates `dir` when missing.
pub fn cast_v2(
    frames: &[(String, f64)],
    cols: u16,
    rows: u16,
    dir: &Path,
) -> Result<PathBuf, ExportError> {
    cast_v2_with(frames, cols, rows, dir, &CastPolicy::default())
}

/// [`cast_v2`] with an explicit header policy.
pub fn cast_v2_with(
    frames: &[(String, f64)],
    cols: u16,
    rows: u16,
    dir: &Path,
    policy: &CastPolicy,
) -> Result<PathBuf, ExportError> {
    if cols == 0 || rows == 0 {
        return Err(ExportError::InvalidInput(format!(
            "cast dimensions must be nonzero, got {cols}x{rows}"
        )));
    }
    for (i, (_, dt)) in frames.iter().enumerate() {
        if !dt.is_finite() || *dt < 0.0 {
            return Err(ExportError::InvalidInput(format!(
                "cast frame {i} has non-finite or negative dt ({dt})"
            )));
        }
    }
    let mut doc = String::new();
    doc.push_str(&format!(
        "{{\"version\":2,\"width\":{cols},\"height\":{rows},\"timestamp\":{},\"title\":{},\"env\":{{\"TERM\":{}}}}}\n",
        policy.timestamp,
        json_string(&policy.title),
        json_string(&policy.term),
    ));
    let mut t = 0.0f64;
    for (text, dt) in frames {
        t += dt;
        doc.push_str(&format!("[{t:.6},\"o\",{}]\n", json_string(text)));
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(CAST_FILE_NAME);
    std::fs::write(&path, doc)?;
    Ok(path)
}
