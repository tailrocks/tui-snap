use super::{GroupedStore, artifact_paths, validate_name};
use crate::snapshot::{
    CompareOutcome, SnapshotError, StoreReport, report_entry, write_atomic, write_report_at,
};
use tuiscotti_core::frame::Frame;
use tuiscotti_render::profile::Profile;
use tuiscotti_render::render::Renderer;

impl GroupedStore {
    /// Explicitly approve one scenario: the four actual artifacts replace
    /// the approved ones (atomic per file). Sidecars stay in the scratch
    /// area — the approved tree holds the four artifacts and nothing else.
    /// There is deliberately no environment-variable auto-accept.
    ///
    /// # Errors
    ///
    /// Returns `SnapshotError` when actual artifacts are missing or writes fail.
    pub fn accept(&self, name: &str) -> Result<(), SnapshotError> {
        validate_name(name)?;
        let actual = artifact_paths(&self.actual_root, name);
        let approved = artifact_paths(&self.approved_root, name);
        for (src, dst, label) in [
            (&actual.ansi, &approved.ansi, ".ansi"),
            (&actual.txt, &approved.txt, ".txt"),
            (&actual.png, &approved.png, ".png"),
            (&actual.html, &approved.html, ".html"),
        ] {
            let bytes = std::fs::read(src).map_err(|e| {
                SnapshotError(format!(
                    "nothing to accept for `{name}` ({label} {}: {e})",
                    src.display()
                ))
            })?;
            write_atomic(dst, &bytes)?;
        }
        Ok(())
    }

    /// Accept every scenario with actual artifacts, recursively. Returns the
    /// accepted names (sorted).
    ///
    /// # Errors
    ///
    /// Returns `SnapshotError` when listing actuals or any accept fails.
    pub fn accept_all(&self) -> Result<Vec<String>, SnapshotError> {
        let names = self.actual_names()?;
        for name in &names {
            self.accept(name)?;
        }
        Ok(names)
    }

    /// Rewrite the HTML review index from on-disk actual vs approved
    /// artifacts. Does not re-render. Unmatched gates do not error here:
    /// inspect [`StoreReport::failed`] and the outcomes.
    ///
    /// # Errors
    ///
    /// Returns `SnapshotError` when rendering or writing the report fails.
    pub fn report(
        &self,
        profile: &Profile,
        faces: &tuiscotti_render::profile::FontFaces<'_>,
        pixel_threshold: f64,
        title: &str,
    ) -> Result<StoreReport, SnapshotError> {
        Renderer::with_profile(profile, faces, |r| {
            self.report_with(r, pixel_threshold, title)
        })
    }

    /// Review index over the ONE persisted verdict per scenario (C05):
    /// a verdict is reused only when its artifact hashes still match the
    /// files on disk and the pixel threshold matches; anything stale is
    /// recomputed via `check`, never silently reused — so report status
    /// equals check status on the same inputs. Candidates with a missing
    /// manifest member report [`crate::snapshot::Status::MissingApproval`] (C08-grouped),
    /// never a pixel verdict. HTML links the PNG files; it does not embed
    /// them.
    ///
    /// # Errors
    ///
    /// Returns `SnapshotError` when rendering or writing the report fails.
    pub fn report_with(
        &self,
        renderer: &mut Renderer,
        pixel_threshold: f64,
        title: &str,
    ) -> Result<StoreReport, SnapshotError> {
        let names = self.actual_names()?;
        let mut entries = Vec::new();
        let mut outcomes = Vec::new();
        let profile = renderer.profile().clone();
        for name in names {
            let outcome = self.report_outcome(renderer, &name, pixel_threshold)?;
            entries.push(report_entry(&outcome, &profile)?);
            outcomes.push(outcome);
        }
        let path = write_report_at(&self.report_path(), title, &entries)?;
        Ok(StoreReport { path, outcomes })
    }

    /// One report row: incomplete evidence first, then the persisted verdict
    /// when fresh, else a genuine recompute through `check_with`.
    fn report_outcome(
        &self,
        renderer: &mut Renderer,
        name: &str,
        pixel_threshold: f64,
    ) -> Result<CompareOutcome, SnapshotError> {
        // C08-grouped: a missing manifest member is incomplete evidence, not
        // a gate verdict. (Hash drift with all members present is NOT
        // incomplete — it is stale, and falls through to recompute below.)
        if let Some(problem) = self.candidate_gap(name)? {
            return Ok(self.incomplete_outcome(name, &problem));
        }
        // C05: fresh persisted verdict — reuse verbatim.
        if let Some(outcome) = self.fresh_verdict(name, pixel_threshold)? {
            return Ok(outcome);
        }
        // Stale or absent verdict (accept/overwrite moved the bytes, or the
        // threshold changed): recompute via check, never silently reuse.
        let frame_path = artifact_paths(&self.actual_root, name).frame_json;
        let text = std::fs::read_to_string(&frame_path)
            .map_err(|e| SnapshotError(format!("cannot read {}: {e}", frame_path.display())))?;
        let frame = Frame::from_json(&text)?;
        let grouped = self.check_with(renderer, name, &frame, pixel_threshold)?;
        Ok(grouped.outcome)
    }
}
