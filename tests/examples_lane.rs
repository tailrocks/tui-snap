//! Executed examples lane (backlog N10): every learning example runs green.
//!
//! Each `examples/NN-*.rs` binary must exit 0 and print its `EXAMPLE-NN-...`
//! marker. Binaries resolve at runtime from this test's own location
//! (`<profile>/deps/<test>` → `<profile>/examples/<name>`), so the lane works
//! under `cargo test`, `cargo nextest run`, custom target dirs, and
//! release profiles. Missing binaries trigger one `cargo build --examples`.
//!
//! Doctest lane: `cargo test --doc` runs 5 doctests (recorded 2026-09-28;
//! re-check with `cargo test --doc 2>&1 | tail -3`).

use std::path::PathBuf;
use std::process::Command;

/// (example binary, acceptable stdout markers — first is the normal one).
const CASES: &[(&str, &[&str])] = &[
    ("01-pure-view", &["EXAMPLE-01-OK"]),
    ("02-styled-shot", &["EXAMPLE-02-OK"]),
    ("03-piped-cli", &["EXAMPLE-03-OK"]),
    // Without the `pty` feature the TUI journey reports SKIP, still exit 0.
    ("04-interactive-tui", &["EXAMPLE-04-OK", "EXAMPLE-04-SKIP"]),
    ("05-locators-waits", &["EXAMPLE-05-OK"]),
    ("06-artifacts-review", &["EXAMPLE-06-OK"]),
    ("07-advanced-profiles", &["EXAMPLE-07-OK"]),
    ("08-agent-workflow", &["EXAMPLE-08-OK"]),
];

fn examples_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("current test exe");
    // .../target/<profile>/deps/examples_lane-<hash> → .../target/<profile>
    let profile = exe
        .parent()
        .and_then(|d| d.parent())
        .expect("profile dir above deps/")
        .to_path_buf();
    profile.join("examples")
}

fn build_examples(profile_dir_name: Option<&str>) {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(cargo);
    cmd.args(["build", "--examples"]);
    if profile_dir_name == Some("release") {
        cmd.arg("--release");
    }
    // Run from the package root so plain `cargo build` resolves the manifest.
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    let out = cmd.output().expect("run cargo build --examples");
    assert!(
        out.status.success(),
        "cargo build --examples failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn examples_lane_executes_everything() {
    let dir = examples_dir();
    let profile_name = dir
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned());
    let exe = |name: &str| dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if CASES.iter().any(|(n, _)| !exe(n).is_file()) {
        build_examples(profile_name.as_deref());
    }

    let mut failures = Vec::new();
    for (name, markers) in CASES {
        let path = exe(name);
        if !path.is_file() {
            failures.push(format!("{name}: binary missing at {}", path.display()));
            continue;
        }
        match Command::new(&path).output() {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
                if !out.status.success() {
                    failures.push(format!(
                        "{name}: exit {}:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
                        out.status
                    ));
                } else if !markers.iter().any(|m| stdout.contains(m)) {
                    failures.push(format!(
                        "{name}: exit 0 but no marker {markers:?} in stdout:\n{stdout}\n--- stderr ---\n{stderr}"
                    ));
                }
            }
            Err(e) => failures.push(format!("{name}: cannot run {}: {e}", path.display())),
        }
    }
    assert!(
        failures.is_empty(),
        "{} example(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
