use super::{
    CellDiff, CompareOutcome, MAX_CELL_DIFFS, SnapshotError, Status, Store, sha256_hex,
    write_atomic,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use tuiscotti_core::frame::Frame;
use tuiscotti_render::diff;
use tuiscotti_render::profile::Profile;
use tuiscotti_render::render;

fn summarize(cell: &tuiscotti_core::frame::Cell) -> String {
    if cell.continuation {
        return "…".to_string();
    }
    let (fg, bg) = Frame::resolve_cell(
        cell,
        tuiscotti_core::frame::Rgb::new(0xd0, 0xd0, 0xd0),
        tuiscotti_core::frame::Rgb::new(0, 0, 0),
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
        tuiscotti_core::frame::UnderlineStyle::None => {}
        tuiscotti_core::frame::UnderlineStyle::Single => mods.push_str("+ul"),
        tuiscotti_core::frame::UnderlineStyle::Double => mods.push_str("+ul2"),
        tuiscotti_core::frame::UnderlineStyle::Curly => mods.push_str("+ulcurl"),
        tuiscotti_core::frame::UnderlineStyle::Dotted => mods.push_str("+uldot"),
        tuiscotti_core::frame::UnderlineStyle::Dashed => mods.push_str("+uldash"),
    }
    if cell.mods.strikethrough {
        mods.push_str("+strike");
    }
    if cell.mods.reverse {
        mods.push_str("+rev");
    }
    if !cell.underline_color.is_default() {
        let uc = match cell.underline_color {
            tuiscotti_core::frame::Color::Default => fg,
            tuiscotti_core::frame::Color::Indexed(i) => tuiscotti_core::frame::Rgb::from_indexed(i),
            tuiscotti_core::frame::Color::Rgb(r) => r,
        };
        write!(mods, "+ulc={}", uc.to_hex()).ok();
    }
    format!(
        "{:?} fg={} bg={}{mods}",
        cell.symbol,
        fg.to_hex(),
        bg.to_hex()
    )
}

impl Store {
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
    ///
    /// # Errors
    ///
    /// Returns `SnapshotError` when rendering, reading, or writing artifacts fails.
    pub fn check(
        &self,
        name: &str,
        actual: &Frame,
        profile: &Profile,
        faces: &tuiscotti_render::profile::FontFaces<'_>,
        pixel_threshold: f64,
    ) -> Result<CompareOutcome, SnapshotError> {
        let mut renderer = render::Renderer::new(profile, faces)?;
        self.check_with(&mut renderer, name, actual, pixel_threshold)
    }

    /// [`Self::check`] through a caller-owned [`render::Renderer`], so a
    /// suite reuses the parsed faces and the glyph cache across checks.
    ///
    /// # Errors
    ///
    /// Returns `SnapshotError` when rendering, reading, or writing artifacts fails.
    pub fn check_with(
        &self,
        renderer: &mut render::Renderer,
        name: &str,
        actual: &Frame,
        pixel_threshold: f64,
    ) -> Result<CompareOutcome, SnapshotError> {
        let candidate = self.write_candidate(renderer, name, actual, pixel_threshold)?;
        let approved_frame_path = self.approved_frame(name);
        let mut outcome = fresh_outcome(name, actual, &candidate, &approved_frame_path);
        let Some(approved) = Self::load_approved_frame(&approved_frame_path, &mut outcome)? else {
            return Ok(outcome);
        };
        outcome.digest_expected = Some(format!("{:016x}", approved.digest()));
        compare_cells(actual, &approved, &mut outcome);
        if self.compare_pixels(name, &candidate, &mut outcome)? {
            return Ok(outcome);
        }
        if matches!(outcome.status, Status::MissingApproval) {
            outcome.status = Status::Matched;
        }
        Ok(outcome)
    }

    /// Validate, render, and write the actual candidate trio (frame/PNG/
    /// fidelity) plus its sealing manifest. Returns what the gates need.
    fn write_candidate(
        &self,
        renderer: &mut render::Renderer,
        name: &str,
        actual: &Frame,
        pixel_threshold: f64,
    ) -> Result<CandidateWrites, SnapshotError> {
        // F1: every path below joins `name` — reject escapes before any write.
        crate::grouped::validate_name(name)?;
        // C04: invalid tolerances are rejected, never silently applied — a
        // NaN threshold would make every `score < threshold` false and fake
        // a match. The strict gate itself takes no threshold; review
        // leniency lives only on this validated perceptual policy.
        let perceptual = diff::PerceptualPolicy::new(pixel_threshold)?;
        actual.validate().map_err(SnapshotError::from)?;
        let artifacts = renderer.render(actual).map_err(SnapshotError::from)?;
        let actual_frame_path = self.actual_frame(name);
        let actual_png_path = self.actual_png(name);
        let frame_bytes = actual.to_json();
        let fidelity_bytes = artifacts.fidelity.to_json();
        write_atomic(&actual_frame_path, frame_bytes.as_bytes())?;
        write_atomic(&actual_png_path, &artifacts.png)?;
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
            "png_sha256": sha256_hex(&artifacts.png),
            "fidelity_sha256": sha256_hex(fidelity_bytes.as_bytes()),
            "profile": renderer.profile().name,
            "complete": true,
        });
        let manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| SnapshotError(format!("candidate manifest failed to serialize: {e}")))?;
        write_atomic(&self.actual_manifest(name), manifest_json.as_bytes())?;
        Ok(CandidateWrites {
            png_bytes: artifacts.png,
            actual_frame: actual_frame_path,
            actual_png: actual_png_path,
            perceptual,
        })
    }

    /// Load the approved frame. `None` (missing file, or a corrupt file
    /// recorded on the outcome) ends the check with the outcome as-is.
    fn load_approved_frame(
        approved_frame_path: &Path,
        outcome: &mut CompareOutcome,
    ) -> Result<Option<Frame>, SnapshotError> {
        let approved_text = match std::fs::read_to_string(approved_frame_path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(SnapshotError(format!(
                    "cannot read {}: {e}",
                    approved_frame_path.display()
                )));
            }
        };
        match Frame::from_json(&approved_text) {
            Ok(f) => Ok(Some(f)),
            Err(e) => {
                outcome.status = Status::CorruptApproval;
                outcome.note = format!(
                    "approved file {} is corrupt: {e}",
                    approved_frame_path.display()
                );
                Ok(None)
            }
        }
    }

    /// Pixel comparison over decoded PNGs. C06: expected bytes come
    /// from disk or the check fails — a missing approved PNG is
    /// `MissingApproval`, never regenerated in memory (frozen visual
    /// mode: a renderer upgrade must fail loudly, not silently
    /// re-render the expectation it is supposed to gate).
    /// Returns `true` when the outcome is final (missing approved PNG).
    fn compare_pixels(
        &self,
        name: &str,
        candidate: &CandidateWrites,
        outcome: &mut CompareOutcome,
    ) -> Result<bool, SnapshotError> {
        let approved_png_path = self.approved_png(name);
        let approved_png_bytes = match std::fs::read(&approved_png_path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                outcome.status = Status::MissingApproval;
                outcome.note = format!(
                    "approved PNG missing on disk: {}; expected bytes come from \
                     disk or the check fails",
                    approved_png_path.display()
                );
                return Ok(true);
            }
            Err(e) => {
                return Err(SnapshotError(format!(
                    "cannot read {}: {e}",
                    approved_png_path.display()
                )));
            }
        };
        outcome.expected_png = Some(approved_png_path);
        let verdict = diff::compare_png(&approved_png_bytes, &candidate.png_bytes)?;
        outcome.expected_png_bytes = Some(approved_png_bytes);
        if verdict.dims_equal {
            outcome.pixel_score = Some(verdict.score);
            if !candidate.perceptual.allows(verdict.score)
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
        } else {
            outcome.status = Status::DimensionMismatch;
        }
        Ok(false)
    }
}

/// What the gates need from the written candidate.
struct CandidateWrites {
    png_bytes: Vec<u8>,
    actual_frame: PathBuf,
    actual_png: PathBuf,
    perceptual: diff::PerceptualPolicy,
}

/// Fresh outcome skeleton: `MissingApproval` until a gate says otherwise.
fn fresh_outcome(
    name: &str,
    actual: &Frame,
    candidate: &CandidateWrites,
    approved_frame_path: &Path,
) -> CompareOutcome {
    CompareOutcome {
        name: name.to_string(),
        status: Status::MissingApproval,
        cell_diffs: Vec::new(),
        cell_diff_total: 0,
        pixel_score: None,
        approved_png_regenerated: false,
        digest_expected: None,
        digest_actual: format!("{:016x}", actual.digest()),
        actual_frame: candidate.actual_frame.clone(),
        actual_png: candidate.actual_png.clone(),
        expected_frame: approved_frame_path.to_path_buf(),
        expected_png: None,
        expected_png_bytes: None,
        diff_png: None,
        note: String::new(),
    }
}

/// Cell comparison (exact; dimension mismatch is a status, not a diff).
fn compare_cells(actual: &Frame, approved: &Frame, outcome: &mut CompareOutcome) {
    match actual.diff_cells(approved) {
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
                    let e = approved.get(x, y).map_or_else(|| "∅".into(), summarize);
                    let a = actual.get(x, y).map_or_else(|| "∅".into(), summarize);
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
}
