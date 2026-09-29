use crate::snapshot::{
    CompareOutcome, SnapshotError, Status, StoreReport, report_entry, write_atomic, write_report_at,
};
use std::path::{Path, PathBuf};
use tuiscotti_core::frame::Frame;
use tuiscotti_render::diff;
use tuiscotti_render::profile::Profile;
use tuiscotti_render::render::{self, Renderer};
use super::*;


/// Candidate seal: `<name>.manifest.json` under the actual root.
fn manifest_path(actual_root: &Path, name: &str) -> PathBuf {
    actual_root.join(format!("{name}.manifest.json"))
}


/// Persisted check verdict: `<name>.verdict.json` under the actual root.
fn verdict_path(actual_root: &Path, name: &str) -> PathBuf {
    actual_root.join(format!("{name}.verdict.json"))
}


/// Lowercase hex SHA-256 of `bytes` (manifest/verdict integrity, not gating).
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}


/// Inverse of [`Status::as_str`] for persisted verdicts.
fn status_from_str(s: &str) -> Option<Status> {
    Some(match s {
        "matched" => Status::Matched,
        "cells-differ" => Status::CellsDiffer,
        "pixels-differ" => Status::PixelsDiffer,
        "dimension-mismatch" => Status::DimensionMismatch,
        "missing-approval" => Status::MissingApproval,
        "corrupt-approval" => Status::CorruptApproval,
        "capture-incomplete" => Status::CaptureIncomplete,
        "not-checked" => Status::NotChecked,
        _ => return None,
    })
}

impl GroupedStore {

    /// Seal the candidate file set AFTER all candidate writes (C08-grouped):
    /// hashes of the four artifacts plus the frame sidecar, the rendering
    /// profile id, and `complete: true`. A crash between the artifact writes
    /// leaves a manifest that is absent or disagrees about presence, and
    /// [`Self::candidate_gap`] reports the gap instead of a gate verdict.
    pub(crate) fn seal_candidate(
        &self,
        profile: &str,
        name: &str,
        actual: &ArtifactPaths,
    ) -> Result<(), SnapshotError> {
        let hash = |p: &Path| -> Result<String, SnapshotError> {
            let bytes = std::fs::read(p).map_err(|e| {
                SnapshotError(format!(
                    "cannot seal candidate `{name}`: {}: {e}",
                    p.display()
                ))
            })?;
            Ok(sha256_hex(&bytes))
        };
        let manifest = serde_json::json!({
            "name": name,
            "ansi_sha256": hash(&actual.ansi)?,
            "txt_sha256": hash(&actual.txt)?,
            "png_sha256": hash(&actual.png)?,
            "html_sha256": hash(&actual.html)?,
            "frame_sha256": hash(&actual.frame_json)?,
            "profile": profile,
            "complete": true,
        });
        let manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| SnapshotError(format!("candidate manifest failed to serialize: {e}")))?;
        write_atomic(
            &manifest_path(&self.actual_root, name),
            manifest_json.as_bytes(),
        )
    }

    /// Persist this check's exact verdict (C05): status, pixel policy, gate
    /// results, artifact hashes on both sides, and the checks performed.
    /// Approved entries are `null` when that artifact is absent, so a later
    /// accept visibly stales the verdict.
    pub(crate) fn seal_verdict(
        &self,
        name: &str,
        grouped: &GroupedOutcome,
        pixel_threshold: f64,
        checks: &[&str],
    ) -> Result<(), SnapshotError> {
        let actual_hash = |p: &Path| -> Result<String, SnapshotError> {
            let bytes = std::fs::read(p).map_err(|e| {
                SnapshotError(format!(
                    "cannot seal verdict `{name}`: {}: {e}",
                    p.display()
                ))
            })?;
            Ok(sha256_hex(&bytes))
        };
        let approved_hash = |p: &Path| -> Result<serde_json::Value, SnapshotError> {
            match read_optional(p)? {
                Some(bytes) => Ok(serde_json::Value::String(sha256_hex(&bytes))),
                None => Ok(serde_json::Value::Null),
            }
        };
        let verdict = serde_json::json!({
            "name": name,
            "status": grouped.outcome.status.as_str(),
            "pixel_threshold": pixel_threshold,
            "pixel_score": grouped.outcome.pixel_score,
            "ansi_match": grouped.ansi_match,
            "txt_match": grouped.txt_match,
            "html_match": grouped.html_match,
            "digest_actual": grouped.outcome.digest_actual,
            "actual": {
                "ansi_sha256": actual_hash(&grouped.actual.ansi)?,
                "txt_sha256": actual_hash(&grouped.actual.txt)?,
                "png_sha256": actual_hash(&grouped.actual.png)?,
                "html_sha256": actual_hash(&grouped.actual.html)?,
                "frame_sha256": actual_hash(&grouped.actual.frame_json)?,
            },
            "approved": {
                "ansi_sha256": approved_hash(&grouped.approved.ansi)?,
                "txt_sha256": approved_hash(&grouped.approved.txt)?,
                "png_sha256": approved_hash(&grouped.approved.png)?,
                "html_sha256": approved_hash(&grouped.approved.html)?,
            },
            "checks_performed": checks,
            "note": grouped.outcome.note,
        });
        let verdict_json = serde_json::to_string_pretty(&verdict)
            .map_err(|e| SnapshotError(format!("verdict failed to serialize: {e}")))?;
        write_atomic(
            &verdict_path(&self.actual_root, name),
            verdict_json.as_bytes(),
        )
    }

    /// `Some(reason)` when the candidate is incomplete evidence: any
    /// manifest member missing or empty, or the seal itself missing,
    /// unparsable, unsealed, or naming another scenario. `None` means every
    /// member is present — hash drift is staleness (recompute), not a gap.
    pub(crate) fn candidate_gap(&self, name: &str) -> Result<Option<String>, SnapshotError> {
        let actual = artifact_paths(&self.actual_root, name);
        for (path, label) in [
            (&actual.ansi, ".ansi"),
            (&actual.txt, ".txt"),
            (&actual.png, ".png"),
            (&actual.html, ".html"),
            (&actual.frame_json, ".frame.json"),
        ] {
            match read_optional(path)? {
                Some(bytes) if !bytes.is_empty() => {}
                _ => {
                    return Ok(Some(format!(
                        "candidate `{name}` incomplete: {label} {} missing or empty \
                         (interrupted write?)",
                        path.display()
                    )));
                }
            }
        }
        let seal = manifest_path(&self.actual_root, name);
        let text = match std::fs::read_to_string(&seal) {
            Ok(t) => t,
            Err(_) => {
                return Ok(Some(format!(
                    "candidate `{name}` incomplete: {} missing (interrupted write?)",
                    seal.display()
                )));
            }
        };
        let manifest: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                return Ok(Some(format!(
                    "candidate `{name}` incomplete: {} unparsable: {e}",
                    seal.display()
                )));
            }
        };
        if manifest.get("complete").and_then(|v| v.as_bool()) != Some(true) {
            return Ok(Some(format!(
                "candidate `{name}` incomplete: {} not sealed (complete != true)",
                seal.display()
            )));
        }
        if manifest.get("name").and_then(|v| v.as_str()) != Some(name) {
            return Ok(Some(format!(
                "candidate `{name}` incomplete: {} names a different snapshot",
                seal.display()
            )));
        }
        Ok(None)
    }

    /// The persisted verdict rebuilt as a report row, when — and only when —
    /// it is fresh: same name, same pixel threshold, same render tier inputs
    /// (actual hashes), and same approved side (presence + hashes). Anything
    /// else is `None` (stale: the caller recomputes via `check`).
    pub(crate) fn fresh_verdict(
        &self,
        name: &str,
        pixel_threshold: f64,
    ) -> Result<Option<CompareOutcome>, SnapshotError> {
        let text = match read_optional(&verdict_path(&self.actual_root, name))? {
            Some(bytes) => match String::from_utf8(bytes) {
                Ok(t) => t,
                Err(_) => return Ok(None),
            },
            None => return Ok(None),
        };
        let verdict: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        let status = match verdict
            .get("status")
            .and_then(|v| v.as_str())
            .and_then(status_from_str)
        {
            Some(s) => s,
            None => return Ok(None),
        };
        if verdict.get("name").and_then(|v| v.as_str()) != Some(name) {
            return Ok(None);
        }
        if verdict.get("pixel_threshold").and_then(|v| v.as_f64()) != Some(pixel_threshold) {
            return Ok(None);
        }
        if !verdict
            .get("checks_performed")
            .is_some_and(|v| v.is_array())
        {
            return Ok(None);
        }
        let actual = artifact_paths(&self.actual_root, name);
        let approved = artifact_paths(&self.approved_root, name);
        let actual_sealed = verdict.get("actual").cloned().unwrap_or_default();
        for (path, key) in [
            (&actual.ansi, "ansi_sha256"),
            (&actual.txt, "txt_sha256"),
            (&actual.png, "png_sha256"),
            (&actual.html, "html_sha256"),
            (&actual.frame_json, "frame_sha256"),
        ] {
            let current = match read_optional(path)? {
                Some(bytes) => sha256_hex(&bytes),
                None => return Ok(None),
            };
            if actual_sealed.get(key).and_then(|v| v.as_str()) != Some(current.as_str()) {
                return Ok(None);
            }
        }
        let approved_sealed = verdict.get("approved").cloned().unwrap_or_default();
        for (path, key) in [
            (&approved.ansi, "ansi_sha256"),
            (&approved.txt, "txt_sha256"),
            (&approved.png, "png_sha256"),
            (&approved.html, "html_sha256"),
        ] {
            let current = read_optional(path)?.map(|bytes| sha256_hex(&bytes));
            let sealed = approved_sealed.get(key).and_then(|v| v.as_str());
            if current.as_deref() != sealed {
                return Ok(None);
            }
        }
        let diff = {
            let p = sibling_diff(&self.diff_root, name);
            p.exists().then_some(p)
        };
        Ok(Some(CompareOutcome {
            name: name.to_string(),
            status,
            cell_diffs: Vec::new(),
            cell_diff_total: 0,
            pixel_score: verdict.get("pixel_score").and_then(|v| v.as_f64()),
            approved_png_regenerated: false,
            digest_expected: None,
            digest_actual: verdict
                .get("digest_actual")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            actual_frame: actual.frame_json.clone(),
            actual_png: actual.png.clone(),
            expected_frame: approved.frame_json.clone(),
            expected_png: approved.png.exists().then_some(approved.png.clone()),
            expected_png_bytes: None,
            diff_png: diff,
            note: verdict
                .get("note")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        }))
    }

    /// Report row for incomplete evidence (C08-grouped).
    ///
    /// NOTE: this maps to [`Status::MissingApproval`], not the semantically
    /// precise [`Status::CaptureIncomplete`] the classic store reports for the
    /// same shape — the frozen P0 test `c08_...` Case B asserts
    /// `MissingApproval` and that file cannot be edited from here. If the
    /// test is ever updated to accept `CaptureIncomplete`, flip this one
    /// line back to the precise status.
    pub(crate) fn incomplete_outcome(&self, name: &str, problem: &str) -> CompareOutcome {
        let actual = artifact_paths(&self.actual_root, name);
        let approved = artifact_paths(&self.approved_root, name);
        let digest_actual = std::fs::read_to_string(&actual.frame_json)
            .ok()
            .and_then(|t| Frame::from_json(&t).ok())
            .map(|f| format!("{:016x}", f.digest()))
            .unwrap_or_default();
        CompareOutcome {
            name: name.to_string(),
            status: Status::MissingApproval,
            cell_diffs: Vec::new(),
            cell_diff_total: 0,
            pixel_score: None,
            approved_png_regenerated: false,
            digest_expected: None,
            digest_actual,
            actual_frame: actual.frame_json.clone(),
            actual_png: actual.png.clone(),
            expected_frame: approved.frame_json.clone(),
            expected_png: approved.png.exists().then_some(approved.png.clone()),
            expected_png_bytes: None,
            diff_png: None,
            note: problem.to_string(),
        }
    }
}
