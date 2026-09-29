use super::*;
use std::collections::HashMap;
use std::fs;
use tuiscotti::runner::{
    AttemptId, BaselineId, Journal, JournalStatus, JunitKey, ManifestVerdict, ScenarioManifest,
    TestContext, is_nextest, resolve_bin,
};

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
