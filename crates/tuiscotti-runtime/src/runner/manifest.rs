use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Required-scenario inventory: record what ran, verify against what must run (N06).
///
/// A filtered run (only some required scenarios executed) evaluates to
/// [`ManifestVerdict::Partial`], never to full.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScenarioManifest {
    required: Vec<String>,
}

impl ScenarioManifest {
    /// Build a manifest from required scenario names (deduped, sorted).
    ///
    /// ```
    /// # use tuiscotti_runtime::runner::{ScenarioManifest, ManifestVerdict};
    /// let m = ScenarioManifest::new(["b", "a", "a"]);
    /// assert!(matches!(m.evaluate(&["a".into(), "b".into()]), ManifestVerdict::Full { .. }));
    /// assert!(matches!(m.evaluate(&["a".into()]), ManifestVerdict::Partial { .. }));
    /// ```
    pub fn new(required: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut v: Vec<String> = required.into_iter().map(Into::into).collect();
        v.sort();
        v.dedup();
        Self { required: v }
    }

    /// Required scenario names.
    #[must_use]
    pub fn required(&self) -> &[String] {
        &self.required
    }

    /// Load a manifest file: one scenario per line; blank lines and `#` comments skipped.
    /// # Errors
    ///
    /// Returns an I/O error when the manifest file cannot be read.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let text = fs::read_to_string(path)?;
        Ok(Self::new(parse_lines(&text)))
    }

    /// Save the manifest in [`ScenarioManifest::load`] format.
    /// # Errors
    ///
    /// Returns an I/O error when the manifest file cannot be written.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut text = String::from("# tui-snap required scenarios, one per line\n");
        for r in &self.required {
            text.push_str(r);
            text.push('\n');
        }
        fs::write(path, text)
    }

    /// Verify `executed` against the required set. Full only when every required
    /// scenario was executed; an empty manifest is [`ManifestVerdict::Incomplete`]
    /// (a vacuous "full" would be a fake-full gate).
    #[must_use]
    pub fn evaluate(&self, executed: &[String]) -> ManifestVerdict {
        if self.required.is_empty() {
            return ManifestVerdict::Incomplete {
                reason: "manifest lists no required scenarios".to_string(),
            };
        }
        let have: HashSet<&str> = executed.iter().map(String::as_str).collect();
        let missing: Vec<String> = self
            .required
            .iter()
            .filter(|r| !have.contains(r.as_str()))
            .cloned()
            .collect();
        if missing.is_empty() {
            ManifestVerdict::Full {
                executed: self.required.len(),
            }
        } else {
            ManifestVerdict::Partial { missing }
        }
    }

    /// Verify an execution record file (see [`ScenarioManifest::record_execution`]).
    /// A missing or unreadable record is [`ManifestVerdict::Incomplete`], never full.
    #[must_use]
    pub fn evaluate_record(&self, record_path: &Path) -> ManifestVerdict {
        match load_record(record_path) {
            Ok(executed) => self.evaluate(&executed),
            Err(e) => ManifestVerdict::Incomplete {
                reason: format!(
                    "cannot read execution record {}: {e}",
                    record_path.display()
                ),
            },
        }
    }

    /// Append one executed scenario to the record file (created with parents).
    /// # Errors
    ///
    /// Returns an I/O error when the record file cannot be created or written.
    pub fn record_execution(record_path: &Path, scenario: &str) -> std::io::Result<()> {
        if let Some(parent) = record_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(record_path)?;
        writeln!(f, "{scenario}")?;
        f.flush()
    }
}

/// Gate verdict for a required-scenario manifest (N06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestVerdict {
    /// Every required scenario executed.
    Full {
        /// How many required scenarios executed.
        executed: usize,
    },
    /// Filtered/partial run: these required scenarios never executed.
    Partial {
        /// Required scenarios that never executed.
        missing: Vec<String>,
    },
    /// No trustworthy evidence (missing record, empty manifest, …).
    Incomplete {
        /// Why no trustworthy verdict exists.
        reason: String,
    },
}

impl ManifestVerdict {
    /// True only for [`ManifestVerdict::Full`].
    #[must_use]
    pub fn is_full(&self) -> bool {
        matches!(self, ManifestVerdict::Full { .. })
    }
}

fn parse_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

fn load_record(path: &Path) -> std::io::Result<Vec<String>> {
    Ok(parse_lines(&fs::read_to_string(path)?))
}
