//! Approved store: full frames + images, never hash-only baselines.
//!
//! Layout under a store root:
//! ```text
//! approved/<name>.frame.json   approved/<name>.png
//! actual/<name>.frame.json     actual/<name>.png (+ .png.fidelity.json)
//! diff/<name>.png              report.html
//! ```
//!
//! Rules (failure handling is part of the design):
//! - actual artifacts are written BEFORE any assertion — a failing test
//!   still leaves reviewable evidence;
//! - approved artifacts are preserved untouched by `check` (only explicit
//!   [`Store::accept`] replaces them);
//! - a mismatch generates a visual diff PNG, cell diagnostics, and an HTML
//!   report; the gate then fails with artifact paths, not a bare hash;
//! - missing approval fails closed ("new snapshot requires review");
//! - corrupt approval files are explicit errors, never silent defaults;
//! - acceptance is an explicit local command. There is no env-var
//!   auto-bless: CI must never accept snapshots by itself;
//! - all writes are per-name files via atomic tmp+rename, so parallel test
//!   processes updating different names are safe (the index report is
//!   rewritten by whoever finalizes last — data files never clobber).

use crate::diff;
use crate::frame::{Frame, FrameError};
use crate::profile::Profile;
use crate::render;
use std::path::{Path, PathBuf};

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

impl From<crate::render::RenderError> for SnapshotError {
    fn from(e: crate::render::RenderError) -> Self {
        SnapshotError(e.to_string())
    }
}

impl From<crate::diff::DiffError> for SnapshotError {
    fn from(e: crate::diff::DiffError) -> Self {
        SnapshotError(e.to_string())
    }
}

/// Gate status for one named snapshot.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Matched,
    CellsDiffer,
    PixelsDiffer,
    DimensionMismatch,
    MissingApproval,
    CorruptApproval,
    /// Actual candidate trio (frame/PNG/manifest) is inconsistent — an
    /// interrupted write, never a pass.
    CaptureIncomplete,
    /// Candidate trio is complete and consistent; no gate verdict yet.
    NotChecked,
}

impl Status {
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

    #[must_use]
    pub fn matched(self) -> bool {
        matches!(self, Status::Matched)
    }
}

/// One differing cell, summarized for humans.
#[derive(Debug, Clone)]
pub struct CellDiff {
    pub x: u16,
    pub y: u16,
    pub expected: String,
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
    pub name: String,
    pub status: Status,
    pub cell_diffs: Vec<CellDiff>,
    pub cell_diff_total: usize,
    pub pixel_score: Option<f64>,
    /// C06 removed in-memory regeneration of missing approved PNGs: the gate
    /// fails closed instead, so this is always `false`. Kept so existing
    /// readers (`expected_png_bytes` consumers, report sidecars) keep compiling.
    pub approved_png_regenerated: bool,
    pub digest_expected: Option<String>,
    pub digest_actual: String,
    /// Extra context (e.g. why an approval file is corrupt).
    pub actual_frame: PathBuf,
    pub actual_png: PathBuf,
    pub expected_frame: PathBuf,
    /// The approved PNG path, but only when it actually exists on disk.
    pub expected_png: Option<PathBuf>,
    /// The exact expected image the pixel gate compared against: the
    /// approved PNG's bytes from disk. `None` whenever the gate did not run
    /// (missing/corrupt approval, missing approved PNG). Reports fall back
    /// to sidecar bytes when present, else a "missing approval" panel.
    pub expected_png_bytes: Option<Vec<u8>>,
    pub diff_png: Option<PathBuf>,
    pub note: String,
}

impl CompareOutcome {
    /// Fail with an actionable message (artifact paths + first diagnostics).
    pub fn ensure_matched(&self) -> Result<(), SnapshotError> {
        if self.status.matched() {
            return Ok(());
        }
        let mut msg = format!(
            "snapshot `{}` requires review ({}).",
            self.name,
            self.status.as_str()
        );
        msg.push_str(&format!(
            "\n  actual:   {} {}",
            self.actual_frame.display(),
            self.actual_png.display()
        ));
        if let Some(p) = &self.expected_png {
            msg.push_str(&format!("\n  expected: {}", p.display()));
        }
        if let Some(p) = &self.diff_png {
            msg.push_str(&format!("\n  diff:     {}", p.display()));
        }
        if self.cell_diff_total > 0 {
            msg.push_str(&format!(
                "\n  {} differing cell(s), first {}:",
                self.cell_diff_total,
                self.cell_diffs.len()
            ));
            for d in &self.cell_diffs {
                msg.push_str(&format!(
                    "\n    ({},{}): expected {} | actual {}",
                    d.x, d.y, d.expected, d.actual
                ));
            }
        }
        if let Some(s) = self.pixel_score {
            msg.push_str(&format!("\n  pixel similarity: {s:.6}"));
        }
        if !self.note.is_empty() {
            msg.push_str(&format!("\n  note: {}", self.note));
        }
        msg.push_str(&format!(
            "\n  review the report, then accept explicitly: tuisnap accept {}",
            self.name
        ));
        Err(SnapshotError(msg))
    }
}

fn summarize(cell: &crate::frame::Cell) -> String {
    if cell.continuation {
        return "…".to_string();
    }
    let (fg, bg) = Frame::resolve_cell(
        cell,
        crate::frame::Rgb::new(0xd0, 0xd0, 0xd0),
        crate::frame::Rgb::new(0, 0, 0),
    );
    let mut mods = String::new();
    if cell.mods.hidden {
        mods.push_str("+hidden");
    }
    if cell.mods.blink {
        mods.push_str("+blink");
    }
    if cell.mods.bold {
        mods.push_str("+bold");
    }
    if cell.mods.dim {
        mods.push_str("+dim");
    }
    if cell.mods.italic {
        mods.push_str("+italic");
    }
    match cell.mods.effective_underline_style() {
        crate::frame::UnderlineStyle::None => {}
        crate::frame::UnderlineStyle::Single => mods.push_str("+ul"),
        crate::frame::UnderlineStyle::Double => mods.push_str("+ul2"),
        crate::frame::UnderlineStyle::Curly => mods.push_str("+ulcurl"),
        crate::frame::UnderlineStyle::Dotted => mods.push_str("+uldot"),
        crate::frame::UnderlineStyle::Dashed => mods.push_str("+uldash"),
    }
    if cell.mods.strikethrough {
        mods.push_str("+strike");
    }
    if cell.mods.reverse {
        mods.push_str("+rev");
    }
    if !cell.underline_color.is_default() {
        let uc = match cell.underline_color {
            crate::frame::Color::Default => fg,
            crate::frame::Color::Indexed(i) => crate::frame::Rgb::from_indexed(i),
            crate::frame::Color::Rgb(r) => r,
        };
        mods.push_str(&format!("+ulc={}", uc.to_hex()));
    }
    format!(
        "{:?} fg={} bg={}{mods}",
        cell.symbol,
        fg.to_hex(),
        bg.to_hex()
    )
}

/// Lowercase hex SHA-256 of `bytes` (manifest integrity, not gating).
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Atomic file write (tmp in same dir + rename). Tmp names carry pid, a
/// process-wide counter, and the thread id, so same-name writers from
/// different threads never share a tmp file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SnapshotError> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .map_err(|e| SnapshotError(format!("cannot create {}: {e}", dir.display())))?;
        }
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
    root: PathBuf,
}

impl Store {
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Store root (approved/actual/diff/report.html live beneath it).
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn approved_frame(&self, name: &str) -> PathBuf {
        self.root
            .join("approved")
            .join(format!("{name}.frame.json"))
    }

    fn approved_png(&self, name: &str) -> PathBuf {
        self.root.join("approved").join(format!("{name}.png"))
    }

    fn actual_frame(&self, name: &str) -> PathBuf {
        self.root.join("actual").join(format!("{name}.frame.json"))
    }

    fn actual_png(&self, name: &str) -> PathBuf {
        self.root.join("actual").join(format!("{name}.png"))
    }

    /// Completion manifest sealing one actual candidate trio
    /// (`<name>.frame.json` + `<name>.png` + fidelity sidecar).
    fn actual_manifest(&self, name: &str) -> PathBuf {
        self.root
            .join("actual")
            .join(format!("{name}.manifest.json"))
    }

    /// Missing-glyph sidecar next to a PNG (`<name>.png.fidelity.json`).
    fn fidelity_sidecar(png: &Path) -> PathBuf {
        png.with_extension("png.fidelity.json")
    }

    fn diff_png(&self, name: &str) -> PathBuf {
        self.root.join("diff").join(format!("{name}.png"))
    }

    /// Names with actual frames (for `--all` acceptance).
    pub fn actual_names(&self) -> Result<Vec<String>, SnapshotError> {
        let dir = self.root.join("actual");
        let mut out = Vec::new();
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| SnapshotError(format!("cannot list {}: {e}", dir.display())))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| SnapshotError(format!("cannot list {}: {e}", dir.display())))?;
            if let Some(name) = entry.path().file_stem().and_then(|s| s.to_str()) {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(base) = name.strip_suffix(".frame") {
                        out.push(base.to_string());
                    }
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// Check one actual frame against approval. Writes actual artifacts
    /// BEFORE comparing; on mismatch also writes the diff PNG. The actual
    /// PNG is paired with a `<name>.png.fidelity.json` sidecar listing any
    /// glyphs the font chain did not cover (never silent tofu).
    ///
    /// `pixel_threshold`: strict gates pass 1.0; review passes lower it
    /// explicitly. Dimensions must match exactly either way.
    ///
    /// This constructs a fresh [`render::Renderer`] per call; bulk gates
    /// should build one and call [`Self::check_with`] instead.
    pub fn check(
        &self,
        name: &str,
        actual: &Frame,
        profile: &Profile,
        faces: &crate::profile::FontFaces<'_>,
        pixel_threshold: f64,
    ) -> Result<CompareOutcome, SnapshotError> {
        let mut renderer = render::Renderer::new(profile, faces)?;
        self.check_with(&mut renderer, name, actual, pixel_threshold)
    }

    /// [`Self::check`] through a caller-owned [`render::Renderer`], so a
    /// suite reuses the parsed faces and the glyph cache across checks.
    pub fn check_with(
        &self,
        renderer: &mut render::Renderer,
        name: &str,
        actual: &Frame,
        pixel_threshold: f64,
    ) -> Result<CompareOutcome, SnapshotError> {
        // F1: every path below joins `name` — reject escapes before any write.
        crate::grouped::validate_name(name)?;
        // C04: invalid tolerances are rejected, never silently applied — a
        // NaN threshold would make every `score < threshold` false and fake
        // a match. The strict gate itself takes no threshold; review
        // leniency lives only on this validated perceptual policy.
        let perceptual = diff::PerceptualPolicy::new(pixel_threshold)?;
        actual.validate().map_err(SnapshotError::from)?;
        let rendered = renderer.render(actual).map_err(SnapshotError::from)?;
        let actual_png_bytes = &rendered.png;
        let actual_frame_path = self.actual_frame(name);
        let actual_png_path = self.actual_png(name);
        let frame_bytes = actual.to_json();
        let fidelity_bytes = rendered.fidelity.to_json();
        write_atomic(&actual_frame_path, frame_bytes.as_bytes())?;
        write_atomic(&actual_png_path, actual_png_bytes)?;
        write_atomic(
            &Self::fidelity_sidecar(&actual_png_path),
            fidelity_bytes.as_bytes(),
        )?;
        // C08: seal the candidate trio last. A crash between the writes
        // above leaves a manifest that is absent or disagrees with the
        // artifacts, and `verify_candidate` reports CaptureIncomplete
        // instead of letting a later report silently re-render the gap.
        let manifest = serde_json::json!({
            "name": name,
            "frame_sha256": sha256_hex(frame_bytes.as_bytes()),
            "png_sha256": sha256_hex(actual_png_bytes),
            "fidelity_sha256": sha256_hex(fidelity_bytes.as_bytes()),
            "profile": renderer.profile().name,
            "complete": true,
        });
        write_atomic(
            &self.actual_manifest(name),
            serde_json::to_string_pretty(&manifest)
                .expect("manifest JSON serializes")
                .as_bytes(),
        )?;

        let approved_frame_path = self.approved_frame(name);
        let approved_png_path = self.approved_png(name);
        let digest_actual = format!("{:016x}", actual.digest());
        let mut outcome = CompareOutcome {
            name: name.to_string(),
            status: Status::MissingApproval,
            cell_diffs: Vec::new(),
            cell_diff_total: 0,
            pixel_score: None,
            approved_png_regenerated: false,
            digest_expected: None,
            digest_actual,
            actual_frame: actual_frame_path.clone(),
            actual_png: actual_png_path,
            expected_frame: approved_frame_path.clone(),
            expected_png: None,
            expected_png_bytes: None,
            diff_png: None,
            note: String::new(),
        };

        let approved_text = match std::fs::read_to_string(&approved_frame_path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(outcome),
            Err(e) => {
                return Err(SnapshotError(format!(
                    "cannot read {}: {e}",
                    approved_frame_path.display()
                )));
            }
        };
        let approved = match Frame::from_json(&approved_text) {
            Ok(f) => f,
            Err(e) => {
                outcome.status = Status::CorruptApproval;
                outcome.note = format!(
                    "approved file {} is corrupt: {e}",
                    approved_frame_path.display()
                );
                return Ok(outcome);
            }
        };
        outcome.digest_expected = Some(format!("{:016x}", approved.digest()));

        // Cell comparison (exact; dimension mismatch is a status, not a diff).
        match actual.diff_cells(&approved) {
            Err(_) => {
                outcome.status = Status::DimensionMismatch;
            }
            Ok(positions) => {
                outcome.cell_diff_total = positions.len();
                // Cursor-only change: every reported position holds equal
                // cells, so cell summaries would print identical text twice.
                // Say what actually changed instead.
                let cells_equal = positions.iter().all(|(x, y)| {
                    approved.get(*x, *y).map(summarize) == actual.get(*x, *y).map(summarize)
                });
                if cells_equal && outcome.cell_diff_total > 0 {
                    outcome.cell_diffs.push(CellDiff {
                        x: actual.cursor.x,
                        y: actual.cursor.y,
                        expected: Frame::summarize_cursor(&approved.cursor),
                        actual: Frame::summarize_cursor(&actual.cursor),
                    });
                } else {
                    for (x, y) in positions.into_iter().take(MAX_CELL_DIFFS) {
                        let e = approved
                            .get(x, y)
                            .map(summarize)
                            .unwrap_or_else(|| "∅".into());
                        let a = actual
                            .get(x, y)
                            .map(summarize)
                            .unwrap_or_else(|| "∅".into());
                        outcome.cell_diffs.push(CellDiff {
                            x,
                            y,
                            expected: e,
                            actual: a,
                        });
                    }
                }
                if outcome.cell_diff_total > 0 {
                    outcome.status = Status::CellsDiffer;
                }
            }
        }

        // Pixel comparison over decoded PNGs. C06: expected bytes come
        // from disk or the check fails — a missing approved PNG is
        // MissingApproval, never regenerated in memory (frozen visual
        // mode: a renderer upgrade must fail loudly, not silently
        // re-render the expectation it is supposed to gate).
        let approved_png_bytes = match std::fs::read(&approved_png_path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                outcome.status = Status::MissingApproval;
                outcome.note = format!(
                    "approved PNG missing on disk: {}; expected bytes come from \
                     disk or the check fails",
                    approved_png_path.display()
                );
                return Ok(outcome);
            }
            Err(e) => {
                return Err(SnapshotError(format!(
                    "cannot read {}: {e}",
                    approved_png_path.display()
                )));
            }
        };
        outcome.expected_png = Some(approved_png_path);
        let verdict = diff::compare_png(&approved_png_bytes, actual_png_bytes)?;
        outcome.expected_png_bytes = Some(approved_png_bytes);
        if !verdict.dims_equal {
            outcome.status = Status::DimensionMismatch;
        } else {
            outcome.pixel_score = Some(verdict.score);
            if !perceptual.allows(verdict.score)
                && !matches!(
                    outcome.status,
                    Status::CellsDiffer | Status::DimensionMismatch
                )
            {
                outcome.status = Status::PixelsDiffer;
            }
            if verdict.score < 1.0 {
                let path = self.diff_png(name);
                write_atomic(&path, &verdict.diff_png)?;
                outcome.diff_png = Some(path);
            }
        }

        if matches!(outcome.status, Status::MissingApproval) {
            outcome.status = Status::Matched;
        }
        Ok(outcome)
    }

    /// Verify one actual candidate trio (frame + PNG + fidelity sidecar
    /// against the completion manifest `check` seals last). Returns
    /// [`Status::NotChecked`] when the trio is complete and consistent —
    /// the candidate is intact but no gate verdict exists yet — and
    /// [`Status::CaptureIncomplete`] when anything is missing, unparsable,
    /// unsealed (`complete != true`), or hash-mismatched. Never errors:
    /// every failure mode IS the incomplete verdict.
    pub fn verify_candidate(&self, name: &str) -> Status {
        match self.candidate_problem(name) {
            None => Status::NotChecked,
            Some(_) => Status::CaptureIncomplete,
        }
    }

    /// `None` when the candidate trio is intact, else a human reason.
    fn candidate_problem(&self, name: &str) -> Option<String> {
        let frame_path = self.actual_frame(name);
        let png_path = self.actual_png(name);
        let manifest_path = self.actual_manifest(name);
        let frame_bytes = match std::fs::read(&frame_path) {
            Ok(b) if !b.is_empty() => b,
            _ => {
                return Some(format!(
                    "candidate `{name}` incomplete: {} missing or empty",
                    frame_path.display()
                ));
            }
        };
        let png_bytes = match std::fs::read(&png_path) {
            Ok(b) if !b.is_empty() => b,
            _ => {
                return Some(format!(
                    "candidate `{name}` incomplete: {} missing or empty",
                    png_path.display()
                ));
            }
        };
        let fidelity_bytes = match std::fs::read(Self::fidelity_sidecar(&png_path)) {
            Ok(b) if !b.is_empty() => b,
            _ => {
                return Some(format!(
                    "candidate `{name}` incomplete: fidelity sidecar for {} missing or empty",
                    png_path.display()
                ));
            }
        };
        let manifest_text = match std::fs::read_to_string(&manifest_path) {
            Ok(t) => t,
            Err(_) => {
                return Some(format!(
                    "candidate `{name}` incomplete: {} missing (interrupted write?)",
                    manifest_path.display()
                ));
            }
        };
        let manifest: serde_json::Value = match serde_json::from_str(&manifest_text) {
            Ok(v) => v,
            Err(e) => {
                return Some(format!(
                    "candidate `{name}` incomplete: {} unparsable: {e}",
                    manifest_path.display()
                ));
            }
        };
        if manifest.get("complete").and_then(|v| v.as_bool()) != Some(true) {
            return Some(format!(
                "candidate `{name}` incomplete: {} not sealed (complete != true)",
                manifest_path.display()
            ));
        }
        if manifest.get("name").and_then(|v| v.as_str()) != Some(name) {
            return Some(format!(
                "candidate `{name}` incomplete: {} names a different snapshot",
                manifest_path.display()
            ));
        }
        for (key, bytes) in [
            ("frame_sha256", frame_bytes.as_slice()),
            ("png_sha256", png_bytes.as_slice()),
            ("fidelity_sha256", fidelity_bytes.as_slice()),
        ] {
            let want = manifest.get(key).and_then(|v| v.as_str()).unwrap_or("");
            if sha256_hex(bytes) != want {
                return Some(format!(
                    "candidate `{name}` incomplete: {key} disagrees with the artifact on disk"
                ));
            }
        }
        None
    }

    /// Explicitly approve one snapshot: actual → approved (atomic).
    /// There is deliberately no environment-variable auto-accept.
    pub fn accept(&self, name: &str) -> Result<(), SnapshotError> {
        // F1: every path below joins `name` — reject escapes before any copy.
        crate::grouped::validate_name(name)?;
        for (src, dst) in [
            (self.actual_frame(name), self.approved_frame(name)),
            (self.actual_png(name), self.approved_png(name)),
        ] {
            let bytes = std::fs::read(&src).map_err(|e| {
                SnapshotError(format!(
                    "nothing to accept for `{name}` ({}: {e})",
                    src.display()
                ))
            })?;
            if src.ends_with(".json") || src.extension().and_then(|s| s.to_str()) == Some("json") {
                let text = String::from_utf8(bytes).map_err(|e| {
                    SnapshotError(format!("actual frame for `{name}` is not UTF-8: {e}"))
                })?;
                Frame::from_json(&text).map_err(|e| {
                    SnapshotError(format!("actual frame for `{name}` invalid, refusing: {e}"))
                })?;
                write_atomic(&dst, text.as_bytes())?;
            } else {
                write_atomic(&dst, &bytes)?;
            }
        }
        // Pair the missing-glyph sidecar when the check produced one.
        let (src, dst) = (
            Self::fidelity_sidecar(&self.actual_png(name)),
            Self::fidelity_sidecar(&self.approved_png(name)),
        );
        if let Ok(bytes) = std::fs::read(&src) {
            write_atomic(&dst, &bytes)?;
        }
        Ok(())
    }

    /// Assemble one report row from a check outcome. The HTML report links
    /// PNGs on disk (no base64). Expected bytes with no disk path are
    /// written next to the report under `report-media/`.
    pub fn report_entry(
        &self,
        outcome: &CompareOutcome,
        profile: &Profile,
    ) -> Result<ReportEntry, SnapshotError> {
        report_entry(outcome, profile)
    }

    /// Re-verify every actual frame in the store and rewrite `report.html` —
    /// the library form of the CLI `report` subcommand. Unmatched gates do
    /// not error here: inspect [`StoreReport::failed`] and the outcomes.
    ///
    /// This constructs a fresh [`render::Renderer`] per call; bulk callers
    /// should build one and use [`Self::report_with`].
    pub fn report(
        &self,
        profile: &Profile,
        faces: &crate::profile::FontFaces<'_>,
        pixel_threshold: f64,
        title: &str,
    ) -> Result<StoreReport, SnapshotError> {
        let mut renderer = render::Renderer::new(profile, faces)?;
        self.report_with(&mut renderer, pixel_threshold, title)
    }

    /// [`Self::report`] through a caller-owned [`render::Renderer`].
    pub fn report_with(
        &self,
        renderer: &mut render::Renderer,
        pixel_threshold: f64,
        title: &str,
    ) -> Result<StoreReport, SnapshotError> {
        // A store with no actuals yet yields an empty report (CLI parity),
        // while a genuinely unreadable directory stays an error.
        let names = match self.actual_names() {
            Ok(names) => names,
            Err(_) if !self.root.join("actual").exists() => Vec::new(),
            Err(e) => return Err(e),
        };
        let mut entries = Vec::new();
        let mut outcomes = Vec::new();
        for name in names {
            // C08: an interrupted candidate (frame without PNG, lost
            // manifest, hash drift) must report CaptureIncomplete — never
            // re-render through `check_with`, which would silently heal the
            // missing artifact back to Matched.
            if let Some(problem) = self.candidate_problem(&name) {
                let actual_frame_path = self.actual_frame(&name);
                let digest_actual = std::fs::read_to_string(&actual_frame_path)
                    .ok()
                    .and_then(|t| Frame::from_json(&t).ok())
                    .map(|f| format!("{:016x}", f.digest()))
                    .unwrap_or_default();
                let outcome = CompareOutcome {
                    name: name.clone(),
                    status: Status::CaptureIncomplete,
                    cell_diffs: Vec::new(),
                    cell_diff_total: 0,
                    pixel_score: None,
                    approved_png_regenerated: false,
                    digest_expected: None,
                    digest_actual,
                    actual_frame: actual_frame_path,
                    actual_png: self.actual_png(&name),
                    expected_frame: self.approved_frame(&name),
                    expected_png: None,
                    expected_png_bytes: None,
                    diff_png: None,
                    note: problem,
                };
                entries.push(self.report_entry(&outcome, renderer.profile())?);
                outcomes.push(outcome);
                continue;
            }
            let text = std::fs::read_to_string(self.actual_frame(&name)).map_err(|e| {
                SnapshotError(format!("cannot read actual frame for `{name}`: {e}"))
            })?;
            let frame = Frame::from_json(&text)?;
            let outcome = self.check_with(renderer, &name, &frame, pixel_threshold)?;
            entries.push(self.report_entry(&outcome, renderer.profile())?);
            outcomes.push(outcome);
        }
        let path = write_report(self, title, &entries)?;
        Ok(StoreReport { path, outcomes })
    }
}

/// Assemble one report row from a check outcome. Free-function form of
/// [`Store::report_entry`] so non-classic stores can build rows without a
/// [`Store`]. Does not read PNG bytes.
pub fn report_entry(
    outcome: &CompareOutcome,
    profile: &Profile,
) -> Result<ReportEntry, SnapshotError> {
    Ok(ReportEntry {
        outcome: outcome.clone(),
        profile_desc: profile.name.clone(),
        font_sha256: profile.font_sha256.clone(),
    })
}

/// Result of [`Store::report`]/[`Store::report_with`]: the rewritten report
/// plus every outcome it embeds.
#[derive(Debug)]
pub struct StoreReport {
    /// Path of the rewritten `report.html`.
    pub path: PathBuf,
    /// One outcome per re-verified actual, in name order.
    pub outcomes: Vec<CompareOutcome>,
}

impl StoreReport {
    /// Outcomes that did not match (the CLI turns this into a non-zero exit).
    #[must_use]
    pub fn failed(&self) -> usize {
        self.outcomes.iter().filter(|o| !o.status.matched()).count()
    }
}

/// One row of the review HTML report. Images are files on disk; the HTML
/// only stores relative `href`s so hundreds of captures stay browser-usable.
pub struct ReportEntry {
    pub outcome: CompareOutcome,
    pub profile_desc: String,
    pub font_sha256: String,
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// JSON embedded in `<script type="application/json">`: escape `<` so a cell
/// symbol like `</script>` cannot terminate the element (still valid JSON —
/// `\u003c` re-parses to `<`, keeping lossless re-import).
pub fn json_for_script(json: &str) -> String {
    json.replace('<', "\\u003c")
}

/// Write a review index: PNGs linked from disk (never base64-embedded),
/// failed captures first, frame JSON linked not inlined.
pub fn write_report(
    store: &Store,
    title: &str,
    entries: &[ReportEntry],
) -> Result<PathBuf, SnapshotError> {
    write_report_at(&store.root.join("report.html"), title, entries)
}

/// [`write_report`] with an explicit output path, for stores whose report
/// does not live at a fixed location (e.g. [`crate::grouped::GroupedStore`],
/// which keeps its report out of the approved tree).
pub fn write_report_at(
    path: &Path,
    title: &str,
    entries: &[ReportEntry],
) -> Result<PathBuf, SnapshotError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| SnapshotError(format!("cannot create {}: {e}", parent.display())))?;
    }
    let report_dir = path.parent().unwrap_or(Path::new("."));
    let failed_n = entries
        .iter()
        .filter(|e| !e.outcome.status.matched())
        .count();
    let mut ordered: Vec<&ReportEntry> = Vec::with_capacity(entries.len());
    ordered.extend(entries.iter().filter(|e| !e.outcome.status.matched()));
    ordered.extend(entries.iter().filter(|e| e.outcome.status.matched()));

    let mut body = format!(
        "<p>{} captures · {} matched · {} failed</p>\n",
        entries.len(),
        entries.len() - failed_n,
        failed_n
    );
    for e in ordered {
        let o = &e.outcome;
        body.push_str(&format!(
            "<section id=\"{}\"><h2>{} — {}</h2>\n",
            esc_attr(&o.name),
            esc_html(&o.name),
            o.status.as_str()
        ));
        body.push_str("<div class=\"imgs\">");
        let expected_src = match &o.expected_png {
            Some(p) if p.exists() => Some(rel_href(report_dir, p)),
            _ => match &o.expected_png_bytes {
                Some(bytes) => Some(rel_href(
                    report_dir,
                    &write_report_sidecar(report_dir, &o.name, "expected", bytes)?,
                )),
                None => None,
            },
        };
        if let Some(src) = expected_src {
            body.push_str(&format!(
                "<figure><figcaption>expected</figcaption><img src=\"{src}\" alt=\"expected {}\"></figure>",
                esc_attr(&o.name)
            ));
        } else {
            body.push_str(
                "<figure><figcaption>expected</figcaption><p>missing approval</p></figure>",
            );
        }
        if o.actual_png.exists() {
            let src = rel_href(report_dir, &o.actual_png);
            body.push_str(&format!(
                "<figure><figcaption>actual</figcaption><img src=\"{src}\" alt=\"actual {}\"></figure>",
                esc_attr(&o.name)
            ));
        }
        if let Some(p) = o.diff_png.as_ref().filter(|p| p.exists()) {
            let src = rel_href(report_dir, p);
            body.push_str(&format!(
                "<figure><figcaption>diff</figcaption><img src=\"{src}\" alt=\"diff {}\"></figure>",
                esc_attr(&o.name)
            ));
        }
        body.push_str("</div>");
        if o.cell_diff_total > 0 {
            body.push_str(&format!(
                "<p>{} differing cell(s):</p><table><tr><th>x</th><th>y</th><th>expected</th><th>actual</th></tr>",
                o.cell_diff_total
            ));
            for d in &o.cell_diffs {
                body.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    d.x,
                    d.y,
                    esc_html(&d.expected),
                    esc_html(&d.actual)
                ));
            }
            body.push_str("</table>");
        }
        if let Some(s) = o.pixel_score {
            body.push_str(&format!("<p>pixel similarity: {s:.6}</p>"));
        }
        if o.actual_frame.exists() {
            body.push_str(&format!(
                "<p><a href=\"{}\">actual frame.json</a></p>",
                rel_href(report_dir, &o.actual_frame)
            ));
        }
        if o.expected_frame.exists() {
            body.push_str(&format!(
                "<p><a href=\"{}\">expected frame.json</a></p>",
                rel_href(report_dir, &o.expected_frame)
            ));
        }
        body.push_str("</section>");
    }
    let profile_line = entries
        .first()
        .map(|e| {
            format!(
                "<p>profile: {} · font sha256: {}</p>",
                esc_html(&e.profile_desc),
                esc_html(&e.font_sha256)
            )
        })
        .unwrap_or_default();
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title>\n<style>body{{font-family:system-ui,sans-serif;background:#141414;color:#eee;margin:24px}}section{{border:1px solid #444;margin:16px 0;padding:16px}}img{{max-width:100%;image-rendering:pixelated}}table{{border-collapse:collapse}}td,th{{border:1px solid #555;padding:2px 8px;font-family:monospace}}</style></head><body><h1>{}</h1>{profile_line}{body}</body></html>",
        esc_html(title),
        esc_html(title)
    );
    write_atomic(path, html.as_bytes())?;
    Ok(path.to_path_buf())
}

fn esc_attr(s: &str) -> String {
    esc_html(s).replace('"', "&quot;")
}

fn rel_href(from_dir: &Path, to: &Path) -> String {
    let from = from_dir.components().collect::<Vec<_>>();
    let to_c = to.components().collect::<Vec<_>>();
    let mut i = 0;
    while i < from.len() && i < to_c.len() && from[i] == to_c[i] {
        i += 1;
    }
    let mut out = PathBuf::new();
    for _ in i..from.len() {
        out.push("..");
    }
    for c in &to_c[i..] {
        out.push(*c);
    }
    if out.as_os_str().is_empty() {
        return to
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| to.display().to_string());
    }
    out.to_string_lossy().replace('\\', "/")
}

fn write_report_sidecar(
    report_dir: &Path,
    name: &str,
    kind: &str,
    bytes: &[u8],
) -> Result<PathBuf, SnapshotError> {
    let dir = report_dir.join("report-media");
    std::fs::create_dir_all(&dir)
        .map_err(|e| SnapshotError(format!("cannot create {}: {e}", dir.display())))?;
    let safe = name.replace('/', "__");
    let path = dir.join(format!("{safe}-{kind}.png"));
    write_atomic(&path, bytes)?;
    Ok(path)
}
