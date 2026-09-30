//! Negative enforcement fixtures: prove every shape/line gate FAILS.
//!
//! Each test materializes a miniature repo (or crate) violating exactly one
//! gate, runs the real check against it — the repo's own `.alint.yml` via
//! `alint`, or `cargo clippy` for the function-length gate Clippy owns — and
//! asserts the check fails naming the violated rule. Asserting the rule id
//! (not just a nonzero exit) keeps the tests honest: an unrelated failure
//! cannot stand in for the gate under test.
//!
//! Fixture payloads live beside this harness with inert `.txt` extensions so
//! the repo's own gates never trip over them; line-count payloads are
//! generated programmatically for exact counts. `alint` must be on `PATH`
//! (CI provisions it for the `xtask` unit; developers get it via `mise`).
//!
//! Style note: the workspace denies `expect_used` outside `#[test]` bodies,
//! so helpers return `Result` and only the tests themselves call `expect`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Rust sources that must live under `crates/`; placed at the miniature root.
const STRAY_RS: &str = include_str!("stray.rs.txt");
/// First-party executable code is Rust-only; any `.py` file must fail.
const FORBIDDEN_PY: &str = include_str!("forbidden.py.txt");
/// Member manifest inheriting everything except `[lints]`.
const MEMBER_NO_INHERIT: &str = include_str!("member_no_inherit.toml.txt");

/// Fresh miniature-repo root for `case`, namespaced by process id.
fn fixture_root(case: &str) -> Result<PathBuf, String> {
    let pid = std::process::id();
    let root = std::env::temp_dir().join(format!("tuiscotti-enforcement-{case}-{pid}"));
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|error| format!("clean stale fixture: {error}"))?;
    }
    fs::create_dir_all(&root).map_err(|error| format!("create fixture root: {error}"))?;
    Ok(root)
}

/// Write `contents` to `path`, creating parent directories.
fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("create parents: {error}"))?;
    }
    fs::write(path, contents).map_err(|error| format!("write fixture: {error}"))?;
    Ok(())
}

/// The repo's own shipped `.alint.yml`: fixtures prove the real config.
fn repo_alint_config() -> Result<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.alint.yml");
    fs::read_to_string(&path).map_err(|error| format!("read repo .alint.yml: {error}"))
}

/// Minimal repo carrying the shipped config; the caller adds the violation.
fn alint_repo(case: &str) -> Result<PathBuf, String> {
    let root = fixture_root(case)?;
    let config = repo_alint_config()?;
    write_file(&root.join(".alint.yml"), &config)?;
    Ok(root)
}

/// `count` physical padding lines (comments count: the gate is physical).
fn padded_lines(count: usize) -> String {
    "// enforcement padding\n".repeat(count)
}

/// Function with exactly `count` Clippy-counted statement lines.
/// (Calibrated: signature and closing brace are not counted.)
fn counted_fn(count: usize) -> String {
    format!("fn target() {{\n{}}}\n", "    let _ = 0;\n".repeat(count))
}

/// Run the shipped-config gate against the miniature repo at `root`.
///
/// Stays in the inherited package directory so tool shims resolve their
/// versions from the repo; the miniature root travels as explicit args.
/// Falls back to mise provisioning when `alint` is not on PATH (CI crate
/// jobs install only rust+nextest before tests; the `alint` custom task
/// runs after). The mise.toml pin stays the single version source.
fn run_alint(root: &Path) -> Result<Output, String> {
    let mut command = alint_command()?;
    command
        .arg("check")
        .arg("--config")
        .arg(root.join(".alint.yml"))
        .arg(root)
        .output()
        .map_err(|error| format!("spawn alint: {error}"))
}

/// `alint` from PATH, else provisioned once through the repo mise pin.
fn alint_command() -> Result<Command, String> {
    if command_exists("alint") {
        return Ok(Command::new("alint"));
    }
    // Nested mise must see the repo config: CI test steps export the
    // --no-config isolation as env (MISE_NO_CONFIG=1 etc.), which nested
    // mise would inherit. Strip it (backend-qualified install keeps the
    // version single-sourced in mise.toml; only the backend id is named).
    let unisolate = |command: &mut Command| {
        command
            .env_remove("MISE_NO_CONFIG")
            .env_remove("MISE_NO_ENV")
            .env_remove("MISE_NO_HOOKS")
            .env_remove("MISE_LOCKFILE")
            .env_remove("MISE_AUTO_INSTALL")
            .env_remove("MISE_EXEC_AUTO_INSTALL");
    };
    let mut install_cmd = Command::new("mise");
    unisolate(&mut install_cmd);
    let install = install_cmd
        .args(["install", "github:asamarts/alint"])
        .output()
        .map_err(|error| format!("spawn mise install alint: {error}"))?;
    if !install.status.success() {
        return Err(format!(
            "mise install alint failed:\n{}",
            String::from_utf8_lossy(&install.stderr)
        ));
    }
    let mut which_cmd = Command::new("mise");
    unisolate(&mut which_cmd);
    let which = which_cmd
        .args(["which", "alint"])
        .output()
        .map_err(|error| format!("spawn mise which alint: {error}"))?;
    if !which.status.success() {
        return Err(format!(
            "mise which alint failed:\n{}",
            String::from_utf8_lossy(&which.stderr)
        ));
    }
    let path = String::from_utf8_lossy(&which.stdout);
    let path = path.lines().next().unwrap_or("").trim();
    if path.is_empty() {
        return Err("mise which alint printed no path".to_owned());
    }
    Ok(Command::new(path))
}

/// Probe PATH for `tool` without depending on a `which` crate.
fn command_exists(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Run `cargo clippy` in `dir`; cargo resolves through the build.
fn run_clippy(dir: &Path) -> Result<Output, String> {
    Command::new(env!("CARGO"))
        .args([
            "clippy",
            "--offline",
            "--message-format",
            "json",
            "--",
            "-D",
            "clippy::too_many_lines",
        ])
        .current_dir(dir)
        .output()
        .map_err(|error| format!("spawn cargo clippy: {error}"))
}

/// Combined stdout/stderr for rule-id assertions.
fn combined(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{stdout}\n{stderr}")
}

/// The gate must fail, and the failure must name `rule`.
fn assert_gate_fails(case: &str, output: &Output, rule: &str) {
    let log = combined(output);
    assert!(!output.status.success(), "{case}: gate passed, log:\n{log}");
    assert!(
        log.contains(rule),
        "{case}: failure does not name {rule}, log:\n{log}"
    );
}

/// Standalone crate with a `count`-line function and an 80-line Clippy gate.
fn clippy_crate(case: &str, name: &str, count: usize) -> Result<PathBuf, String> {
    let root = fixture_root(case)?;
    let manifest =
        format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n");
    write_file(&root.join("Cargo.toml"), &manifest)?;
    write_file(&root.join("clippy.toml"), "too-many-lines-threshold = 80\n")?;
    write_file(&root.join("src/lib.rs"), &counted_fn(count))?;
    Ok(root)
}

#[test]
fn fn_81_counted_lines_fails_clippy() {
    let root = clippy_crate("fn-81", "neg81", 81).expect("materialize fn-81");
    let output = run_clippy(&root).expect("run clippy on fn-81");
    assert_gate_fails("fn-81", &output, "too_many_lines");
}

#[test]
fn fn_80_counted_lines_passes_clippy() {
    let root = clippy_crate("fn-80", "neg80", 80).expect("materialize fn-80");
    let output = run_clippy(&root).expect("run clippy on fn-80");
    let log = combined(&output);
    assert!(output.status.success(), "fn-80 control failed:\n{log}");
}

#[test]
fn file_401_lines_fails_alint() {
    let root = alint_repo("file-401").expect("materialize file-401");
    write_file(&root.join("crates/foo/src/big.rs"), &padded_lines(401)).expect("write big.rs");
    let output = run_alint(&root).expect("run alint");
    assert_gate_fails("file-401", &output, "rust-max-lines");
}

#[test]
fn lib_151_lines_fails_alint() {
    let root = alint_repo("lib-151").expect("materialize lib-151");
    write_file(&root.join("crates/foo/src/lib.rs"), &padded_lines(151)).expect("write lib.rs");
    let output = run_alint(&root).expect("run alint");
    assert_gate_fails("lib-151", &output, "lib-main-max-lines");
}

#[test]
fn missing_required_file_fails_alint() {
    // Eight of nine required files present: exactly-one-missing must fail.
    // (A single ANY-of rule would pass here; the per-file rules must not.)
    let root = alint_repo("missing-file").expect("materialize missing-file");
    for name in [
        "Cargo.lock",
        "clippy.toml",
        "deny.toml",
        "rustfmt.toml",
        "CODEOWNERS",
        "AGENTS.md",
    ] {
        write_file(&root.join(name), "").expect("write required file");
    }
    write_file(&root.join(".config/nextest.toml"), "").expect("write nextest.toml");
    let output = run_alint(&root).expect("run alint");
    assert_gate_fails("missing-file", &output, "required-file-cargo-toml");
}

#[test]
fn rs_outside_crates_fails_alint() {
    let root = alint_repo("stray-rs").expect("materialize stray-rs");
    write_file(&root.join("stray.rs"), STRAY_RS).expect("write stray.rs");
    let output = run_alint(&root).expect("run alint");
    assert_gate_fails("stray-rs", &output, "crates-only");
}

#[test]
fn forbidden_py_fails_alint() {
    let root = alint_repo("forbidden-py").expect("materialize forbidden-py");
    write_file(&root.join("tool.py"), FORBIDDEN_PY).expect("write tool.py");
    let output = run_alint(&root).expect("run alint");
    assert_gate_fails("forbidden-py", &output, "no-python-javascript-typescript");
}

#[test]
fn missing_lint_inheritance_fails_alint() {
    let root = alint_repo("no-inherit").expect("materialize no-inherit");
    write_file(&root.join("crates/foo/Cargo.toml"), MEMBER_NO_INHERIT)
        .expect("write member manifest");
    let output = run_alint(&root).expect("run alint");
    assert_gate_fails("no-inherit", &output, "member-inherit-lints");
    let log = combined(&output);
    assert!(
        !log.contains("member-inherit-edition"),
        "no-inherit: inherited edition must pass:\n{log}"
    );
}
