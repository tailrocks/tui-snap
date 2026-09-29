//! Integration tests for the runner-neutral context + nextest adapter.
//!
//! These tests never mutate the process environment or CWD (parallel-safe
//! under both libtest and nextest); all env-driven behavior goes through the
//! `*_with_map` / `from_map` constructors with injected maps.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tuiscotti::runner::{
    AttemptId, BaselineId, Journal, JournalStatus, JunitKey, ManifestVerdict, ResolveError,
    ScenarioManifest, TestContext, child_command, is_nextest, is_nextest_map, resolve_bin,
    resolve_bin_with_map,
};

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn nextest_env(root: &Path) -> HashMap<String, String> {
    env(&[
        ("NEXTEST_RUN_ID", "1f79aa0d-4ec8-4a5c-aa83-5e8dc2f36573"),
        ("NEXTEST_BINARY_ID", "tuiscotti::runner"),
        ("NEXTEST_TEST_NAME", "env_parsing"),
        ("NEXTEST_ATTEMPT", "2"),
        ("NEXTEST_TOTAL_ATTEMPTS", "3"),
        ("NEXTEST_ATTEMPT_ID", "1f79$abc"),
        ("NEXTEST_STRESS_CURRENT", "none"),
        ("NEXTEST_STRESS_TOTAL", "none"),
        ("NEXTEST_PROFILE", "ci"),
        ("NEXTEST_VERSION", "0.9.143"),
        (
            "NEXTEST_WORKSPACE_ROOT",
            root.to_str().expect("utf8 tmp path"),
        ),
        ("NEXTEST_EXECUTION_MODE", "process-per-test"),
    ])
}

fn tmp_root(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "tuisnap-runner-{}-{}-{}",
        tag,
        std::process::id(),
        // nanos make parallel nextest processes distinct
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn env_parsing_nextest_shape() {
    let root = tmp_root("parse");
    let e = nextest_env(&root);
    assert!(is_nextest_map(&e));

    let b = BaselineId::from_map("scenario-a", &e, "/fallback");
    assert_eq!(b.workspace, root.to_str().unwrap());
    assert_eq!(b.package, "tuiscotti");
    assert_eq!(b.binary, "runner");
    assert_eq!(b.test, "env_parsing");
    assert_eq!(b.scenario, "scenario-a");
    assert_eq!(b.profile, "ci");
    assert_eq!(b.variant, None);

    let a = AttemptId::from_map(&e);
    assert_eq!(a.run, "1f79aa0d-4ec8-4a5c-aa83-5e8dc2f36573");
    assert_eq!(a.attempt, 2);
    assert_eq!(a.stress_iter, None);
    assert_eq!(a.shard, None);
    assert_eq!(a.attempt_uid.as_deref(), Some("1f79$abc"));

    // Stable key excludes run/attempt: retry keeps identity.
    let mut e2 = e.clone();
    e2.insert("NEXTEST_RUN_ID".into(), "other-run".into());
    e2.insert("NEXTEST_ATTEMPT".into(), "3".into());
    let b2 = BaselineId::from_map("scenario-a", &e2, "/fallback");
    assert_eq!(b.stable_key(), b2.stable_key());
}

#[test]
fn env_parsing_absent_env_fallback() {
    let e = HashMap::new();
    assert!(!is_nextest_map(&e));

    let b = BaselineId::from_map("s", &e, "/fallback-ws");
    assert_eq!(b.workspace, "/fallback-ws");
    assert_eq!(b.package, "unknown-package");
    assert_eq!(b.binary, "unknown-package");
    assert_eq!(b.test, "unknown-test");
    assert_eq!(b.profile, "local");

    let a1 = AttemptId::from_map(&e);
    let a2 = AttemptId::from_map(&e);
    assert_eq!(a1.attempt, 0);
    assert!(a1.run.starts_with("local-"));
    assert_ne!(a1.run, a2.run, "local run ids must be unique");
    assert_eq!(a1.stress_iter, None);
    assert!(JunitKey::from_map(&e).is_none());
}

#[test]
fn env_parsing_binary_id_shapes_and_stress() {
    // Unit-test binary id is bare crate name.
    let e = env(&[("NEXTEST_BINARY_ID", "my-crate")]);
    let b = BaselineId::from_map("s", &e, "/w");
    assert_eq!(
        (b.package.as_str(), b.binary.as_str()),
        ("my-crate", "my-crate")
    );

    // Bench/example shape keeps kind prefix on binary.
    let e = env(&[("NEXTEST_BINARY_ID", "my-crate::bench/perf")]);
    let b = BaselineId::from_map("s", &e, "/w");
    assert_eq!(
        (b.package.as_str(), b.binary.as_str()),
        ("my-crate", "bench/perf")
    );

    // Garbage attempt is lossy-zero, never a panic.
    let e = env(&[("NEXTEST_ATTEMPT", "bogus")]);
    assert_eq!(AttemptId::from_map(&e).attempt, 0);

    // Stress index parses; "none" means no stress run.
    let e = env(&[("NEXTEST_STRESS_CURRENT", "3")]);
    assert_eq!(AttemptId::from_map(&e).stress_iter, Some(3));
    let e = env(&[("NEXTEST_STRESS_CURRENT", "none")]);
    assert_eq!(AttemptId::from_map(&e).stress_iter, None);

    // Workspace fallback chain: manifest dir, then caller fallback.
    let e = env(&[("CARGO_MANIFEST_DIR", "/manifest")]);
    assert_eq!(BaselineId::from_map("s", &e, "/fb").workspace, "/manifest");

    // Variant/shard are caller-set, never inferred.
    let b = BaselineId::from_map("s", &HashMap::new(), "/w").with_variant("dark");
    assert_eq!(b.variant.as_deref(), Some("dark"));
    let a = AttemptId::from_map(&HashMap::new()).with_shard("0/4");
    assert_eq!(a.shard.as_deref(), Some("0/4"));
    assert!(a.dir_suffix().contains("shard-0_4"));
}

#[test]
fn live_runner_consistency() {
    // Passes under both runners: under nextest the live context must show a
    // 1-indexed attempt and a JUnit key; otherwise a local attempt-0 identity.
    let ctx = TestContext::current("live-check").unwrap();
    if is_nextest() {
        assert!(ctx.is_nextest());
        assert!(ctx.attempt().attempt >= 1);
        assert!(JunitKey::current().is_some());
    } else {
        assert!(!ctx.is_nextest());
        assert_eq!(ctx.attempt().attempt, 0);
        assert!(JunitKey::current().is_none());
    }
}

#[test]
fn isolation_two_contexts_distinct_dirs() {
    let root = tmp_root("isol");
    let e1 = nextest_env(&root);
    let mut e2 = e1.clone();
    e2.insert("NEXTEST_ATTEMPT".into(), "3".into());

    let c1 = TestContext::from_map("s", &e1, &root).unwrap();
    let c2 = TestContext::from_map("s", &e2, &root).unwrap();
    assert_ne!(c1.scratch_dir(), c2.scratch_dir());
    assert!(c1.scratch_dir().is_dir() && c2.scratch_dir().is_dir());
    assert!(c1.scratch_dir().join("evidence").is_dir());

    // Same attempt twice still collides safely: second claim gets a suffix.
    let c1b = TestContext::from_map("s", &e1, &root).unwrap();
    assert_ne!(c1.scratch_dir(), c1b.scratch_dir());
    assert!(c1b.scratch_dir().is_dir());
}

#[test]
fn child_env_and_home_isolation_are_child_only() {
    let root = tmp_root("child");
    let ctx = TestContext::from_map("s", &nextest_env(&root), &root).unwrap();

    let vars: HashMap<_, _> = ctx.child_env().into_iter().collect();
    assert_eq!(vars["TUISNAP_ATTEMPT"], "2");
    assert_eq!(vars["TUISNAP_SCENARIO"], "s");
    assert!(vars["TUISNAP_BASELINE"].contains("tuisnap"));

    let mut cmd = child_command(&ctx, "true");
    ctx.apply_home_isolation(&mut cmd).unwrap();
    let home = ctx.scratch_dir().join("home");
    assert!(home.join("config").is_dir());
    assert!(home.join("run").is_dir());
    let got: HashMap<_, _> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.unwrap().to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert_eq!(got["HOME"], home.to_string_lossy());
    assert_eq!(got["TUISNAP_RUN_ID"], ctx.attempt().run);

    // Parent process untouched.
    assert!(std::env::var_os("TUISNAP_RUN_ID").is_none());
    assert_ne!(
        std::env::var_os("HOME").unwrap(),
        std::ffi::OsString::from(home.to_string_lossy().into_owned())
    );
}

#[test]
fn resolve_bin_prefers_nextest_and_dedupes_forms() {
    let root = tmp_root("res");
    let exe = root.join("my-prog");
    fs::write(&exe, "fake").unwrap();
    let exe_s = exe.to_str().unwrap().to_string();

    // Both hyphen and underscore forms set to the same file: one result.
    let e = env(&[
        ("NEXTEST_BIN_EXE_my-prog", exe_s.as_str()),
        ("NEXTEST_BIN_EXE_my_prog", exe_s.as_str()),
    ]);
    assert_eq!(resolve_bin_with_map("pkg", "my-prog", &e).unwrap(), exe);

    // Cargo fallback works when nextest vars are absent.
    let e = env(&[("CARGO_BIN_EXE_my-prog", exe_s.as_str())]);
    assert_eq!(resolve_bin_with_map("pkg", "my-prog", &e).unwrap(), exe);
}

#[test]
fn resolve_bin_ambiguity_errors_never_silent_pick() {
    let root = tmp_root("amb");
    let a = root.join("a-prog");
    let b = root.join("b-prog");
    fs::write(&a, "a").unwrap();
    fs::write(&b, "b").unwrap();

    let e = env(&[
        ("NEXTEST_BIN_EXE_my-prog", a.to_str().unwrap()),
        ("CARGO_BIN_EXE_my-prog", b.to_str().unwrap()),
    ]);
    match resolve_bin_with_map("pkg", "my-prog", &e) {
        Err(ResolveError::Ambiguous { candidates, .. }) => {
            assert_eq!(candidates.len(), 2);
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }

    // Set-but-missing values are reported, not probed into target/.
    let e = env(&[("NEXTEST_BIN_EXE_my-prog", "/nonexistent/xyz")]);
    match resolve_bin_with_map("pkg", "my-prog", &e) {
        Err(ResolveError::Missing {
            searched, values, ..
        }) => {
            assert!(searched.iter().any(|s| s == "NEXTEST_BIN_EXE_my-prog"));
            assert_eq!(values.len(), 1);
        }
        other => panic!("expected Missing, got {other:?}"),
    }

    // Empty env: missing, listing what was searched.
    match resolve_bin_with_map("pkg", "my-prog", &HashMap::new()) {
        Err(ResolveError::Missing { searched, .. }) => assert!(!searched.is_empty()),
        other => panic!("expected Missing, got {other:?}"),
    }
}

#[test]
fn resolve_bin_live_env_best_effort() {
    // Under nextest or cargo test the tuisnap binary vars may be present; when
    // present they must resolve to a real file. Absent env must yield Missing,
    // never a guessed path or Ambiguous-from-nothing.
    match resolve_bin("tuisnap", "tuisnap") {
        Ok(p) => assert!(p.is_file(), "resolved path must exist: {}", p.display()),
        Err(ResolveError::Missing { .. }) => {}
        Err(e) => panic!("unexpected live resolve error: {e}"),
    }
}

#[test]
fn manifest_full_partial_incomplete() {
    let root = tmp_root("man");
    let manifest_path = root.join("required.txt");
    let record_path = root.join("record.txt");

    let m = ScenarioManifest::new(["alpha", "beta"]);
    m.save(&manifest_path).unwrap();
    let loaded = ScenarioManifest::load(&manifest_path).unwrap();
    assert_eq!(
        loaded.required(),
        &["alpha".to_string(), "beta".to_string()]
    );

    // No record file: incomplete, never full.
    assert!(matches!(
        m.evaluate_record(&record_path),
        ManifestVerdict::Incomplete { .. }
    ));

    // Filtered run: only alpha executed -> partial naming beta.
    ScenarioManifest::record_execution(&record_path, "alpha").unwrap();
    match m.evaluate_record(&record_path) {
        ManifestVerdict::Partial { missing } => assert_eq!(missing, ["beta"]),
        other => panic!("expected Partial, got {other:?}"),
    }
    assert!(!m.evaluate_record(&record_path).is_full());

    // Full run: both executed -> full.
    ScenarioManifest::record_execution(&record_path, "beta").unwrap();
    match m.evaluate_record(&record_path) {
        ManifestVerdict::Full { executed } => assert_eq!(executed, 2),
        other => panic!("expected Full, got {other:?}"),
    }

    // Empty manifest can never gate as full.
    let empty = ScenarioManifest::new(Vec::<String>::new());
    assert!(matches!(
        empty.evaluate(&[]),
        ManifestVerdict::Incomplete { .. }
    ));
}

#[test]
fn journal_killed_attempt_incomplete_completed_is_complete() {
    let root = tmp_root("jrnl");
    let dir = root.join("attempt-1");
    fs::create_dir_all(&dir).unwrap();

    // Missing journal entirely: incomplete.
    assert!(matches!(
        Journal::status(&dir),
        JournalStatus::Incomplete { .. }
    ));

    // Killed attempt: events flushed, no completion marker -> incomplete.
    let mut j = Journal::open(&dir.join("journal.jsonl")).unwrap();
    j.append("start", "attempt begins").unwrap();
    j.append("capture", "frame \"quoted\" \\ done").unwrap();
    drop(j);
    let st = Journal::status(&dir);
    assert!(matches!(st, JournalStatus::Incomplete { .. }), "got {st:?}");
    // …but the flushed events stay inspectable.
    let text = fs::read_to_string(dir.join("journal.jsonl")).unwrap();
    assert!(text.contains("\"seq\":0") && text.contains("\"seq\":1"));

    // Explicit completion -> complete with the terminal status.
    let mut j = Journal::open(&dir.join("journal.jsonl")).unwrap();
    j.complete("pass").unwrap();
    match Journal::status(&dir) {
        JournalStatus::Complete { status } => assert_eq!(status, "pass"),
        other => panic!("expected Complete, got {other:?}"),
    }
}

#[test]
fn retry_preserves_failed_attempt_evidence() {
    let root = tmp_root("retry");
    let e1 = nextest_env(&root);
    let mut e2 = e1.clone();
    e2.insert("NEXTEST_ATTEMPT".into(), "3".into());

    let failed = TestContext::from_map("s", &e1, &root).unwrap();
    let marker = failed.evidence_dir().join("failure.txt");
    fs::write(&marker, "attempt-2 evidence").unwrap();
    let mut j = Journal::open(&failed.journal_path()).unwrap();
    j.append("fail", "assertion broke").unwrap();
    drop(j);

    // Retry (attempt 3) gets its own dirs; failed attempt's files untouched.
    let retry = TestContext::from_map("s", &e2, &root).unwrap();
    assert_ne!(failed.scratch_dir(), retry.scratch_dir());
    assert_eq!(fs::read_to_string(&marker).unwrap(), "attempt-2 evidence");
    assert!(matches!(
        Journal::status(failed.scratch_dir()),
        JournalStatus::Incomplete { .. }
    ));
    let mut j = Journal::open(&retry.journal_path()).unwrap();
    j.complete("pass").unwrap();
    assert!(Journal::status(retry.scratch_dir()).is_complete());
    // Failed attempt still incomplete afterwards: success never rewrites it.
    assert!(!Journal::status(failed.scratch_dir()).is_complete());
}

#[test]
fn junit_correlation_read_only() {
    let root = tmp_root("junit");
    let key = JunitKey::from_map(&nextest_env(&root)).unwrap();
    assert!(key.matches("tuiscotti::runner", "env_parsing"));
    assert!(!key.matches("tuiscotti::runner", "other-test"));
    assert!(!key.matches("other::bin", "env_parsing"));
    assert!(key.run_matches("1f79aa0d-4ec8-4a5c-aa83-5e8dc2f36573"));
    assert!(!key.run_matches("other-run"));

    // Partial identity is refused: no key, no invented correlation.
    let partial = env(&[("NEXTEST_RUN_ID", "r")]);
    assert!(JunitKey::from_map(&partial).is_none());
}

#[test]
fn no_global_mutation() {
    let env_before: HashMap<String, String> = std::env::vars().collect();
    let cwd_before = std::env::current_dir().unwrap();

    let _ = TestContext::current("no-mutation-probe").unwrap();
    let _ = BaselineId::from_env("probe");
    let _ = AttemptId::from_env();
    let _ = is_nextest();
    let _ = resolve_bin("tuisnap", "tuisnap");
    let _ = JunitKey::current();

    let env_after: HashMap<String, String> = std::env::vars().collect();
    assert_eq!(env_before, env_after, "process env must be unchanged");
    assert_eq!(
        cwd_before,
        std::env::current_dir().unwrap(),
        "CWD must be unchanged"
    );
}
