use super::*;
use std::collections::HashMap;
use tuiscotti::runner::{AttemptId, BaselineId, JunitKey, is_nextest_map};

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
