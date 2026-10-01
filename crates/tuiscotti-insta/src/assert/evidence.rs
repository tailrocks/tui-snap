//! Partitioned candidate-evidence bundles and PNG/snapshot binding tags.
//!
//! Layout: `<root>/<package>/<test>/<scenario>[@<variant>]/run-<run>/attempt-<n>/
//! [canonical.txt, image.png, sample.{ansi,txt,html}, manifest.json, complete.json]`.
//!
//! - The STABLE path (`package/test/scenario/variant`) names WHAT was
//!   asserted; the RUN path (`run/attempt[/stress/shard]`) names WHICH
//!   execution produced it. Retries and shards never overwrite each other,
//!   and rerunning the suite preserves every attempt's bundle.
//! - Each bundle is published atomically (temp dir + rename): a bundle
//!   directory that exists is COMPLETE — all artifacts plus the manifest —
//!   never a half-written set racing a failure.
//! - `manifest.json` records the deterministic sample binding (canonical +
//!   render identity + payload hashes) alongside, but separate from, the
//!   run/attempt identity — the binding proves WHAT the sample is, the path
//!   proves WHEN it ran.

use super::{AssertError, Sample, SnapshotIdentity};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

mod png_tag;

pub(crate) use png_tag::snap_generation;
pub use png_tag::{png_generation, png_tag_generation};

// ---------------------------------------------------------------------------
// Run/attempt identity (nextest-aware, dependency-free).
// ---------------------------------------------------------------------------

/// Which execution produced a bundle. Mirrors the nextest environment
/// contract (`tuiscotti_runtime::runner` owns the canonical copy; this is
/// the assert facade's local reader so `tuiscotti-insta` never depends on
/// the runtime): `NEXTEST_RUN_ID`, 1-indexed `NEXTEST_ATTEMPT` (`0` outside
/// nextest, garbage lossy-zero), `NEXTEST_STRESS_CURRENT` (`"none"` →
/// `None`), globally unique `NEXTEST_ATTEMPT_ID` when present. Shards are
/// caller-set via [`SHARD_ENV`], never inferred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptIdentity {
    /// `NEXTEST_RUN_ID`, else a process-unique local id.
    pub run: String,
    /// `NEXTEST_ATTEMPT`; `0` outside nextest.
    pub attempt: u32,
    /// `NEXTEST_STRESS_CURRENT`, if set and numeric.
    pub stress_iter: Option<u64>,
    /// Caller-set shard/partition ([`SHARD_ENV`]).
    pub shard: Option<String>,
    /// `NEXTEST_ATTEMPT_ID` (globally unique per attempt), when present.
    pub attempt_uid: Option<String>,
}

/// Env var carrying an explicit shard/partition label for evidence paths.
pub const SHARD_ENV: &str = "TUISCOTTI_SHARD";

/// Local run id for this process, computed once: `local-<pid>-<nanos>`.
/// Stable within the run (every assert in one process shares it), unique
/// across runs.
fn local_run_id() -> String {
    static ONCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("local-{}-{nanos}", std::process::id())
    })
    .clone()
}

impl AttemptIdentity {
    /// Read attempt identity from the process environment.
    #[must_use]
    pub fn from_env() -> Self {
        let env: HashMap<String, String> = std::env::vars()
            .filter(|(k, _)| k.starts_with("NEXTEST_") || k == SHARD_ENV)
            .collect();
        Self::from_map(&env)
    }

    /// Read attempt identity from an injected environment (hermetic tests).
    #[must_use]
    pub fn from_map(env: &HashMap<String, String>) -> Self {
        let run = env
            .get("NEXTEST_RUN_ID")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(local_run_id);
        let attempt = env
            .get("NEXTEST_ATTEMPT")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let stress_iter = env.get("NEXTEST_STRESS_CURRENT").and_then(|s| {
            if s == "none" {
                None
            } else {
                s.parse::<u64>().ok()
            }
        });
        let shard = env.get(SHARD_ENV).filter(|s| !s.is_empty()).cloned();
        let attempt_uid = env
            .get("NEXTEST_ATTEMPT_ID")
            .filter(|s| !s.is_empty())
            .cloned();
        Self {
            run,
            attempt,
            stress_iter,
            shard,
            attempt_uid,
        }
    }

    /// Run directory stem: `run-<sanitized-run>`.
    #[must_use]
    pub fn run_dir(&self) -> String {
        format!("run-{}", sanitize_segment(&self.run))
    }

    /// Attempt directory stem: `attempt-<n>[-stress<i>][-shard<s>]`.
    #[must_use]
    pub fn attempt_dir(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!("attempt-{}", self.attempt);
        if let Some(i) = self.stress_iter {
            write!(s, "-stress{i}").unwrap_or_default();
        }
        if let Some(sh) = self.shard.as_deref() {
            write!(s, "-shard-{}", sanitize_segment(sh)).unwrap_or_default();
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Evidence identity + bundle writer.
// ---------------------------------------------------------------------------

/// Full evidence identity: WHAT was asserted × WHICH execution ran it.
#[derive(Debug, Clone)]
pub struct EvidenceId {
    /// Caller package (`env!("CARGO_PKG_NAME")` at the macro call site).
    pub package: String,
    /// Test identity (test-thread name, sanitized).
    pub test: String,
    /// Scenario (snapshot base name; name-validated, never sanitized-silent).
    pub scenario: String,
    /// Snapshot variant (active Insta suffix), if any.
    pub variant: Option<String>,
    /// Which execution produced the bundle.
    pub attempt: AttemptIdentity,
}

impl EvidenceId {
    /// Bundle directory under `root`:
    /// `<root>/<package>/<test>/<scenario>[@<variant>]/<run>/<attempt>/`.
    /// Every segment is name-validated or sanitized, so the result can
    /// never escape `root`.
    #[must_use]
    pub fn bundle_dir(&self, root: &Path) -> PathBuf {
        let scenario = match self.variant.as_deref() {
            Some(v) => format!("{}@{}", self.scenario, sanitize_segment(v)),
            None => self.scenario.clone(),
        };
        root.join(sanitize_segment(&self.package))
            .join(sanitize_segment(&self.test))
            .join(scenario)
            .join(self.attempt.run_dir())
            .join(self.attempt.attempt_dir())
    }
}

/// Current test identity: the test-thread name (`cargo test` and nextest
/// both run each test on a thread named for the test), or `"main"` when
/// the thread is unnamed.
#[must_use]
pub fn current_test_name() -> String {
    std::thread::current().name().unwrap_or("main").to_string()
}

/// Sanitize one path segment: keep `[A-Za-z0-9._-]`, map everything else
/// (including `/`, `\`, `:`) to `_`; empty/degenerate results become
/// `"unknown"`. Never returns `.`, `..`, or anything containing a
/// separator.
#[must_use]
pub fn sanitize_segment(seg: &str) -> String {
    let out: String = seg
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = out.trim_matches('.');
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        write!(s, "{b:02x}").unwrap_or_default();
    }
    s
}

static BUNDLE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// One bundle's payload: the rendered sample plus its bindings.
#[derive(Debug)]
pub(crate) struct BundlePayload<'a> {
    /// The rendered sample (canonical + all four artifacts, PNG untagged).
    pub sample: &'a Sample,
    /// Binding-tagged PNG: what the PNG snapshot compares AND reviewers see.
    pub png_tagged: &'a [u8],
    /// Compound sample binding ([`super::sample_binding`]).
    pub binding: &'a str,
    /// Bare canonical generation ([`super::generation_id`]).
    pub generation: &'a str,
    /// Render identity string covered by the binding.
    pub render_identity: &'a str,
}

/// Write one complete candidate bundle and return its directory.
///
/// The manifest records the tagged-image hash AND the untagged payload hash
/// so the binding inputs stay auditable. The bundle is assembled in a
/// uniquely named temp dir and renamed into place, so a bundle directory
/// that exists is always complete.
///
/// # Errors
///
/// Returns [`AssertError`] when the scenario name is invalid or any write
/// fails. Nothing is published on error.
pub(crate) fn write_bundle_in(
    root: &Path,
    id: &EvidenceId,
    identity: &SnapshotIdentity,
    payload: &BundlePayload<'_>,
) -> Result<PathBuf, AssertError> {
    // Every path below joins the scenario name — reject escapes before any write.
    tuiscotti_core::names::validate_name(&id.scenario)
        .map_err(|e| AssertError::Io(e.to_string()))?;
    let io = |p: &Path, e: std::io::Error| AssertError::Io(format!("{}: {e}", p.display()));
    let final_dir = id.bundle_dir(root);
    if let Some(parent) = final_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    }
    let tmp = final_dir.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        BUNDLE_COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    // Best-effort cleanup of OUR OWN uniquely named temp dir only.
    let _stale = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| io(&tmp, e))?;
    let failed = |e: AssertError| {
        let _orphan = std::fs::remove_dir_all(&tmp);
        e
    };
    let put = |name: &str, bytes: &[u8]| -> Result<(), AssertError> {
        let p = tmp.join(name);
        std::fs::write(&p, bytes).map_err(|e| failed(io(&p, e)))
    };
    let sample = payload.sample;
    put("canonical.txt", sample.canonical.as_bytes())?;
    put("image.png", payload.png_tagged)?;
    put("sample.ansi", sample.ansi.as_bytes())?;
    put("sample.txt", sample.txt.as_bytes())?;
    put("sample.html", sample.html.as_bytes())?;
    let manifest = serde_json::json!({
        "schema": "tuiscotti-evidence-bundle/1",
        "binding": payload.binding,
        "generation": payload.generation,
        "render_identity": payload.render_identity,
        "snapshot": {
            "dir": identity.dir.display().to_string(),
            "canonical": identity.canonical,
            "png": identity.png_base,
        },
        "payload_sha256": {
            "canonical_txt": sha256_hex(sample.canonical.as_bytes()),
            "image_png_tagged": sha256_hex(payload.png_tagged),
            "image_png_untagged": sha256_hex(&sample.png),
            "sample_ansi": sha256_hex(sample.ansi.as_bytes()),
            "sample_txt": sha256_hex(sample.txt.as_bytes()),
            "sample_html": sha256_hex(sample.html.as_bytes()),
        },
        "run": {
            "package": id.package,
            "test": id.test,
            "scenario": id.scenario,
            "variant": id.variant,
            "run_id": id.attempt.run,
            "attempt": id.attempt.attempt,
            "stress_iter": id.attempt.stress_iter,
            "shard": id.attempt.shard,
            "attempt_uid": id.attempt.attempt_uid,
        },
    });
    let manifest_text = serde_json::to_string_pretty(&manifest)
        .map_err(|e| failed(AssertError::Io(format!("manifest encode: {e}"))))?;
    put("manifest.json", manifest_text.as_bytes())?;
    put("complete.json", b"{\"complete\":true}")?;
    // Same-process duplicate assert (same scenario+attempt twice): replace
    // the previous bundle; distinct attempts/shards never share this path.
    if final_dir.exists() {
        std::fs::remove_dir_all(&final_dir).map_err(|e| failed(io(&final_dir, e)))?;
    }
    std::fs::rename(&tmp, &final_dir).map_err(|e| failed(io(&final_dir, e)))?;
    Ok(final_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mid-publish failure (here: a blocker file where the final bundle
    /// dir must land, hit AFTER every artifact was staged in tmp) errors
    /// AND removes the staged tmp dir: no partial bundle, no tmp leftover,
    /// and the pre-existing path is preserved untouched.
    #[test]
    fn failed_bundle_writes_clean_their_tmp() {
        let root = tempfile::tempdir().expect("tempdir");
        let id = EvidenceId {
            package: "pkg".to_string(),
            test: "t".to_string(),
            scenario: "shot".to_string(),
            variant: None,
            attempt: AttemptIdentity::from_map(&HashMap::from([
                ("NEXTEST_RUN_ID".to_string(), "r".to_string()),
                ("NEXTEST_ATTEMPT".to_string(), "1".to_string()),
            ])),
        };
        let identity = SnapshotIdentity {
            dir: root.path().join("snaps"),
            canonical: "shot".to_string(),
            png_base: "shot-img".to_string(),
        };
        let final_dir = id.bundle_dir(root.path());
        std::fs::create_dir_all(final_dir.parent().expect("bundle parent"))
            .expect("create bundle parent");
        std::fs::write(&final_dir, b"blocker").expect("write blocker");
        let sample = Sample {
            canonical: "canonical".to_string(),
            ansi: "ansi".to_string(),
            txt: "txt".to_string(),
            html: "html".to_string(),
            png: b"fake-png".to_vec(),
        };
        let payload = BundlePayload {
            sample: &sample,
            png_tagged: b"fake-png-tagged",
            binding: "v2-test",
            generation: "gen",
            render_identity: "render",
        };
        let err = write_bundle_in(root.path(), &id, &identity, &payload)
            .expect_err("blocked bundle publish must fail");
        assert!(err.to_string().contains("attempt-1"), "{err}");
        let mut leftovers = Vec::new();
        let mut stack = vec![root.path().to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).expect("read dir") {
                let p = e.expect("dir entry").path();
                if p.file_name()
                    .is_some_and(|n| n.to_string_lossy().contains(".tmp-"))
                {
                    leftovers.push(p.clone());
                }
                if p.is_dir() {
                    stack.push(p);
                }
            }
        }
        assert!(leftovers.is_empty(), "tmp leftovers: {leftovers:?}");
        assert_eq!(
            std::fs::read(&final_dir).expect("blocker intact"),
            b"blocker"
        );
    }
}
