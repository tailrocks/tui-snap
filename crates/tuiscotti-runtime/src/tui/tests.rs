//! Unit tests: bounded joins and the cargo-bin resolver.

use std::ffi::OsStr;
use std::time::{Duration, Instant};

use super::builder::resolve_cargo_bin_with_map;
use super::session_teardown::join_one;
use super::shared::Shared;

#[test]
fn join_one_returns_for_clean_thread() {
    let shared = Shared::new();
    let h = std::thread::spawn(|| {});
    join_one(h, &shared, "worker", Duration::from_secs(5));
    assert_eq!(shared.teardown_error(), None);
}

#[test]
fn join_one_records_panic() {
    let shared = Shared::new();
    let h = std::thread::spawn(|| panic!("boom"));
    join_one(h, &shared, "reader", Duration::from_secs(5));
    assert_eq!(
        shared.teardown_error().as_deref(),
        Some("reader thread panicked")
    );
}

/// F5: a thread stuck forever (kill-failure stand-in for a reader
/// blocked in `read()`) must not hang teardown: bounded wait, then
/// detach with a diagnostic.
#[test]
fn join_one_detaches_stuck_thread() {
    let shared = Shared::new();
    let h = std::thread::Builder::new()
        .name("stuck-stand-in".to_string())
        .spawn(std::thread::park)
        .expect("spawn stuck-stand-in thread");
    let start = Instant::now();
    join_one(h, &shared, "reader", Duration::from_millis(50));
    assert!(start.elapsed() < Duration::from_secs(5), "join hung");
    let err = shared.teardown_error().expect("diagnostic recorded");
    assert!(err.contains("did not exit"), "{err}");
    assert!(err.contains("detached"), "{err}");
}

/// The PTY resolver delegates to the canonical `command` lookup: same
/// name plus same env must resolve identically through both paths.
#[test]
fn resolve_cargo_bin_matches_canonical_lookup() {
    use std::collections::HashMap;
    let dir = std::env::temp_dir().join(format!("tuiscotti-tui-resolve-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmpdir");
    let exe = dir.join("tuiscotti-g6-probe-xyz");
    std::fs::write(&exe, "fake").expect("write fake exe");
    let exe_s = exe.to_str().expect("utf8 tmp path").to_string();

    for var in crate::command::cargo_bin_env_names("tuiscotti-g6-probe-xyz") {
        let env = HashMap::from([(var, exe_s.clone())]);
        let via_tui = resolve_cargo_bin_with_map(OsStr::new("tuiscotti-g6-probe-xyz"), &env)
            .expect("tui hit");
        let via_command = crate::command::cargo_bin_path_with_map("tuiscotti-g6-probe-xyz", &env)
            .expect("command hit");
        assert_eq!(via_tui, via_command.into_os_string());
    }

    // Missing everywhere: both paths fail, and the tui error still names
    // the binary and the searched locations.
    let env = HashMap::new();
    let err = resolve_cargo_bin_with_map(OsStr::new("tuiscotti-g6-probe-xyz"), &env)
        .expect_err("missing binary must fail");
    let msg = err.to_string();
    assert!(msg.contains("tuiscotti-g6-probe-xyz"), "{msg}");
    assert!(msg.contains("searched:"), "{msg}");
    assert!(crate::command::cargo_bin_path_with_map("tuiscotti-g6-probe-xyz", &env).is_err());
}
