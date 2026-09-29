use crate::snapshot::{
    CompareOutcome, SnapshotError, Status, StoreReport, report_entry, write_atomic, write_report_at,
};
use std::path::{Path, PathBuf};
use tuiscotti_core::frame::Frame;
use tuiscotti_render::diff;
use tuiscotti_render::profile::Profile;
use tuiscotti_render::render::{self, Renderer};
use super::*;


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


/// Locate the first differing byte of two blobs, for human diagnostics.
fn first_difference(approved: &[u8], actual: &[u8]) -> String {
    let n = approved.len().min(actual.len());
    let mut i = 0;
    while i < n && approved[i] == actual[i] {
        i += 1;
    }
    let line = approved[..i].iter().filter(|&&c| c == b'\n').count() + 1;
    format!(
        "first difference at byte {i} (approved line {line}); approved {} bytes, actual {} bytes",
        approved.len(),
        actual.len()
    )
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
        validate_name(name)?;
        actual.validate().map_err(SnapshotError::from)?;

        let actual_ansi = render::ansi_dump(actual);
        let actual_txt = actual.text();

        let actual_paths = artifact_paths(&self.actual_root, name);
        let approved_paths = artifact_paths(&self.approved_root, name);
        // Cheap actuals first: a failing gate still leaves reviewable evidence.
        write_atomic(&actual_paths.ansi, actual_ansi.as_bytes())?;
        write_atomic(&actual_paths.txt, actual_txt.as_bytes())?;
        write_atomic(&actual_paths.frame_json, actual.to_json().as_bytes())?;

        let outcome = CompareOutcome {
            name: name.to_string(),
            status: Status::MissingApproval,
            cell_diffs: Vec::new(),
            cell_diff_total: 0,
            pixel_score: None,
            approved_png_regenerated: false,
            digest_expected: None,
            digest_actual: format!("{:016x}", actual.digest()),
            actual_frame: actual_paths.frame_json.clone(),
            actual_png: actual_paths.png.clone(),
            // Never exists in a conforming approved tree (four artifacts
            // only); reports simply omit the expected-frame panel.
            expected_frame: approved_paths.frame_json.clone(),
            expected_png: None,
            expected_png_bytes: None,
            diff_png: None,
            note: String::new(),
        };
        let mut grouped = GroupedOutcome {
            outcome,
            ansi_match: None,
            txt_match: None,
            html_match: None,
            actual: actual_paths,
            approved: approved_paths,
        };
        let outcome = &mut grouped.outcome;

        // Missing ANY approved artifact fails closed as MissingApproval.
        let approved_ansi = read_optional(&grouped.approved.ansi)?;
        let approved_txt = read_optional(&grouped.approved.txt)?;
        let approved_html = read_optional(&grouped.approved.html)?;
        let approved_png = read_optional(&grouped.approved.png)?;
        let mut missing = Vec::new();
        if approved_ansi.is_none() {
            missing.push(".ansi");
        }
        if approved_txt.is_none() {
            missing.push(".txt");
        }
        if approved_html.is_none() {
            missing.push(".html");
        }
        if approved_png.is_none() {
            missing.push(".png");
        }
        if !missing.is_empty() {
            let artifacts = renderer
                .render_artifacts(actual, name)
                .map_err(SnapshotError::from)?;
            write_atomic(&grouped.actual.png, &artifacts.png)?;
            write_atomic(&grouped.actual.html, artifacts.html.as_bytes())?;
            write_atomic(
                &fidelity_sidecar(&grouped.actual.png),
                artifacts.fidelity.to_json().as_bytes(),
            )?;
            outcome.note = format!(
                "missing approved artifact(s) for `{name}`: {}",
                missing.join(" ")
            );
            // C06-mirror: the candidate above is rendered fresh from the
            // actual frame — no approved bytes are copied into actual/ — and
            // the gate fails closed. Seal so reports reuse this verdict.
            let profile_name = renderer.profile().name.clone();
            self.seal_candidate(&profile_name, name, &grouped.actual)?;
            self.seal_verdict(name, &grouped, pixel_threshold, &["approved-presence"])?;
            return Ok(grouped);
        }
        let (Some(approved_ansi), Some(approved_txt), Some(approved_html), Some(approved_png)) =
            (approved_ansi, approved_txt, approved_html, approved_png)
        else {
            return Err(SnapshotError(format!(
                "approved artifacts for `{name}` failed the presence check twice"
            )));
        };
        outcome.expected_png = Some(grouped.approved.png.clone());
        outcome.expected_png_bytes = Some(approved_png.clone());

        let mut notes: Vec<String> = Vec::new();

        // ANSI byte gate: the cell-exact comparison (symbol+fg+bg+mods).
        let ansi_equal = approved_ansi.as_slice() == actual_ansi.as_bytes();
        grouped.ansi_match = Some(ansi_equal);
        if !ansi_equal {
            outcome.status = Status::CellsDiffer;
            notes.push(format!(
                "ansi differs (cell-exact gate): {}",
                first_difference(&approved_ansi, actual_ansi.as_bytes())
            ));
        }

        // TXT byte gate: content only (style-only changes keep txt equal).
        let txt_equal = approved_txt.as_slice() == actual_txt.as_bytes();
        grouped.txt_match = Some(txt_equal);
        if !txt_equal {
            if matches!(outcome.status, Status::MissingApproval) {
                outcome.status = Status::CellsDiffer;
            }
            notes.push(format!(
                "txt differs: {}",
                first_difference(&approved_txt, actual_txt.as_bytes())
            ));
        }

        // C02: actual evidence always renders fresh from the candidate frame.
        // The old tiered fast path copied approved PNG/HTML bytes into
        // actual/ when the cell gates passed — fabricating html_match=true
        // and a pixel score of 1.0 without rendering, and masking
        // approved-side tamper. There is no skip-render path anymore, so
        // there is nothing to mark not-checked: every tier renders.
        let artifacts = renderer
            .render_artifacts(actual, name)
            .map_err(SnapshotError::from)?;
        write_atomic(&grouped.actual.png, &artifacts.png)?;
        write_atomic(&grouped.actual.html, artifacts.html.as_bytes())?;
        write_atomic(
            &fidelity_sidecar(&grouped.actual.png),
            artifacts.fidelity.to_json().as_bytes(),
        )?;
        let (actual_html_bytes, actual_png_bytes) = (artifacts.html.into_bytes(), artifacts.png);

        // HTML byte gate: identical cells with a changed renderer/font fail
        // here — a render-level event, reported as PixelsDiffer.
        let html_equal = approved_html == actual_html_bytes;
        grouped.html_match = Some(html_equal);
        if !html_equal {
            if matches!(outcome.status, Status::MissingApproval) {
                outcome.status = Status::PixelsDiffer;
            }
            notes.push(format!(
                "html differs (render-level gate): {}",
                first_difference(&approved_html, &actual_html_bytes)
            ));
        }

        // PNG pixel gate: decoded pixels, same threshold semantics as the
        // classic store. A corrupt approved PNG is an explicit error.
        let verdict = diff::compare_png(&approved_png, &actual_png_bytes)?;
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

        if matches!(outcome.status, Status::MissingApproval) {
            outcome.status = Status::Matched;
        }
        outcome.note = notes.join("; ");
        // C05/C08-grouped: seal the candidate file set, then persist this
        // exact verdict — the ONE verdict reports reuse when fresh.
        let profile_name = renderer.profile().name.clone();
        self.seal_candidate(&profile_name, name, &grouped.actual)?;
        self.seal_verdict(
            name,
            &grouped,
            pixel_threshold,
            &[
                "ansi-byte-gate",
                "txt-byte-gate",
                "html-byte-gate",
                "png-pixel-gate",
            ],
        )?;
        Ok(grouped)
    }
}


/// Diff PNG path of one scenario under the diff root (`<name>.png`).
pub(crate) fn sibling_diff(diff_root: &Path, name: &str) -> PathBuf {
    diff_root.join(format!("{name}.png"))
}
