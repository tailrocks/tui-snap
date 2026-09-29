use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
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
#[must_use]
pub fn is_nextest() -> bool {
    std::env::var_os("NEXTEST_RUN_ID").is_some()
}

/// [`is_nextest`] over an injected environment (tests avoid global env mutation).
#[must_use]
pub fn is_nextest_map<S: std::hash::BuildHasher>(env: &HashMap<String, String, S>) -> bool {
    env.contains_key("NEXTEST_RUN_ID")
}

pub(crate) fn get(env: &HashMap<String, String>, key: &str) -> Option<String> {
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
    #[must_use]
    pub fn from_env(scenario: &str) -> Self {
        let env: HashMap<String, String> =
            std::env::vars().filter(|(k, _)| is_relevant(k)).collect();
        let cwd = std::env::current_dir()
            .map_or_else(|_| ".".to_string(), |p| p.to_string_lossy().into_owned());
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
    #[must_use]
    pub fn from_map(
        scenario: &str,
        env: &HashMap<String, String>,
        fallback_workspace: &str,
    ) -> Self {
        let (package, binary) = parse_binary_id(
            get(env, "NEXTEST_BINARY_ID").as_deref(),
            get(env, "CARGO_PKG_NAME").as_deref(),
        );
        // `NEXTEST_WORKSPACE_ROOT` is authoritative. The cargo-test fallbacks
        // name a package dir, so resolve the enclosing workspace root: scratch
        // lives under the workspace `target/`, not the package's.
        let workspace = get(env, "NEXTEST_WORKSPACE_ROOT").unwrap_or_else(|| {
            let anchor =
                get(env, "CARGO_MANIFEST_DIR").unwrap_or_else(|| fallback_workspace.to_string());
            workspace_root_of(Path::new(&anchor))
                .to_string_lossy()
                .into_owned()
        });
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
    #[must_use]
    pub fn with_variant(mut self, variant: &str) -> Self {
        self.variant = Some(variant.to_string());
        self
    }

    /// Key stable across retries/stress/shards of this scenario. Excludes every
    /// [`AttemptId`] field by construction.
    #[must_use]
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
pub(crate) fn parse_binary_id(
    binary_id: Option<&str>,
    cargo_pkg: Option<&str>,
) -> (String, String) {
    if let Some(id) = binary_id {
        match id.split_once("::") {
            Some((pkg, rest)) => (pkg.to_string(), rest.to_string()),
            None => (id.to_string(), id.to_string()),
        }
    } else {
        let pkg = cargo_pkg.unwrap_or(UNKNOWN_PACKAGE).to_string();
        (pkg.clone(), pkg)
    }
}

/// Nearest enclosing cargo workspace root for `start` (a package manifest
/// dir or cwd): the closest ancestor-or-self whose `Cargo.toml` declares
/// `[workspace]`, matching cargo's own root discovery. Returns `start`
/// unchanged when no workspace manifest is found, so synthetic test maps
/// keep their verbatim values.
fn workspace_root_of(start: &Path) -> PathBuf {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if let Ok(text) = fs::read_to_string(dir.join("Cargo.toml"))
            && text
                .lines()
                .any(|l| l.trim_start().starts_with("[workspace"))
        {
            return dir.to_path_buf();
        }
        cur = dir.parent();
    }
    start.to_path_buf()
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
    #[must_use]
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
    #[must_use]
    pub fn with_shard(mut self, shard: &str) -> Self {
        self.shard = Some(shard.to_string());
        self
    }

    /// Filesystem-safe leaf qualifying one attempt's artifacts. Distinct runs,
    /// attempts, stress iterations, and shards map to distinct leaves.
    #[must_use]
    pub fn dir_suffix(&self) -> String {
        let run8: String = sanitize(&self.run).chars().take(8).collect();
        let mut s = format!("run-{run8}-attempt-{}", self.attempt);
        if let Some(i) = self.stress_iter {
            s.push_str("-stress-");
            s.push_str(&i.to_string());
        }
        if let Some(sh) = &self.shard {
            s.push_str("-shard-");
            s.push_str(&sanitize(sh));
        }
        s
    }
}

static LOCAL_RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn generate_local_run_id() -> String {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
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
pub(crate) fn sanitize(name: &str) -> String {
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
