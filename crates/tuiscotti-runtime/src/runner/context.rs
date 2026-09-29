use super::{AttemptId, BaselineId, is_nextest_map, sanitize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Runner-neutral context for one scenario execution (N02, N09).
///
/// Owns a stable [`BaselineId`], the current [`AttemptId`], and an isolated,
/// attempt-qualified scratch directory. Construction creates directories but
/// never mutates process environment or working directory.
#[derive(Debug, Clone)]
pub struct TestContext {
    baseline: BaselineId,
    attempt: AttemptId,
    scratch: PathBuf,
    evidence: PathBuf,
    nextest: bool,
}

impl TestContext {
    /// Capture the context for `scenario` from the process environment.
    /// # Errors
    ///
    /// Returns an I/O error when scratch directories cannot be created.
    pub fn current(scenario: &str) -> std::io::Result<Self> {
        let env: HashMap<String, String> = std::env::vars().collect();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::from_map(scenario, &env, &cwd)
    }

    /// Capture the context from an injected environment and working directory.
    /// Pure with respect to global state; safe under parallel tests.
    /// # Errors
    ///
    /// Returns an I/O error when scratch directories cannot be created.
    pub fn from_map(
        scenario: &str,
        env: &HashMap<String, String>,
        cwd: &Path,
    ) -> std::io::Result<Self> {
        let cwd_s = cwd.to_string_lossy().into_owned();
        let baseline = BaselineId::from_map(scenario, env, &cwd_s);
        let attempt = AttemptId::from_map(env);
        let nextest = is_nextest_map(env);
        let root = PathBuf::from(&baseline.workspace)
            .join("target")
            .join("tuiscotti-scratch")
            .join(sanitize(&format!(
                "{}-{}-{}-{}",
                baseline.package, baseline.binary, baseline.test, baseline.scenario
            )));
        fs::create_dir_all(&root)?;
        let scratch = claim_unique_dir(&root, &attempt.dir_suffix())?;
        let evidence = scratch.join("evidence");
        fs::create_dir_all(&evidence)?;
        Ok(Self {
            baseline,
            attempt,
            scratch,
            evidence,
            nextest,
        })
    }

    /// Stable baseline identity (excludes run/attempt).
    #[must_use]
    pub fn baseline(&self) -> &BaselineId {
        &self.baseline
    }

    /// Current attempt identity.
    #[must_use]
    pub fn attempt(&self) -> &AttemptId {
        &self.attempt
    }

    /// Isolated scratch directory, unique to this attempt (collision-suffixed).
    #[must_use]
    pub fn scratch_dir(&self) -> &Path {
        &self.scratch
    }

    /// Evidence directory (`<scratch>/evidence`) for Journals, captures, diffs.
    #[must_use]
    pub fn evidence_dir(&self) -> &Path {
        &self.evidence
    }

    /// Default journal path (`<scratch>/journal.jsonl`).
    #[must_use]
    pub fn journal_path(&self) -> PathBuf {
        self.scratch.join("journal.jsonl")
    }

    /// Whether this context was captured under cargo-nextest.
    #[must_use]
    pub fn is_nextest(&self) -> bool {
        self.nextest
    }

    /// Child-only environment: identity + locations for spawned processes.
    /// Applying these to a [`Command`] never touches the parent environment.
    #[must_use]
    pub fn child_env(&self) -> Vec<(String, String)> {
        let mut v = vec![
            ("TUISCOTTI_RUN_ID".to_string(), self.attempt.run.clone()),
            (
                "TUISCOTTI_ATTEMPT".to_string(),
                self.attempt.attempt.to_string(),
            ),
            (
                "TUISCOTTI_SCENARIO".to_string(),
                self.baseline.scenario.clone(),
            ),
            (
                "TUISCOTTI_SCRATCH".to_string(),
                self.scratch.to_string_lossy().into_owned(),
            ),
            (
                "TUISCOTTI_EVIDENCE".to_string(),
                self.evidence.to_string_lossy().into_owned(),
            ),
            ("TUISCOTTI_BASELINE".to_string(), self.baseline.stable_key()),
        ];
        if let Some(i) = self.attempt.stress_iter {
            v.push(("TUISCOTTI_STRESS_ITER".to_string(), i.to_string()));
        }
        v
    }

    /// Apply [`TestContext::child_env`] to a child command. Parent env untouched;
    /// the child's working directory is left alone.
    pub fn apply_to<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        for (k, val) in self.child_env() {
            cmd.env(k, val);
        }
        cmd
    }

    /// HOME/XDG isolation entries rooted at `<scratch>/home`. Child-only.
    #[must_use]
    pub fn home_isolation(&self) -> Vec<(String, String)> {
        let home = self.scratch.join("home");
        let s = |p: PathBuf| p.to_string_lossy().into_owned();
        vec![
            ("HOME".to_string(), s(home.clone())),
            ("XDG_CONFIG_HOME".to_string(), s(home.join("config"))),
            ("XDG_CACHE_HOME".to_string(), s(home.join("cache"))),
            ("XDG_DATA_HOME".to_string(), s(home.join("data"))),
            ("XDG_STATE_HOME".to_string(), s(home.join("state"))),
            ("XDG_RUNTIME_DIR".to_string(), s(home.join("run"))),
        ]
    }

    /// Create the isolated home tree and apply it to a child command.
    /// # Errors
    ///
    /// Returns an I/O error when the home tree cannot be created.
    pub fn apply_home_isolation<'a>(
        &self,
        cmd: &'a mut Command,
    ) -> std::io::Result<&'a mut Command> {
        for (_, dir) in self.home_isolation() {
            fs::create_dir_all(&dir)?;
        }
        for (k, val) in self.home_isolation() {
            cmd.env(k, val);
        }
        Ok(cmd)
    }
}

/// Atomically claim `<root>/<leaf>` (or `<leaf>-2`, … on collision).
/// Existing directories are never reused, so a retry/stress rerun cannot
/// overwrite a failed attempt's artifacts (N08).
fn claim_unique_dir(root: &Path, leaf: &str) -> std::io::Result<PathBuf> {
    let mut n = 0u32;
    loop {
        let name = if n == 0 {
            leaf.to_string()
        } else {
            format!("{leaf}-{n}")
        };
        let path = root.join(name);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                n += 1;
                if n > 9999 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        format!("scratch dir collision storm under {}", root.display()),
                    ));
                }
            }
            Err(e) => return Err(e),
        }
    }
}
