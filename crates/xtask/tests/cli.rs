//! CLI contract: help, subcommand forwarding, recursion guard.

use std::process::Command;

fn xtask() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
}

#[test]
fn help_exits_zero() {
    let out = xtask().arg("--help").output().expect("spawn xtask");
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("usage: cargo xtask"), "help text: {stdout}");
}

#[test]
fn every_subcommand_accepts_help() {
    for sub in [
        "migrate", "policy", "fixtures", "brand", "deps", "docs", "perf", "package", "fonts",
    ] {
        let out = xtask()
            .arg(sub)
            .arg("--help")
            .output()
            .expect("spawn xtask");
        assert_eq!(out.status.code(), Some(0), "xtask {sub} --help");
    }
}

#[test]
fn unknown_subcommand_forwards_and_exits_two() {
    // Proves cargo forwards custom argv to xtask (we see the literal token).
    let out = xtask()
        .arg("definitely-not-a-subcommand")
        .output()
        .expect("spawn xtask");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("definitely-not-a-subcommand"),
        "stderr: {stderr}"
    );
}

#[test]
fn recursion_guard_refuses_nested_run() {
    let out = xtask()
        .env("TUISCOTTI_XTASK_ACTIVE", "1")
        .arg("policy")
        .output()
        .expect("spawn xtask");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("recursive"), "stderr: {stderr}");
}
