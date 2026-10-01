use super::*;
use std::collections::HashMap;
use std::fs;
use tuiscotti::runner::{
    JunitKey, ResolveError, TestContext, child_command, is_nextest, resolve_bin,
    resolve_bin_with_map,
};

#[test]
fn live_runner_consistency() {
    // Passes under both runners: under nextest the live context must show a
    // 1-indexed attempt and a JUnit key; otherwise a local attempt-0 identity.
    let ctx = TestContext::current("live-check").expect("live context succeeds");
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
    let root = tmp_root("isol").expect("tmp_root succeeds");
    let e1 = nextest_env(&root).expect("nextest_env succeeds");
    let mut e2 = e1.clone();
    e2.insert("NEXTEST_ATTEMPT".into(), "3".into());

    let c1 = TestContext::from_map("s", &e1, &root).expect("context from_map succeeds");
    let c2 = TestContext::from_map("s", &e2, &root).expect("context from_map succeeds");
    assert_ne!(c1.scratch_dir(), c2.scratch_dir());
    assert!(c1.scratch_dir().is_dir() && c2.scratch_dir().is_dir());
    assert!(c1.scratch_dir().join("evidence").is_dir());

    // Same attempt twice still collides safely: second claim gets a suffix.
    let c1b = TestContext::from_map("s", &e1, &root).expect("context from_map succeeds");
    assert_ne!(c1.scratch_dir(), c1b.scratch_dir());
    assert!(c1b.scratch_dir().is_dir());
}

#[test]
fn child_env_and_home_isolation_are_child_only() {
    let root = tmp_root("child").expect("tmp_root succeeds");
    let ctx = TestContext::from_map(
        "s",
        &nextest_env(&root).expect("nextest_env succeeds"),
        &root,
    )
    .expect("context from_map succeeds");

    let vars: HashMap<_, _> = ctx.child_env().into_iter().collect();
    assert_eq!(vars["TUISCOTTI_ATTEMPT"], "2");
    assert_eq!(vars["TUISCOTTI_SCENARIO"], "s");
    assert!(vars["TUISCOTTI_BASELINE"].contains("tuiscotti"));

    let mut cmd = child_command(&ctx, "true");
    ctx.apply_home_isolation(&mut cmd)
        .expect("home isolation succeeds");
    let home = ctx.scratch_dir().join("home");
    assert!(home.join("config").is_dir());
    assert!(home.join("run").is_dir());
    let got: HashMap<_, _> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.expect("env value present").to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert_eq!(got["HOME"], home.to_string_lossy());
    assert_eq!(got["TUISCOTTI_RUN_ID"], ctx.attempt().run);

    // Parent process untouched.
    assert!(std::env::var_os("TUISCOTTI_RUN_ID").is_none());
    assert_ne!(
        std::env::var_os("HOME").expect("HOME is set"),
        std::ffi::OsString::from(home.to_string_lossy().into_owned())
    );
}

#[test]
fn resolve_bin_prefers_nextest_and_dedupes_forms() {
    let root = tmp_root("res").expect("tmp_root succeeds");
    let exe = root.join("my-prog");
    fs::write(&exe, "fake").expect("write fake exe succeeds");
    let exe_s = exe.to_str().expect("exe to_str succeeds").to_string();

    // Both hyphen and underscore forms set to the same file: one result.
    let e = env(&[
        ("NEXTEST_BIN_EXE_my-prog", exe_s.as_str()),
        ("NEXTEST_BIN_EXE_my_prog", exe_s.as_str()),
    ]);
    assert_eq!(
        resolve_bin_with_map("pkg", "my-prog", &e).expect("resolve_bin succeeds"),
        exe
    );

    // Cargo fallback works when nextest vars are absent.
    let e = env(&[("CARGO_BIN_EXE_my-prog", exe_s.as_str())]);
    assert_eq!(
        resolve_bin_with_map("pkg", "my-prog", &e).expect("resolve_bin succeeds"),
        exe
    );
}

#[test]
fn resolve_bin_ambiguity_errors_never_silent_pick() {
    let root = tmp_root("amb").expect("tmp_root succeeds");
    let a = root.join("a-prog");
    let b = root.join("b-prog");
    fs::write(&a, "a").expect("write fake succeeds");
    fs::write(&b, "b").expect("write fake succeeds");

    let e = env(&[
        (
            "NEXTEST_BIN_EXE_my-prog",
            a.to_str().expect("path to_str succeeds"),
        ),
        (
            "CARGO_BIN_EXE_my-prog",
            b.to_str().expect("path to_str succeeds"),
        ),
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
    // Under nextest or cargo test the tuiscotti binary vars may be present; when
    // present they must resolve to a real file. Absent env must yield Missing,
    // never a guessed path or Ambiguous-from-nothing.
    match resolve_bin("tuiscotti", "tuiscotti") {
        Ok(p) => assert!(p.is_file(), "resolved path must exist: {}", p.display()),
        Err(ResolveError::Missing { .. }) => {}
        Err(e) => panic!("unexpected live resolve error: {e}"),
    }
}
