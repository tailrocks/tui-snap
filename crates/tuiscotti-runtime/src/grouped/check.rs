use super::*;
use crate::snapshot::{
    CompareOutcome, SnapshotError, Status, StoreReport, report_entry, write_atomic, write_report_at,
};
use std::path::{Path, PathBuf};
use tuiscotti_core::frame::Frame;
use tuiscotti_render::diff;
use tuiscotti_render::profile::Profile;
use tuiscotti_render::render::{self, Renderer};

/// `root/<name>.<ext>` for the four artifacts plus the frame sidecar.
/// `name` must be pre-validated ([`validate_name`]).
pub(crate) fn artifact_paths(root: &Path, name: &str) -> ArtifactPaths {
    ArtifactPaths {
        ansi: root.join(format!("{name}.ansi")),
        txt: root.join(format!("{name}.txt")),
        png: root.join(format!("{name}.png")),
        html: root.join(format!("{name}.html")),
        frame_json: root.join(format!("{name}.frame.json")),
    }
}

/// Missing-glyph sidecar next to a PNG (`<name>.png.fidelity.json`).
fn fidelity_sidecar(png: &Path) -> PathBuf {
    png.with_extension("png.fidelity.json")
}

pub(crate) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, SnapshotError> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(SnapshotError(format!(
            "cannot read {}: {e}",
            path.display()
        ))),
    }
}

impl GroupedStore {
    /// Check one frame against the approved artifacts. Writes the four
    /// actual artifacts (plus debug sidecars) under the actual root BEFORE
    /// comparing; on pixel mismatch also writes the diff PNG under the diff
    /// root. Gate semantics are in the module docs.
    ///
    /// This constructs a fresh [`Renderer`] per call; bulk gates should
    /// build one and call [`Self::check_with`] instead.
    pub fn check(
        &self,
        name: &str,
        actual: &Frame,
        profile: &Profile,
        faces: &tuiscotti_render::profile::FontFaces<'_>,
        pixel_threshold: f64,
    ) -> Result<GroupedOutcome, SnapshotError> {
        let mut renderer = Renderer::new(profile, faces)?;
        self.check_with(&mut renderer, name, actual, pixel_threshold)
    }

    /// [`Self::check`] through a caller-owned [`Renderer`], so a suite
    /// reuses the parsed faces and the glyph cache across checks. PNG/HTML
    /// always render fresh from the candidate frame (C02).
    pub fn check_with(
        &self,
        renderer: &mut Renderer,
        name: &str,
        actual: &Frame,
        pixel_threshold: f64,
    ) -> Result<GroupedOutcome, SnapshotError> {
        let cheap = self.write_cheap_actuals(name, actual)?;
        let mut grouped = fresh_grouped(name, &cheap, actual);

        // Missing ANY approved artifact fails closed as MissingApproval.
        let approved_ansi = read_optional(&grouped.approved.ansi)?;
        let approved_txt = read_optional(&grouped.approved.txt)?;
        let approved_html = read_optional(&grouped.approved.html)?;
        let approved_png = read_optional(&grouped.approved.png)?;
        let missing = missing_approved_names(
            approved_ansi.is_none(),
            approved_txt.is_none(),
            approved_html.is_none(),
            approved_png.is_none(),
        );
        if !missing.is_empty() {
            self.seal_missing_approval(
                renderer,
                name,
                actual,
                &mut grouped,
                pixel_threshold,
                &missing,
            )?;
            return Ok(grouped);
        }
        let (Some(approved_ansi), Some(approved_txt), Some(approved_html), Some(approved_png)) =
            (approved_ansi, approved_txt, approved_html, approved_png)
        else {
            return Err(SnapshotError(format!(
                "approved artifacts for `{name}` failed the presence check twice"
            )));
        };
        grouped.outcome.expected_png = Some(grouped.approved.png.clone());
        grouped.outcome.expected_png_bytes = Some(approved_png.clone());

        let mut notes: Vec<String> = Vec::new();
        run_byte_gates(
            &mut grouped,
            &approved_ansi,
            &cheap.ansi,
            &approved_txt,
            &cheap.txt,
            &mut notes,
        );

        // C02: actual evidence always renders fresh from the candidate frame.
        // The old tiered fast path copied approved PNG/HTML bytes into
        // actual/ when the cell gates passed — fabricating html_match=true
        // and a pixel score of 1.0 without rendering, and masking
        // approved-side tamper. There is no skip-render path anymore, so
        // there is nothing to mark not-checked: every tier renders.
        let (actual_html_bytes, actual_png_bytes) =
            self.render_actual_artifacts(renderer, name, actual, &grouped.actual)?;
        run_html_gate(&mut grouped, &approved_html, &actual_html_bytes, &mut notes);
        self.run_png_gate(
            name,
            &mut grouped,
            &approved_png,
            &actual_png_bytes,
            pixel_threshold,
            &mut notes,
        )?;

        self.finalize_check(renderer, name, &mut grouped, pixel_threshold, notes)?;
        Ok(grouped)
    }

    /// Validate, render the cheap actuals (ansi/txt/frame), and write them.
    /// A failing gate still leaves reviewable evidence.
    fn write_cheap_actuals(
        &self,
        name: &str,
        actual: &Frame,
    ) -> Result<CheapActuals, SnapshotError> {
        validate_name(name)?;
        actual.validate().map_err(SnapshotError::from)?;
        let actual_ansi = render::ansi_dump(actual);
        let actual_txt = actual.text();
        let actual_paths = artifact_paths(&self.actual_root, name);
        let approved_paths = artifact_paths(&self.approved_root, name);
        write_atomic(&actual_paths.ansi, actual_ansi.as_bytes())?;
        write_atomic(&actual_paths.txt, actual_txt.as_bytes())?;
        write_atomic(&actual_paths.frame_json, actual.to_json().as_bytes())?;
        Ok(CheapActuals {
            ansi: actual_ansi,
            txt: actual_txt,
            actual: actual_paths,
            approved: approved_paths,
        })
    }

    /// Render the expensive actuals (png/html/fidelity) fresh from the
    /// candidate frame and write them. Returns `(html_bytes, png_bytes)`.
    fn render_actual_artifacts(
        &self,
        renderer: &mut Renderer,
        name: &str,
        actual: &Frame,
        grouped_actual: &ArtifactPaths,
    ) -> Result<(Vec<u8>, Vec<u8>), SnapshotError> {
        let artifacts = renderer
            .render_artifacts(actual, name)
            .map_err(SnapshotError::from)?;
        write_atomic(&grouped_actual.png, &artifacts.png)?;
        write_atomic(&grouped_actual.html, artifacts.html.as_bytes())?;
        write_atomic(
            &fidelity_sidecar(&grouped_actual.png),
            artifacts.fidelity.to_json().as_bytes(),
        )?;
        Ok((artifacts.html.into_bytes(), artifacts.png))
    }

    /// Missing-approval path: render fresh actuals anyway (reviewable
    /// evidence), note the gap, and seal so reports reuse this verdict.
    fn seal_missing_approval(
        &self,
        renderer: &mut Renderer,
        name: &str,
        actual: &Frame,
        grouped: &mut GroupedOutcome,
        pixel_threshold: f64,
        missing: &[&str],
    ) -> Result<(), SnapshotError> {
        let artifacts = renderer
            .render_artifacts(actual, name)
            .map_err(SnapshotError::from)?;
        write_atomic(&grouped.actual.png, &artifacts.png)?;
        write_atomic(&grouped.actual.html, artifacts.html.as_bytes())?;
        write_atomic(
            &fidelity_sidecar(&grouped.actual.png),
            artifacts.fidelity.to_json().as_bytes(),
        )?;
        grouped.outcome.note = format!(
            "missing approved artifact(s) for `{name}`: {}",
            missing.join(" ")
        );
        // C06-mirror: the candidate above is rendered fresh from the
        // actual frame — no approved bytes are copied into actual/ — and
        // the gate fails closed. Seal so reports reuse this verdict.
        let profile_name = renderer.profile().name.clone();
        self.seal_candidate(&profile_name, name, &grouped.actual)?;
        self.seal_verdict(name, grouped, pixel_threshold, &["approved-presence"])?;
        Ok(())
    }

    /// PNG pixel gate: decoded pixels, same threshold semantics as the
    /// classic store. A corrupt approved PNG is an explicit error.
    fn run_png_gate(
        &self,
        name: &str,
        grouped: &mut GroupedOutcome,
        approved_png: &[u8],
        actual_png_bytes: &[u8],
        pixel_threshold: f64,
        notes: &mut Vec<String>,
    ) -> Result<(), SnapshotError> {
        let outcome = &mut grouped.outcome;
        let verdict = diff::compare_png(approved_png, actual_png_bytes)?;
        if !verdict.dims_equal {
            outcome.status = Status::DimensionMismatch;
            notes.push(format!(
                "png dimensions differ: approved {:?}, actual {:?}",
                verdict.expected_dims, verdict.actual_dims
            ));
        } else {
            outcome.pixel_score = Some(verdict.score);
            if verdict.score < pixel_threshold
                && !matches!(
                    outcome.status,
                    Status::CellsDiffer | Status::DimensionMismatch
                )
            {
                outcome.status = Status::PixelsDiffer;
            }
            if verdict.score < 1.0 {
                let path = sibling_diff(&self.diff_root, name);
                write_atomic(&path, &verdict.diff_png)?;
                outcome.diff_png = Some(path);
            }
        }
        Ok(())
    }

    /// Flip a clean run to `Matched`, join the notes, and seal the
    /// candidate file set plus this exact verdict (C05/C08-grouped).
    fn finalize_check(
        &self,
        renderer: &Renderer,
        name: &str,
        grouped: &mut GroupedOutcome,
        pixel_threshold: f64,
        notes: Vec<String>,
    ) -> Result<(), SnapshotError> {
        if matches!(grouped.outcome.status, Status::MissingApproval) {
            grouped.outcome.status = Status::Matched;
        }
        grouped.outcome.note = notes.join("; ");
        // C05/C08-grouped: seal the candidate file set, then persist this
        // exact verdict — the ONE verdict reports reuse when fresh.
        let profile_name = renderer.profile().name.clone();
        self.seal_candidate(&profile_name, name, &grouped.actual)?;
        self.seal_verdict(
            name,
            grouped,
            pixel_threshold,
            &[
                "ansi-byte-gate",
                "txt-byte-gate",
                "html-byte-gate",
                "png-pixel-gate",
            ],
        )
    }
}

/// Diff PNG path of one scenario under the diff root (`<name>.png`).
pub(crate) fn sibling_diff(diff_root: &Path, name: &str) -> PathBuf {
    diff_root.join(format!("{name}.png"))
}
