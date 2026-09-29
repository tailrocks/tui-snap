//! Runner-neutral test context + cargo-nextest adapter (backlog N02–N10).
//!
//! This module never assumes which runner executes the test. Under
//! cargo-nextest it derives stable identity from the environment; under plain
//! `cargo test` (or any other runner) it degrades to a local run identity with
//! attempt `0`. It performs no global mutation: no `set_var`, no `set_current_dir`.
//!
//! # Qualified nextest surface
//!
//! Verified against installed `cargo-nextest 0.9.143` plus
//! `https://nexte.st/docs/configuration/env-vars/` and
//! `https://nexte.st/docs/glossary/`:
//!
//! | Variable | Since | Meaning |
//! |---|---|---|
//! | `NEXTEST_RUN_ID` | 0.9.138 | UUID shared by one `cargo nextest run` invocation |
//! | `NEXTEST_BINARY_ID` | 0.9.116 | `crate` \| `crate::bin` \| `crate::kind/bin` |
//! | `NEXTEST_TEST_NAME` | 0.9.116 | Test name |
//! | `NEXTEST_ATTEMPT` | 0.9.116 | 1-indexed attempt number (`"1"` without retries) |
//! | `NEXTEST_TOTAL_ATTEMPTS` | 0.9.116 | Configured attempt count |
//! | `NEXTEST_ATTEMPT_ID` | 0.9.116 | Globally unique per-attempt id (contains `$`) |
//! | `NEXTEST_STRESS_CURRENT` | 0.9.116 | 0-indexed stress index, or `"none"` |
//! | `NEXTEST_STRESS_TOTAL` | 0.9.116 | Stress total, `"unknown"`, or `"none"` |
//! | `NEXTEST_PROFILE` | 0.9.89 | Nextest profile in use |
//! | `NEXTEST_VERSION` | 0.9.130 | Nextest semver string |
//! | `NEXTEST_WORKSPACE_ROOT` | 0.9.130 | Workspace root (remap-aware) |
//! | `NEXTEST_BIN_EXE_<name>` | 0.9.113 | Remapped binary path; hyphen and underscore forms |
//! | `NEXTEST_EXECUTION_MODE` | — | Currently always `process-per-test` |
//! | `NEXTEST_TEST_GROUP` (+`_SLOT`, `_GLOBAL_SLOT`) | 0.9.90 | Test-group placement |
//!
//! There are **no** `NEXTEST_SHARD_*` variables: partitioning is a run-time
//! selection, not per-test identity. [`AttemptId::shard`] is therefore always
//! `None` from the environment; callers that know their partition set it with
//! [`AttemptId::with_shard`]. (Live probe on 0.9.143 also shows
//! `NEXTEST_RUN_MODE`, `NEXTEST_TEST_PHASE`, and
//! `NEXTEST_{REQUIRED,RECOMMENDED}_VERSION`, which this adapter does not consume.)
//!
//! Nextest ≥ 0.9.116 is required for attempt identity; run correlation
//! (`NEXTEST_RUN_ID`) needs ≥ 0.9.138. Older runners degrade to local identity.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Nextest version this adapter was qualified against.
pub const QUALIFIED_NEXTEST_VERSION: &str = "0.9.143";

/// Placeholder used when no package identity is discoverable.
pub const UNKNOWN_PACKAGE: &str = "unknown-package";
/// Placeholder used when no test name is discoverable.
pub const UNKNOWN_TEST: &str = "unknown-test";
/// Profile label used outside nextest.
pub const LOCAL_PROFILE: &str = "local";

/// True when the current process runs under cargo-nextest.
///
/// Detection key is `NEXTEST_RUN_ID` (set for every test process since 0.9.138).
pub fn is_nextest() -> bool {
    std::env::var_os("NEXTEST_RUN_ID").is_some()
}

/// [`is_nextest`] over an injected environment (tests avoid global env mutation).
pub fn is_nextest_map(env: &HashMap<String, String>) -> bool {
    env.contains_key("NEXTEST_RUN_ID")
}

fn get(env: &HashMap<String, String>, key: &str) -> Option<String> {
    env.get(key).cloned()
}

/// Stable baseline identity: what is under test (N02).
///
/// Equal across retries, stress iterations, and shards of the same scenario;
/// see [`BaselineId::stable_key`]. `scenario` is caller-supplied (the tui-snap
/// capture scenario); everything else comes from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineId {
    /// Workspace root (`NEXTEST_WORKSPACE_ROOT`, else `CARGO_MANIFEST_DIR`, else CWD).
    pub workspace: String,
    /// Cargo package name (first segment of `NEXTEST_BINARY_ID`, else `CARGO_PKG_NAME`).
    pub package: String,
    /// Test binary within the package (remainder of `NEXTEST_BINARY_ID`).
    pub binary: String,
    /// Test name (`NEXTEST_TEST_NAME`, else [`UNKNOWN_TEST`]).
    pub test: String,
    /// Caller-supplied scenario name.
    pub scenario: String,
    /// Nextest profile (`NEXTEST_PROFILE`, else [`LOCAL_PROFILE`]).
    pub profile: String,
    /// Optional caller-supplied variant (theme, viewport class, …). Never inferred.
    pub variant: Option<String>,
}

impl BaselineId {
    /// Read identity from the process environment.
    pub fn from_env(scenario: &str) -> Self {
        let env: HashMap<String, String> =
            std::env::vars().filter(|(k, _)| is_relevant(k)).collect();
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_string());
        Self::from_map(scenario, &env, &cwd)
    }

    /// Read identity from an injected environment. `fallback_workspace` is used
    /// when neither `NEXTEST_WORKSPACE_ROOT` nor `CARGO_MANIFEST_DIR` is present.
    ///
    /// ```
    /// # use std::collections::HashMap;
    /// # use tuiscotti_runtime::runner::BaselineId;
    /// let mut env = HashMap::new();
    /// env.insert("NEXTEST_BINARY_ID".into(), "my-crate::integration".into());
    /// env.insert("NEXTEST_TEST_NAME".into(), "renders_empty".into());
    /// env.insert("NEXTEST_PROFILE".into(), "ci".into());
    /// env.insert("NEXTEST_WORKSPACE_ROOT".into(), "/repo".into());
    /// let id = BaselineId::from_map("empty-view", &env, "/fallback");
    /// assert_eq!(id.package, "my-crate");
    /// assert_eq!(id.binary, "integration");
    /// assert_eq!(id.profile, "ci");
    /// assert_eq!(id.workspace, "/repo");
    /// ```
    pub fn from_map(
        scenario: &str,
        env: &HashMap<String, String>,
        fallback_workspace: &str,
    ) -> Self {
        let (package, binary) = parse_binary_id(
            get(env, "NEXTEST_BINARY_ID").as_deref(),
            get(env, "CARGO_PKG_NAME").as_deref(),
        );
        let workspace = get(env, "NEXTEST_WORKSPACE_ROOT")
            .or_else(|| get(env, "CARGO_MANIFEST_DIR"))
            .unwrap_or_else(|| fallback_workspace.to_string());
        Self {
            workspace,
            package,
            binary,
            test: get(env, "NEXTEST_TEST_NAME").unwrap_or_else(|| UNKNOWN_TEST.to_string()),
            scenario: scenario.to_string(),
            profile: get(env, "NEXTEST_PROFILE").unwrap_or_else(|| LOCAL_PROFILE.to_string()),
            variant: None,
        }
    }

    /// Attach a caller-supplied variant (theme, viewport class, …).
    pub fn with_variant(mut self, variant: &str) -> Self {
        self.variant = Some(variant.to_string());
        self
    }

    /// Key stable across retries/stress/shards of this scenario. Excludes every
    /// [`AttemptId`] field by construction.
    pub fn stable_key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.workspace,
            self.package,
            self.binary,
            self.test,
            self.scenario,
            self.profile,
            self.variant.as_deref().unwrap_or("-"),
        )
    }
}

/// Split `NEXTEST_BINARY_ID` (`crate` | `crate::bin` | `crate::kind/bin`).
/// `cargo_pkg` is the `CARGO_PKG_NAME` fallback when no binary id is present.
fn parse_binary_id(binary_id: Option<&str>, cargo_pkg: Option<&str>) -> (String, String) {
    match binary_id {
        Some(id) => match id.split_once("::") {
            Some((pkg, rest)) => (pkg.to_string(), rest.to_string()),
            None => (id.to_string(), id.to_string()),
        },
        None => {
            let pkg = cargo_pkg.unwrap_or(UNKNOWN_PACKAGE).to_string();
            (pkg.clone(), pkg)
        }
    }
}

/// Per-attempt identity: which execution of the baseline (N02, N08).
///
/// Under nextest, `run` is the shared run UUID and `attempt` is 1-indexed.
/// Outside nextest, `run` is a generated `local-…` id and `attempt` is `0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptId {
    /// `NEXTEST_RUN_ID`, else a generated unique local id.
    pub run: String,
    /// `NEXTEST_ATTEMPT` (1-indexed); `0` outside nextest.
    pub attempt: u32,
    /// `NEXTEST_STRESS_CURRENT` (`"none"` → `None`).
    pub stress_iter: Option<u64>,
    /// Shard/partition. Nextest exposes none; set explicitly via [`AttemptId::with_shard`].
    pub shard: Option<String>,
    /// `NEXTEST_ATTEMPT_ID` (globally unique per attempt), when present.
    /// Live-verified format on 0.9.143: `<run-uuid>:<binary-id>$<test-name>`.
    pub attempt_uid: Option<String>,
}

impl AttemptId {
    /// Read attempt identity from the process environment.
    pub fn from_env() -> Self {
        let env: HashMap<String, String> =
            std::env::vars().filter(|(k, _)| is_relevant(k)).collect();
        Self::from_map(&env)
    }

    /// Read attempt identity from an injected environment.
    ///
    /// ```
    /// # use std::collections::HashMap;
    /// # use tuiscotti_runtime::runner::AttemptId;
    /// // Absent env degrades to a local run with attempt 0.
    /// let id = AttemptId::from_map(&HashMap::new());
    /// assert_eq!(id.attempt, 0);
    /// assert!(id.run.starts_with("local-"));
    /// assert_eq!(id.stress_iter, None);
    ///
    /// let mut env = HashMap::new();
    /// env.insert("NEXTEST_RUN_ID".into(), "1f79aa0d-4ec8-4a5c-aa83-5e8dc2f36573".into());
    /// env.insert("NEXTEST_ATTEMPT".into(), "2".into());
    /// env.insert("NEXTEST_STRESS_CURRENT".into(), "none".into());
    /// let id = AttemptId::from_map(&env);
    /// assert_eq!(id.attempt, 2);
    /// assert_eq!(id.stress_iter, None);
    /// ```
    pub fn from_map(env: &HashMap<String, String>) -> Self {
        let run = get(env, "NEXTEST_RUN_ID").unwrap_or_else(generate_local_run_id);
        let attempt = get(env, "NEXTEST_ATTEMPT")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        let stress_iter = match get(env, "NEXTEST_STRESS_CURRENT") {
            Some(v) if v != "none" => v.parse::<u64>().ok(),
            _ => None,
        };
        Self {
            run,
            attempt,
            stress_iter,
            shard: None,
            attempt_uid: get(env, "NEXTEST_ATTEMPT_ID"),
        }
    }

    /// Attach a shard/partition label known to the caller.
    pub fn with_shard(mut self, shard: &str) -> Self {
        self.shard = Some(shard.to_string());
        self
    }

    /// Filesystem-safe leaf qualifying one attempt's artifacts. Distinct runs,
    /// attempts, stress iterations, and shards map to distinct leaves.
    pub fn dir_suffix(&self) -> String {
        let run8: String = sanitize(&self.run).chars().take(8).collect();
        let mut s = format!("run-{run8}-attempt-{}", self.attempt);
        if let Some(i) = self.stress_iter {
            s.push_str(&format!("-stress-{i}"));
        }
        if let Some(sh) = &self.shard {
            s.push_str(&format!("-shard-{}", sanitize(sh)));
        }
        s
    }
}

static LOCAL_RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

fn generate_local_run_id() -> String {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ctr = LOCAL_RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("local-{pid}-{nanos:x}-{ctr:x}")
}

fn is_relevant(key: &str) -> bool {
    key.starts_with("NEXTEST_")
        || key.starts_with("CARGO_BIN_EXE_")
        || key == "CARGO_MANIFEST_DIR"
        || key == "CARGO_PKG_NAME"
}

/// Replace path-hostile characters; cap length for deep scratch trees.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect()
}

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

/// Executable resolution failure (N03, N04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// No candidate named an existing file. `searched` lists the env vars
    /// consulted; `values` lists values that were set but pointed nowhere.
    Missing {
        package: String,
        bin: String,
        searched: Vec<String>,
        values: Vec<(String, String)>,
    },
    /// Two distinct existing files were named. The caller must disambiguate;
    /// this adapter never silently picks one.
    Ambiguous {
        package: String,
        bin: String,
        candidates: Vec<PathBuf>,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::Missing {
                package,
                bin,
                searched,
                values,
            } => {
                write!(
                    f,
                    "cannot resolve binary '{bin}' of package '{package}': no candidate exists \
                     (searched vars: {}; set-but-missing: {:?}). Refusing to guess a target-dir path.",
                    searched.join(", "),
                    values
                )
            }
            ResolveError::Ambiguous {
                package,
                bin,
                candidates,
            } => {
                write!(
                    f,
                    "ambiguous binary '{bin}' of package '{package}': distinct existing candidates: {:?}",
                    candidates
                )
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// Resolve a binary target's executable (N03, N04).
///
/// Consults, in order, `NEXTEST_BIN_EXE_<bin>` (exact, then `-`→`_` form) and
/// `CARGO_BIN_EXE_<bin>` (exact, then `-`→`_` form). Nextest remaps these when
/// reusing archived builds, so they stay correct under archive/remap runs.
/// Identical paths dedupe (nextest sets both hyphen and underscore forms).
///
/// There is deliberately no `target/debug` probing and no nested `cargo build`:
/// both would silently use stale or source-relative paths.
/// Reads the process environment; see [`resolve_bin_with_map`] for the pure form.
pub fn resolve_bin(package: &str, bin: &str) -> Result<PathBuf, ResolveError> {
    let env: HashMap<String, String> = std::env::vars().collect();
    resolve_bin_with_map(package, bin, &env)
}

/// [`resolve_bin`] over an injected environment.
pub fn resolve_bin_with_map(
    package: &str,
    bin: &str,
    env: &HashMap<String, String>,
) -> Result<PathBuf, ResolveError> {
    let underscored = bin.replace('-', "_");
    let mut names = vec![
        format!("NEXTEST_BIN_EXE_{bin}"),
        format!("CARGO_BIN_EXE_{bin}"),
    ];
    if underscored != bin {
        names.push(format!("NEXTEST_BIN_EXE_{underscored}"));
        names.push(format!("CARGO_BIN_EXE_{underscored}"));
    }
    let mut values = Vec::new();
    let mut existing: Vec<PathBuf> = Vec::new();
    for name in &names {
        if let Some(v) = env.get(name) {
            let p = PathBuf::from(v);
            if p.is_file() {
                if !existing.contains(&p) {
                    existing.push(p);
                }
            } else {
                values.push((name.clone(), v.clone()));
            }
        }
    }
    match existing.len() {
        0 => Err(ResolveError::Missing {
            package: package.to_string(),
            bin: bin.to_string(),
            searched: names,
            values,
        }),
        1 => Ok(existing.pop().unwrap()),
        _ => Err(ResolveError::Ambiguous {
            package: package.to_string(),
            bin: bin.to_string(),
            candidates: existing,
        }),
    }
}

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

/// Append-only JSONL event journal (N07).
///
/// Every [`Journal::append`] flushes, so a killed or timed-out attempt leaves
/// its last flushed events inspectable. Completion requires an explicit
/// [`Journal::complete`]; a journal without the completion marker is
/// [`JournalStatus::Incomplete`], never a pass.
#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    file: File,
    seq: u64,
}

/// Completion marker filename written beside `journal.jsonl`.
pub const COMPLETE_MARKER: &str = "COMPLETE";

impl Journal {
    /// Open (or resume) the journal at `path`, creating parent directories.
    /// The sequence counter resumes after the existing line count.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let seq = fs::File::open(path)
            .map(|f| BufReader::new(f).lines().count() as u64)
            .unwrap_or(0);
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            seq,
        })
    }

    /// Journal path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one event and flush. Minimal std-only JSON escaping applies.
    pub fn append(&mut self, event: &str, detail: &str) -> std::io::Result<()> {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        writeln!(
            self.file,
            "{{\"seq\":{},\"unix_ms\":{ms},\"event\":\"{}\",\"detail\":\"{}\"}}",
            self.seq,
            json_escape(event),
            json_escape(detail),
        )?;
        self.file.flush()?;
        self.seq += 1;
        Ok(())
    }

    /// Mark the attempt complete with a terminal `status`, then write the
    /// `COMPLETE` marker. Fail-closed: anything killed before this stays incomplete.
    pub fn complete(&mut self, status: &str) -> std::io::Result<()> {
        self.append("complete", status)?;
        if let Some(parent) = self.path.parent() {
            fs::write(parent.join(COMPLETE_MARKER), format!("{status}\n"))?;
        }
        Ok(())
    }

    /// Read the completion status of a journal directory: complete only when the
    /// `COMPLETE` marker exists **and** the journal's last event is `complete`.
    /// Missing journal, missing marker, or any other tail → incomplete.
    pub fn status(dir: &Path) -> JournalStatus {
        let marker = dir.join(COMPLETE_MARKER);
        if !marker.is_file() {
            return JournalStatus::Incomplete {
                reason: format!("missing {} marker", dir.join(COMPLETE_MARKER).display()),
            };
        }
        let text = match fs::read_to_string(dir.join("journal.jsonl")) {
            Ok(t) => t,
            Err(e) => {
                return JournalStatus::Incomplete {
                    reason: format!("cannot read journal.jsonl: {e}"),
                }
            }
        };
        match text.lines().rfind(|l| !l.trim().is_empty()) {
            Some(last) if is_complete_event(last) => JournalStatus::Complete {
                status: extract_field(last, "detail").unwrap_or_default(),
            },
            _ => JournalStatus::Incomplete {
                reason: "journal tail is not a complete event".to_string(),
            },
        }
    }
}

/// Journal completion state (N07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalStatus {
    /// Explicitly completed with this terminal status string.
    Complete { status: String },
    /// Killed, timed out, or never finished: not a pass.
    Incomplete { reason: String },
}

impl JournalStatus {
    /// True only for [`JournalStatus::Complete`].
    pub fn is_complete(&self) -> bool {
        matches!(self, JournalStatus::Complete { .. })
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn is_complete_event(line: &str) -> bool {
    extract_field(line, "event").as_deref() == Some("complete")
}

/// Extract a top-level string field from a flat JSON object line (handles escapes).
fn extract_field(line: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let mut rest = line.split_once(&key)?.1.trim_start();
    rest = rest.strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let n = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(n)?);
                }
                _ => return None,
            },
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}

/// Read-only correlation key joining attempt artifacts to nextest results (N05, N10).
///
/// `testsuite` is the nextest binary id (`name` on each JUnit `testsuite`),
/// `testcase` is the test name, and `run_uuid` is the `uuid` on the JUnit
/// `testsuites` root. This helper only *reads* identity for correlation; it
/// never rewrites pass/fail status and never invents test entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JunitKey {
    /// `NEXTEST_RUN_ID`.
    pub run_uuid: String,
    /// `NEXTEST_BINARY_ID`.
    pub testsuite: String,
    /// `NEXTEST_TEST_NAME`.
    pub testcase: String,
}

impl JunitKey {
    /// Correlation key for this process, or `None` outside nextest (or when any
    /// of the three identity vars is missing — partial keys are refused).
    pub fn current() -> Option<Self> {
        let env: HashMap<String, String> = std::env::vars().collect();
        Self::from_map(&env)
    }

    /// [`JunitKey::current`] over an injected environment.
    ///
    /// ```
    /// # use std::collections::HashMap;
    /// # use tuiscotti_runtime::runner::JunitKey;
    /// assert!(JunitKey::from_map(&HashMap::new()).is_none());
    /// let mut env = HashMap::new();
    /// env.insert("NEXTEST_RUN_ID".into(), "run-1".into());
    /// env.insert("NEXTEST_BINARY_ID".into(), "my-crate::integration".into());
    /// env.insert("NEXTEST_TEST_NAME".into(), "renders_empty".into());
    /// let key = JunitKey::from_map(&env).unwrap();
    /// assert!(key.matches("my-crate::integration", "renders_empty"));
    /// assert!(!key.matches("my-crate::integration", "other"));
    /// assert!(key.run_matches("run-1"));
    /// ```
    pub fn from_map(env: &HashMap<String, String>) -> Option<Self> {
        Some(Self {
            run_uuid: get(env, "NEXTEST_RUN_ID")?,
            testsuite: get(env, "NEXTEST_BINARY_ID")?,
            testcase: get(env, "NEXTEST_TEST_NAME")?,
        })
    }

    /// Do JUnit `testsuite`/`testcase` names identify this attempt's test?
    pub fn matches(&self, testsuite: &str, testcase: &str) -> bool {
        self.testsuite == testsuite && self.testcase == testcase
    }

    /// Does a JUnit root `uuid` identify this attempt's run?
    pub fn run_matches(&self, run_uuid: &str) -> bool {
        self.run_uuid == run_uuid
    }
}

/// Build a child [`Command`] with the context's env applied, without touching
/// the parent environment or working directory.
pub fn child_command(ctx: &TestContext, program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    ctx.apply_to(&mut cmd);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_id_shapes() {
        assert_eq!(
            parse_binary_id(Some("my-crate"), None),
            ("my-crate".to_string(), "my-crate".to_string())
        );
        assert_eq!(
            parse_binary_id(Some("my-crate::integration"), None),
            ("my-crate".to_string(), "integration".to_string())
        );
        assert_eq!(
            parse_binary_id(Some("my-crate::bench/perf"), None),
            ("my-crate".to_string(), "bench/perf".to_string())
        );
        assert_eq!(
            parse_binary_id(None, Some("pkg")),
            ("pkg".to_string(), "pkg".to_string())
        );
        assert_eq!(
            parse_binary_id(None, None),
            (UNKNOWN_PACKAGE.to_string(), UNKNOWN_PACKAGE.to_string())
        );
    }

    #[test]
    fn json_roundtrip() {
        let s = "a\"b\\c\nd\te\x01f";
        let line = format!("{{\"event\":\"x\",\"detail\":\"{}\"}}", json_escape(s));
        assert_eq!(extract_field(&line, "detail").as_deref(), Some(s));
        assert_eq!(extract_field(&line, "event").as_deref(), Some("x"));
        assert!(extract_field(&line, "nope").is_none());
    }

    #[test]
    fn local_run_ids_unique() {
        let a = generate_local_run_id();
        let b = generate_local_run_id();
        assert_ne!(a, b);
        assert!(a.starts_with("local-"));
    }
}
