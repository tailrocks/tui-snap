//! G6 binary-resolution unification: one canonical lookup, three paths.
//!
//! `command::cargo_bin_path` is the canonical resolver; `tui` delegates to it
//! fully and `runner::resolve_bin` consults the same canonical env names
//! (keeping its nextest coverage, ambiguity refusal, and no-probing rule).
//! These tests prove the three paths resolve the same name identically.
//!
//! No process-env mutation (`unsafe_code` is forbidden): everything goes
//! through the `*_with_map` pure forms with injected maps.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tuiscotti::command::{cargo_bin_env_names, cargo_bin_path_with_map};
use tuiscotti::runner::{ResolveError, resolve_bin_with_map};

const BIN: &str = "tuisnap-g6-unify-xyz";

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn fake_exe(dir: &Path, name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let p = dir.join(name);
    fs::write(&p, "fake")?;
    Ok(p)
}

#[test]
fn canonical_env_names_are_exact_then_normalized() {
    assert_eq!(
        cargo_bin_env_names(BIN),
        vec![
            format!("CARGO_BIN_EXE_{BIN}"),
            "CARGO_BIN_EXE_TUISNAP_G6_UNIFY_XYZ".to_string(),
        ]
    );
    // Already-normalized names yield a single entry (no duplicate lookup).
    assert_eq!(
        cargo_bin_env_names("TUISNAP_G6_UNIFY_XYZ"),
        vec!["CARGO_BIN_EXE_TUISNAP_G6_UNIFY_XYZ".to_string()]
    );
    // Directory components are stripped to the file name.
    assert_eq!(
        cargo_bin_env_names("target/debug/my-prog"),
        cargo_bin_env_names("my-prog")
    );
}

#[test]
fn command_and_runner_resolve_same_name_identically() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let exe = fake_exe(tmp.path(), "unify-probe").expect("fake_exe succeeds");
    let exe_s = exe.to_str().expect("exe to_str succeeds");

    // Exact env hit: identical results through both public paths.
    for var in cargo_bin_env_names(BIN) {
        let e = env(&[(var.as_str(), exe_s)]);
        let via_command = cargo_bin_path_with_map(BIN, &e).expect("cargo_bin_path succeeds");
        let via_runner = resolve_bin_with_map("pkg", BIN, &e).expect("resolve_bin succeeds");
        assert_eq!(via_command, exe);
        assert_eq!(via_runner, exe);
        assert_eq!(via_command, via_runner);
    }
    // (The third path, `tui`, delegates to `command::cargo_bin_path` by
    // construction; its identity is pinned by the unit test
    // `resolve_cargo_bin_matches_canonical_lookup` in `tui.rs`, since the
    // PTY resolver has no injectable-env public form.)
}

#[test]
fn command_prefers_exact_over_normalized() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let exact_exe = fake_exe(tmp.path(), "exact-probe").expect("fake_exe succeeds");
    let norm_exe = fake_exe(tmp.path(), "norm-probe").expect("fake_exe succeeds");
    let vars = cargo_bin_env_names(BIN);
    assert_eq!(vars.len(), 2);
    let e = env(&[
        (
            vars[0].as_str(),
            exact_exe.to_str().expect("exe to_str succeeds"),
        ),
        (
            vars[1].as_str(),
            norm_exe.to_str().expect("exe to_str succeeds"),
        ),
    ]);
    assert_eq!(
        cargo_bin_path_with_map(BIN, &e).expect("cargo_bin_path succeeds"),
        exact_exe
    );
}

#[test]
fn runner_still_refuses_ambiguous_hits() {
    // Intentional contract difference, preserved: where `command` picks the
    // first hit in canonical order, `runner` errors instead of guessing.
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let a = fake_exe(tmp.path(), "amb-a").expect("fake_exe succeeds");
    let b = fake_exe(tmp.path(), "amb-b").expect("fake_exe succeeds");
    let vars = cargo_bin_env_names(BIN);
    let e = env(&[
        (vars[0].as_str(), a.to_str().expect("exe to_str succeeds")),
        (vars[1].as_str(), b.to_str().expect("exe to_str succeeds")),
    ]);
    match resolve_bin_with_map("pkg", BIN, &e) {
        Err(ResolveError::Ambiguous { candidates, .. }) => {
            assert_eq!(candidates.len(), 2);
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

#[test]
fn set_but_missing_env_values_are_skipped_not_returned() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let missing = tmp.path().join("not-there");
    assert!(!missing.exists());
    // Exact points nowhere, normalized hits: normalized wins.
    let exe = fake_exe(tmp.path(), "fallback-probe").expect("fake_exe succeeds");
    let vars = cargo_bin_env_names(BIN);
    let e = env(&[
        (
            vars[0].as_str(),
            missing.to_str().expect("path to_str succeeds"),
        ),
        (vars[1].as_str(), exe.to_str().expect("exe to_str succeeds")),
    ]);
    assert_eq!(
        cargo_bin_path_with_map(BIN, &e).expect("cargo_bin_path succeeds"),
        exe
    );
    assert_eq!(
        resolve_bin_with_map("pkg", BIN, &e).expect("resolve_bin succeeds"),
        exe
    );
}

#[test]
fn missing_binaries_list_what_was_searched() {
    let e = HashMap::new();
    let err = cargo_bin_path_with_map(BIN, &e).expect_err("missing bin is an error");
    assert_eq!(
        err.kind(),
        tuiscotti::command::SpawnErrorKind::BinaryNotFound
    );
    assert!(!err.searched().is_empty(), "searched: {err}");
    assert!(err.detail().contains(BIN), "{err}");
    for var in cargo_bin_env_names(BIN) {
        assert!(err.detail().contains(&var), "{err}");
    }
    match resolve_bin_with_map("pkg", BIN, &e) {
        Err(ResolveError::Missing { searched, .. }) => {
            for var in cargo_bin_env_names(BIN) {
                assert!(searched.contains(&var), "{searched:?}");
            }
        }
        other => panic!("expected Missing, got {other:?}"),
    }
}
