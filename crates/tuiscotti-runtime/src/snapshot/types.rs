use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use tuiscotti_core::frame::FrameError;

/// Snapshot failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotError(pub String);

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "snapshot error: {}", self.0)
    }
}

impl std::error::Error for SnapshotError {}

impl From<FrameError> for SnapshotError {
    fn from(e: FrameError) -> Self {
        SnapshotError(e.to_string())
    }
}

impl From<tuiscotti_render::render::RenderError> for SnapshotError {
    fn from(e: tuiscotti_render::render::RenderError) -> Self {
        SnapshotError(e.to_string())
    }
}

impl From<tuiscotti_render::diff::DiffError> for SnapshotError {
    fn from(e: tuiscotti_render::diff::DiffError) -> Self {
        SnapshotError(e.to_string())
    }
}

/// Gate status for one named snapshot.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Every gate passed.
    Matched,
    /// Cell-exact gate failed.
    CellsDiffer,
    /// Render-level or pixel gate failed.
    PixelsDiffer,
    /// Actual and approved dimensions differ.
    DimensionMismatch,
    /// An approved artifact is absent (fail-closed).
    MissingApproval,
    /// An approved artifact cannot be parsed.
    CorruptApproval,
    /// Actual candidate trio (frame/PNG/manifest) is inconsistent — an
    /// interrupted write, never a pass.
    CaptureIncomplete,
    /// Candidate trio is complete and consistent; no gate verdict yet.
    NotChecked,
}

impl Status {
    /// Machine-readable status slug (e.g. `cells-differ`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Matched => "matched",
            Status::CellsDiffer => "cells-differ",
            Status::PixelsDiffer => "pixels-differ",
            Status::DimensionMismatch => "dimension-mismatch",
            Status::MissingApproval => "missing-approval",
            Status::CorruptApproval => "corrupt-approval",
            Status::CaptureIncomplete => "capture-incomplete",
            Status::NotChecked => "not-checked",
        }
    }

    /// Whether this status is a pass.
    #[must_use]
    pub fn matched(self) -> bool {
        matches!(self, Status::Matched)
    }
}

/// One differing cell, summarized for humans.
#[derive(Debug, Clone)]
pub struct CellDiff {
    /// Cell column.
    pub x: u16,
    /// Cell row.
    pub y: u16,
    /// Human summary of the approved cell.
    pub expected: String,
    /// Human summary of the actual cell.
    pub actual: String,
}

/// Cap stored per-cell diagnostics (the total is always counted).
pub const MAX_CELL_DIFFS: usize = 100;

/// Outcome of one `check`. Artifacts on disk even when unmatched.
///
/// Dropping this without [`Self::ensure_matched`] (or otherwise asserting on
/// [`Self::status`]) is a silent pass — hence `#[must_use]`.
#[must_use]
#[derive(Debug, Clone)]
pub struct CompareOutcome {
    /// Snapshot name.
    pub name: String,
    /// Gate status.
    pub status: Status,
    /// First differing cells (capped at `MAX_CELL_DIFFS`).
    pub cell_diffs: Vec<CellDiff>,
    /// Total differing cells (always fully counted).
    pub cell_diff_total: usize,
    /// Pixel similarity score, when the pixel gate ran.
    pub pixel_score: Option<f64>,
    /// C06 removed in-memory regeneration of missing approved PNGs: the gate
    /// fails closed instead, so this is always `false`. Kept so existing
    /// readers (`expected_png_bytes` consumers, report sidecars) keep compiling.
    pub approved_png_regenerated: bool,
    /// Digest of the approved frame, when one was loaded.
    pub digest_expected: Option<String>,
    /// Digest of the actual frame.
    pub digest_actual: String,
    /// Actual frame path.
    pub actual_frame: PathBuf,
    /// Actual PNG path.
    pub actual_png: PathBuf,
    /// Approved frame path.
    pub expected_frame: PathBuf,
    /// The approved PNG path, but only when it actually exists on disk.
    pub expected_png: Option<PathBuf>,
    /// The exact expected image the pixel gate compared against: the
    /// approved PNG's bytes from disk. `None` whenever the gate did not run
    /// (missing/corrupt approval, missing approved PNG). Reports fall back
    /// to sidecar bytes when present, else a "missing approval" panel.
    pub expected_png_bytes: Option<Vec<u8>>,
    /// Diff PNG path, written on sub-1.0 pixel scores.
    pub diff_png: Option<PathBuf>,
    /// Extra context (e.g. why an approval file is corrupt).
    pub note: String,
}

impl CompareOutcome {
    /// Fail with an actionable message (artifact paths + first diagnostics).
    ///
    /// # Errors
    ///
    /// Returns `SnapshotError` describing the mismatch when not matched.
    pub fn ensure_matched(&self) -> Result<(), SnapshotError> {
        if self.status.matched() {
            return Ok(());
        }
        let mut msg = format!(
            "snapshot `{}` requires review ({}).",
            self.name,
            self.status.as_str()
        );
        write!(
            msg,
            "\n  actual:   {} {}",
            self.actual_frame.display(),
            self.actual_png.display()
        )
        .ok();
        if let Some(p) = &self.expected_png {
            write!(msg, "\n  expected: {}", p.display()).ok();
        }
        if let Some(p) = &self.diff_png {
            write!(msg, "\n  diff:     {}", p.display()).ok();
        }
        if self.cell_diff_total > 0 {
            write!(
                msg,
                "\n  {} differing cell(s), first {}:",
                self.cell_diff_total,
                self.cell_diffs.len()
            )
            .ok();
            for d in &self.cell_diffs {
                write!(
                    msg,
                    "\n    ({},{}): expected {} | actual {}",
                    d.x, d.y, d.expected, d.actual
                )
                .ok();
            }
        }
        if let Some(s) = self.pixel_score {
            write!(msg, "\n  pixel similarity: {s:.6}").ok();
        }
        if !self.note.is_empty() {
            write!(msg, "\n  note: {}", self.note).ok();
        }
        write!(
            msg,
            "\n  review the report, then accept explicitly: tuisnap accept {}",
            self.name
        )
        .ok();
        Err(SnapshotError(msg))
    }
}

/// Lowercase hex SHA-256 of `bytes` (manifest integrity, not gating).
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        write!(s, "{b:02x}").ok();
    }
    s
}

/// Atomic file write (tmp in same dir + rename). Tmp names carry pid, a
/// process-wide counter, and the thread id, so same-name writers from
/// different threads never share a tmp file.
///
/// # Errors
///
/// Returns `SnapshotError` when the write or publish fails.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SnapshotError> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)
            .map_err(|e| SnapshotError(format!("cannot create {}: {e}", dir.display())))?;
    }
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!(
        "tmp.{}.{n}.{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&tmp, bytes)
        .map_err(|e| SnapshotError(format!("cannot write {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| SnapshotError(format!("cannot publish {}: {e}", path.display())))?;
    Ok(())
}

/// Approved/actual/diff artifact store.
#[derive(Debug, Clone)]
pub struct Store {
    pub(crate) root: PathBuf,
}
