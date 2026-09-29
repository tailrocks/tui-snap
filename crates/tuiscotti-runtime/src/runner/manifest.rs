use super::*;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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
        let mut v: Vec<String> = required.into_iter().map(|s| s.into()).collect();
        v.sort();
        v.dedup();
        Self { required: v }
    }

    /// Required scenario names.
    pub fn required(&self) -> &[String] {
        &self.required
    }

    /// Load a manifest file: one scenario per line; blank lines and `#` comments skipped.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let text = fs::read_to_string(path)?;
        Ok(Self::new(parse_lines(&text)))
    }

    /// Save the manifest in [`ScenarioManifest::load`] format.
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
    pub fn evaluate(&self, executed: &[String]) -> ManifestVerdict {
        if self.required.is_empty() {
            return ManifestVerdict::Incomplete {
                reason: "manifest lists no required scenarios".to_string(),
            };
        }
        let have: HashSet<&str> = executed.iter().map(|s| s.as_str()).collect();
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
    Full { executed: usize },
    /// Filtered/partial run: these required scenarios never executed.
    Partial { missing: Vec<String> },
    /// No trustworthy evidence (missing record, empty manifest, …).
    Incomplete { reason: String },
}

impl ManifestVerdict {
    /// True only for [`ManifestVerdict::Full`].
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
