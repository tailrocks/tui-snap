use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use super::*;


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
    pub fn current(scenario: &str) -> std::io::Result<Self> {
        let env: HashMap<String, String> = std::env::vars().collect();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::from_map(scenario, &env, &cwd)
    }

    /// Capture the context from an injected environment and working directory.
    /// Pure with respect to global state; safe under parallel tests.
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
            .join("tuisnap-scratch")
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
    pub fn baseline(&self) -> &BaselineId {
        &self.baseline
    }

    /// Current attempt identity.
    pub fn attempt(&self) -> &AttemptId {
        &self.attempt
    }

    /// Isolated scratch directory, unique to this attempt (collision-suffixed).
    pub fn scratch_dir(&self) -> &Path {
        &self.scratch
    }

    /// Evidence directory (`<scratch>/evidence`) for Journals, captures, diffs.
    pub fn evidence_dir(&self) -> &Path {
        &self.evidence
    }

    /// Default journal path (`<scratch>/journal.jsonl`).
    pub fn journal_path(&self) -> PathBuf {
        self.scratch.join("journal.jsonl")
    }

    /// Whether this context was captured under cargo-nextest.
    pub fn is_nextest(&self) -> bool {
        self.nextest
    }

    /// Child-only environment: identity + locations for spawned processes.
    /// Applying these to a [`Command`] never touches the parent environment.
    pub fn child_env(&self) -> Vec<(String, String)> {
        let mut v = vec![
            ("TUISNAP_RUN_ID".to_string(), self.attempt.run.clone()),
            (
                "TUISNAP_ATTEMPT".to_string(),
                self.attempt.attempt.to_string(),
            ),
            (
                "TUISNAP_SCENARIO".to_string(),
                self.baseline.scenario.clone(),
            ),
            (
                "TUISNAP_SCRATCH".to_string(),
                self.scratch.to_string_lossy().into_owned(),
            ),
            (
                "TUISNAP_EVIDENCE".to_string(),
                self.evidence.to_string_lossy().into_owned(),
            ),
            ("TUISNAP_BASELINE".to_string(), self.baseline.stable_key()),
        ];
        if let Some(i) = self.attempt.stress_iter {
            v.push(("TUISNAP_STRESS_ITER".to_string(), i.to_string()));
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
