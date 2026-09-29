use super::*;
use std::path::{Path, PathBuf};
use tuiscotti_core::frame::{Frame, FrameError};
use tuiscotti_render::diff;
use tuiscotti_render::profile::Profile;
use tuiscotti_render::render;

impl Store {
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
        faces: &tuiscotti_render::profile::FontFaces<'_>,
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
