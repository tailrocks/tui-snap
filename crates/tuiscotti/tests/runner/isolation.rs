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
