use super::*;
use crate::snapshot::{
    CompareOutcome, SnapshotError, Status, StoreReport, report_entry, write_atomic, write_report_at,
};
use std::path::{Path, PathBuf};
use tuiscotti_core::frame::Frame;
use tuiscotti_render::diff;
use tuiscotti_render::profile::Profile;
use tuiscotti_render::render::{self, Renderer};

impl From<InvalidName> for SnapshotError {
    fn from(e: InvalidName) -> Self {
        SnapshotError(e.to_string())
    }
}

/// The on-disk artifact paths of one scenario under one root.
#[derive(Debug, Clone)]
pub struct ArtifactPaths {
    pub ansi: PathBuf,
    pub txt: PathBuf,
    pub png: PathBuf,
    pub html: PathBuf,
    /// Canonical frame sidecar (scratch roots only — never approved state).
    pub frame_json: PathBuf,
}

/// Outcome of one grouped [`GroupedStore::check_with`]. The shared
/// [`CompareOutcome`] carries status, pixel score, diff path and the paths
/// the report machinery reads; the per-artifact booleans record each byte
/// gate individually (`None` = the approved artifact is missing).
#[derive(Debug, Clone)]
pub struct GroupedOutcome {
    pub outcome: CompareOutcome,
    /// `.ansi` byte gate (the cell-exact gate).
    pub ansi_match: Option<bool>,
    /// `.txt` byte gate.
    pub txt_match: Option<bool>,
    /// `.html` byte gate (the render-level gate).
    pub html_match: Option<bool>,
    /// Where the actual artifacts were written.
    pub actual: ArtifactPaths,
    /// Where the approved artifacts live (some may not exist yet).
    pub approved: ArtifactPaths,
}

impl GroupedOutcome {
    pub fn status(&self) -> Status {
        self.outcome.status
    }

    #[must_use]
    pub fn matched(&self) -> bool {
        self.outcome.status.matched()
    }

    /// Fail with an actionable message (artifact paths + which gates fell).
    pub fn ensure_matched(&self) -> Result<(), SnapshotError> {
        self.outcome.ensure_matched()
    }
}

/// Grouped approved/actual/diff artifact store. See the module docs for the
/// layout and gate semantics.
#[derive(Debug, Clone)]
pub struct GroupedStore {
    pub(crate) approved_root: PathBuf,
    pub(crate) actual_root: PathBuf,
    pub(crate) diff_root: PathBuf,
    pub(crate) report_path: Option<PathBuf>,
}

/// `root` with `suffix` appended to its last component
/// (`snapshots` + `.actual` → `snapshots.actual`).
fn sibling(root: &Path, suffix: &str) -> PathBuf {
    match root.file_name() {
        Some(f) => root.with_file_name(format!("{}{suffix}", f.to_string_lossy())),
        None => PathBuf::from(format!("{}{suffix}", root.display())),
    }
}

impl GroupedStore {
    /// Approved root `snapshots/` defaults to actual `snapshots.actual/`,
    /// diff `snapshots.diff/`, report `snapshots.actual/report.html`.
    #[must_use]
    pub fn new(approved_root: &Path) -> Self {
        Self {
            approved_root: approved_root.to_path_buf(),
            actual_root: sibling(approved_root, ".actual"),
            diff_root: sibling(approved_root, ".diff"),
            report_path: None,
        }
    }

    #[must_use]
    pub fn with_actual_root(mut self, root: &Path) -> Self {
        self.actual_root = root.to_path_buf();
        self
    }

    #[must_use]
    pub fn with_diff_root(mut self, root: &Path) -> Self {
        self.diff_root = root.to_path_buf();
        self
    }

    /// Explicit report path. Consumers typically point this under `target/`
    /// or the store scratch area — never inside the approved tree.
    #[must_use]
    pub fn with_report_path(mut self, path: &Path) -> Self {
        self.report_path = Some(path.to_path_buf());
        self
    }

    #[must_use]
    pub fn approved_root(&self) -> &Path {
        &self.approved_root
    }

    #[must_use]
    pub fn actual_root(&self) -> &Path {
        &self.actual_root
    }

    #[must_use]
    pub fn diff_root(&self) -> &Path {
        &self.diff_root
    }

    /// The configured report path, defaulting to `<actual_root>/report.html`.
    #[must_use]
    pub fn report_path(&self) -> PathBuf {
        self.report_path
            .clone()
            .unwrap_or_else(|| self.actual_root.join("report.html"))
    }

    /// Scenario names with actual artifacts, recursively, sorted. An absent
    /// actual root lists nothing (a fresh store is empty, not an error).
    pub fn actual_names(&self) -> Result<Vec<String>, SnapshotError> {
        list_names(&self.actual_root)
    }

    /// Scenario names with approved artifacts, recursively, sorted.
    pub fn approved_names(&self) -> Result<Vec<String>, SnapshotError> {
        list_names(&self.approved_root)
    }
}

/// Recursively collect scenario names below `root`: every `*.ansi` file,
/// relative to `root`, suffix stripped, segments re-joined with `/`.
fn list_names(root: &Path) -> Result<Vec<String>, SnapshotError> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    walk_names(root, root, &mut out)?;
    out.sort();
    out.dedup();
    Ok(out)
}

fn walk_names(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), SnapshotError> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| SnapshotError(format!("cannot list {}: {e}", dir.display())))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| SnapshotError(format!("cannot list {}: {e}", dir.display())))?;
        let path = entry.path();
        if path.is_dir() {
            walk_names(root, &path, out)?;
            continue;
        }
        let Some(file) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(_) = file.strip_suffix(".ansi") else {
            continue;
        };
        let rel = path.strip_prefix(root).map_err(|e| {
            SnapshotError(format!(
                "cannot relativize {} below {}: {e}",
                path.display(),
                root.display()
            ))
        })?;
        let mut segments: Vec<&str> = rel
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect();
        if let Some(last) = segments.last_mut() {
            *last = last.strip_suffix(".ansi").unwrap_or(last);
        }
        let name = segments.join("/");
        validate_name(&name).map_err(SnapshotError::from)?;
        out.push(name);
    }
    Ok(())
}
