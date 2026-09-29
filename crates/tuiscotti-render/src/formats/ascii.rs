//! ASCII: explicit 7-bit diagnostic projection of canonical state.
//!
//! [`ascii_projection`] renders a [`Frame`] as pure 7-bit ASCII. Every
//! non-ASCII scalar is replaced under the documented table below and every
//! replacement is recorded in [`AsciiArtifact::substitutions`]. Column
//! geometry is preserved: a width-2 lead cell always emits exactly two ASCII
//! columns, a width-1 cell exactly one; continuation cells emit nothing.
//!
//! Substitution table (applied per scalar, then padded/truncated to the cell
//! width with `?`):
//!
//! | Input | Output | Input | Output |
//! |---|---|---|
//! | `─` | `-` | `│` | `|` |
//! | `┌` `┐` `└` `┘` `├` `┤` `┬` `┴` `┼` `╭` `╮` `╯` `╰` | `+` | `═` `║` `╔` `╗` `╚` `╝` `╠` `╣` `╦` `╩` `╬` | `#` |
//! | `█` `▓` `▒` `░` `▀` `▄` `■` `□` `▪` `▫` | `#` | Braille `⠀`..`⣿` | `:` |
//! | `→` | `>` | `←` | `<` |
//! | `↑` | `^` | `↓` `↔` | `v` / `<` |
//! | `✓` | `v` | `✗` | `x` |
//! | `★` | `*` | `…` | `.` |
//! | `·` | `.` | `×` | `x` |
//! | `÷` | `/` | `±` | `+` |
//! | Combining marks / default-ignorables (VS16, ZWJ, …) | dropped (no column) |
//! | C0/C1 controls (other than row joins) | `?` |
//! | Anything else non-ASCII | `?` |
//!
//! Whitespace policy: interior whitespace is preserved verbatim, trailing
//! blanks are trimmed per row, rows join with `\n`, no trailing newline —
//! the same framing as [`crate::formats::txt::txt_projection`].
//!
//! Lossy ASCII never stands in for equality: [`AsciiArtifact::lossy`] is
//! true whenever any substitution fired, and
//! [`AsciiArtifact::lossless_text`] returns `Some` only for loss-free
//! projections. Equality gates must use canonical JSON or TXT, never ASCII.
//!
//! [`Frame`]: tuiscotti_core::frame::Frame

use tuiscotti_core::frame::Frame;

/// One cell whose symbol could not be represented in 7-bit ASCII as-is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsciiSubstitution {
    /// Grid-local cell position.
    pub x: u16,
    /// Grid-local cell position.
    pub y: u16,
    /// Original canonical symbol.
    pub original: String,
    /// ASCII replacement actually emitted (exactly `width` columns).
    pub replacement: String,
}

/// A 7-bit ASCII projection plus its exact loss accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsciiArtifact {
    /// 7-bit ASCII text: every byte is `< 0x80`.
    pub text: String,
    /// Every cell whose symbol was substituted, in row-major order.
    pub substitutions: Vec<AsciiSubstitution>,
}

impl AsciiArtifact {
    /// True when any substitution fired. A lossy projection is diagnostic
    /// output only and must never feed an equality gate.
    #[must_use]
    pub fn lossy(&self) -> bool {
        !self.substitutions.is_empty()
    }

    /// The projected text, but only when nothing was substituted.
    #[must_use]
    pub fn lossless_text(&self) -> Option<&str> {
        if self.lossy() { None } else { Some(&self.text) }
    }
}

/// Map one scalar to ASCII. `None` means the scalar is dropped (combining
/// marks and default-ignorables carry no column of their own).
fn map_scalar(c: char) -> Option<char> {
    if c.is_ascii() {
        if c.is_control() {
            return Some('?');
        }
        return Some(c);
    }
    let out = match c {
        '─' => '-',
        '│' => '|',
        '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮' | '╯' | '╰' => {
            '+'
        }
        '═' | '║' | '╔' | '╗' | '╚' | '╝' | '╠' | '╣' | '╦' | '╩' | '╬' => {
            '#'
        }
        '█' | '▓' | '▒' | '░' | '▀' | '▄' | '■' | '□' | '▪' | '▫' => '#',
        '→' => '>',
        '←' | '↔' => '<',
        '↑' => '^',
        '↓' => 'v',
        '✓' => 'v',
        '✗' => 'x',
        '★' => '*',
        '…' | '·' => '.',
        '×' => 'x',
        '÷' => '/',
        '±' => '+',
        '\u{2800}'..='\u{28ff}' => ':',
        '\u{0300}'..='\u{036f}' | '\u{fe00}'..='\u{fe0f}' | '\u{200d}' | '\u{feff}' => return None,
        _ => '?',
    };
    Some(out)
}

/// Project one lead cell to exactly `width` ASCII columns.
fn project_cell(symbol: &str, width: u8) -> String {
    let mut mapped = String::new();
    for c in symbol.chars() {
        if let Some(o) = map_scalar(c) {
            mapped.push(o);
        }
    }
    let cols = usize::from(width.max(1));
    if mapped.len() > cols {
        mapped.truncate(cols);
    }
    while mapped.len() < cols {
        mapped.push('?');
    }
    mapped
}

/// Render `frame` as 7-bit ASCII with exact substitution accounting.
///
/// Continuation cells contribute nothing (the lead already holds the
/// symbol). See the module docs for the substitution table and the
/// whitespace policy.
#[must_use]
pub fn ascii_projection(frame: &Frame) -> AsciiArtifact {
    let mut substitutions = Vec::new();
    let mut out = String::new();
    for y in 0..frame.rows {
        if y > 0 {
            out.push('\n');
        }
        let mut row = String::new();
        for x in 0..frame.cols {
            let Some(cell) = frame.get(x, y) else {
                continue;
            };
            if cell.continuation {
                continue;
            }
            let replacement = project_cell(&cell.symbol, cell.width);
            if replacement != cell.symbol {
                substitutions.push(AsciiSubstitution {
                    x,
                    y,
                    original: cell.symbol.clone(),
                    replacement: replacement.clone(),
                });
            }
            row.push_str(&replacement);
        }
        out.push_str(row.trim_end());
    }
    AsciiArtifact {
        text: out,
        substitutions,
    }
}

/// Fail unless every byte of `text` is 7-bit ASCII.
pub fn assert_seven_bit(text: &str) -> Result<(), crate::formats::FormatError> {
    match text.bytes().position(|b| b >= 0x80) {
        Some(i) => Err(crate::formats::FormatError(format!(
            "non-ASCII byte 0x{:02x} at offset {i} in ASCII projection",
            text.as_bytes()[i]
        ))),
        None => Ok(()),
    }
}
