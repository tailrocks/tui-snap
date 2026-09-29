use super::*;
use std::collections::HashMap;
use std::fs;
use tuiscotti::runner::{
    AttemptId, BaselineId, Journal, JournalStatus, JunitKey, ManifestVerdict, ScenarioManifest,
    TestContext, is_nextest, resolve_bin,
};

#[test]
fn manifest_full_partial_incomplete() {
    let root = tmp_root("man").expect("tmp_root succeeds");
    let manifest_path = root.join("required.txt");
    let record_path = root.join("record.txt");

    let m = ScenarioManifest::new(["alpha", "beta"]);
    m.save(&manifest_path).expect("manifest save succeeds");
    let loaded = ScenarioManifest::load(&manifest_path).expect("manifest load succeeds");
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
    ScenarioManifest::record_execution(&record_path, "alpha").expect("record execution succeeds");
    match m.evaluate_record(&record_path) {
        ManifestVerdict::Partial { missing } => assert_eq!(missing, ["beta"]),
        other => panic!("expected Partial, got {other:?}"),
    }
    assert!(!m.evaluate_record(&record_path).is_full());

    // Full run: both executed -> full.
    ScenarioManifest::record_execution(&record_path, "beta").expect("record execution succeeds");
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
    let root = tmp_root("jrnl").expect("tmp_root succeeds");
    let dir = root.join("attempt-1");
    fs::create_dir_all(&dir).expect("create_dir_all succeeds");

    // Missing journal entirely: incomplete.
    assert!(matches!(
        Journal::status(&dir),
        JournalStatus::Incomplete { .. }
    ));

    // Killed attempt: events flushed, no completion marker -> incomplete.
    let mut j = Journal::open(&dir.join("journal.jsonl")).expect("journal open succeeds");
    j.append("start", "attempt begins")
        .expect("journal append succeeds");
    j.append("capture", "frame \"quoted\" \\ done")
        .expect("journal append succeeds");
    drop(j);
    let st = Journal::status(&dir);
    assert!(matches!(st, JournalStatus::Incomplete { .. }), "got {st:?}");
    // …but the flushed events stay inspectable.
    let text = fs::read_to_string(dir.join("journal.jsonl")).expect("read journal succeeds");
    assert!(text.contains("\"seq\":0") && text.contains("\"seq\":1"));

    // Explicit completion -> complete with the terminal status.
    let mut j = Journal::open(&dir.join("journal.jsonl")).expect("journal open succeeds");
    j.complete("pass").expect("journal complete succeeds");
    match Journal::status(&dir) {
        JournalStatus::Complete { status } => assert_eq!(status, "pass"),
        other @ JournalStatus::Incomplete { .. } => {
            panic!("expected Complete, got {other:?}")
        }
    }
}

#[test]
fn retry_preserves_failed_attempt_evidence() {
    let root = tmp_root("retry").expect("tmp_root succeeds");
    let e1 = nextest_env(&root).expect("nextest_env succeeds");
    let mut e2 = e1.clone();
    e2.insert("NEXTEST_ATTEMPT".into(), "3".into());

    let failed = TestContext::from_map("s", &e1, &root).expect("context from_map succeeds");
    let marker = failed.evidence_dir().join("failure.txt");
    fs::write(&marker, "attempt-2 evidence").expect("write evidence succeeds");
    let mut j = Journal::open(&failed.journal_path()).expect("journal open succeeds");
    j.append("fail", "assertion broke")
        .expect("journal append succeeds");
    drop(j);

    // Retry (attempt 3) gets its own dirs; failed attempt's files untouched.
    let retry = TestContext::from_map("s", &e2, &root).expect("context from_map succeeds");
    assert_ne!(failed.scratch_dir(), retry.scratch_dir());
    assert_eq!(
        fs::read_to_string(&marker).expect("read evidence succeeds"),
        "attempt-2 evidence"
    );
    assert!(matches!(
        Journal::status(failed.scratch_dir()),
        JournalStatus::Incomplete { .. }
    ));
    let mut j = Journal::open(&retry.journal_path()).expect("journal open succeeds");
    j.complete("pass").expect("journal complete succeeds");
    assert!(Journal::status(retry.scratch_dir()).is_complete());
    // Failed attempt still incomplete afterwards: success never rewrites it.
    assert!(!Journal::status(failed.scratch_dir()).is_complete());
}

#[test]
fn junit_correlation_read_only() {
    let root = tmp_root("junit").expect("tmp_root succeeds");
    let key = JunitKey::from_map(&nextest_env(&root).expect("nextest_env succeeds"))
        .expect("junit key succeeds");
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
    let cwd_before = std::env::current_dir().expect("current_dir succeeds");

    let _ = TestContext::current("no-mutation-probe").expect("probe context succeeds");
    drop(BaselineId::from_env("probe"));
    drop(AttemptId::from_env());
    let _nextest_probe = is_nextest();
    drop(resolve_bin("tuiscotti", "tuiscotti"));
    drop(JunitKey::current());

    let env_after: HashMap<String, String> = std::env::vars().collect();
    assert_eq!(env_before, env_after, "process env must be unchanged");
    assert_eq!(
        cwd_before,
        std::env::current_dir().expect("current_dir succeeds"),
        "CWD must be unchanged"
    );
}
