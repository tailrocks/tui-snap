//! Four-artifact export ([`emit_four`]).
//!
//! Moved out of `frozen.rs` so both files stay under the repo line gate;
//! behavior is unchanged.

use super::{AssertError, FOUR_STEM, render_sample};
use std::path::{Path, PathBuf};
use tuiscotti_core::screen::Screen;

/// Paths written by [`emit_four`].
#[derive(Debug, Clone)]
pub struct EmittedPaths {
    /// Directory the artifacts were written to.
    pub dir: PathBuf,
    /// `<dir>/snapshot.ansi` (normalized SGR dump).
    pub ansi: PathBuf,
    /// `<dir>/snapshot.txt` (plain text).
    pub txt: PathBuf,
    /// `<dir>/snapshot.png` (authoritative PNG).
    pub png: PathBuf,
    /// `<dir>/snapshot.html` (standalone render).
    pub html: PathBuf,
}

/// Emit ANSI/TXT/PNG/HTML from one [`Screen`] in a single sample pass.
/// Byte-deterministic: the same screen always yields identical bytes.
///
/// # Errors
///
/// Returns [`AssertError`] when rendering fails or an artifact cannot be
/// written.
pub fn emit_four(screen: &Screen, dir: &Path) -> Result<EmittedPaths, AssertError> {
    let sample = render_sample(screen)?;
    let io = |p: &Path, e: std::io::Error| AssertError::Io(format!("{}: {e}", p.display()));
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    let paths = EmittedPaths {
        dir: dir.to_path_buf(),
        ansi: dir.join(format!("{FOUR_STEM}.ansi")),
        txt: dir.join(format!("{FOUR_STEM}.txt")),
        png: dir.join(format!("{FOUR_STEM}.png")),
        html: dir.join(format!("{FOUR_STEM}.html")),
    };
    std::fs::write(&paths.ansi, sample.ansi.as_bytes()).map_err(|e| io(&paths.ansi, e))?;
    std::fs::write(&paths.txt, sample.txt.as_bytes()).map_err(|e| io(&paths.txt, e))?;
    std::fs::write(&paths.png, &sample.png).map_err(|e| io(&paths.png, e))?;
    std::fs::write(&paths.html, sample.html.as_bytes()).map_err(|e| io(&paths.html, e))?;
    Ok(paths)
}
