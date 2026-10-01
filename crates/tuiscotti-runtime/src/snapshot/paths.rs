use super::{SnapshotError, Store};
use std::path::{Path, PathBuf};

impl Store {
    /// Store rooted at `root` (approved/actual/diff/report.html beneath it).
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

    pub(crate) fn approved_frame(&self, name: &str) -> PathBuf {
        self.root
            .join("approved")
            .join(format!("{name}.frame.json"))
    }

    pub(crate) fn approved_png(&self, name: &str) -> PathBuf {
        self.root.join("approved").join(format!("{name}.png"))
    }

    pub(crate) fn actual_frame(&self, name: &str) -> PathBuf {
        self.root.join("actual").join(format!("{name}.frame.json"))
    }

    pub(crate) fn actual_png(&self, name: &str) -> PathBuf {
        self.root.join("actual").join(format!("{name}.png"))
    }

    /// Completion manifest sealing one actual candidate trio
    /// (`<name>.frame.json` + `<name>.png` + fidelity sidecar).
    pub(crate) fn actual_manifest(&self, name: &str) -> PathBuf {
        self.root
            .join("actual")
            .join(format!("{name}.manifest.json"))
    }

    /// Missing-glyph sidecar next to a PNG (`<name>.png.fidelity.json`).
    pub(crate) fn fidelity_sidecar(png: &Path) -> PathBuf {
        png.with_extension("png.fidelity.json")
    }

    pub(crate) fn diff_png(&self, name: &str) -> PathBuf {
        self.root.join("diff").join(format!("{name}.png"))
    }

    /// Names with actual frames (for `--all` acceptance).
    ///
    /// # Errors
    ///
    /// Returns `SnapshotError` when the actual directory cannot be listed.
    pub fn actual_names(&self) -> Result<Vec<String>, SnapshotError> {
        let dir = self.root.join("actual");
        let mut out = Vec::new();
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| SnapshotError(format!("cannot list {}: {e}", dir.display())))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| SnapshotError(format!("cannot list {}: {e}", dir.display())))?;
            if let Some(name) = entry.path().file_stem().and_then(|s| s.to_str())
                && entry.path().extension().and_then(|s| s.to_str()) == Some("json")
                && let Some(base) = name.strip_suffix(".frame")
            {
                out.push(base.to_string());
            }
        }
        out.sort();
        Ok(out)
    }
}
