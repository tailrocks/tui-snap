//! M2 vertical slice, item 2: piped CLI error against the real binary.
//!
//! The real `tuiscotti` binary run with a bad flag through the piped
//! adapter (`tuiscotti::command::Command::cargo_bin`), asserting the exit
//! code, usage text on stderr, and an insta snapshot of a documented
//! stdout/stderr/exit projection.
//!
//! This test lives in the CLI package (not next to the `settings_view`
//! slice in `tuiscotti-fixtures`) because stable cargo cannot express a
//! cross-package binary dependency (`artifact = "bin"` is nightly-only):
//! the per-package CI unit never builds another package's binary, so the
//! test must run where `CARGO_BIN_EXE_tuiscotti` is set.
//!
//! `INSTA_UPDATE` stays ambient (read-only): Insta exposes no `Settings`
//! switch for the update behavior, and `set_var` is an `unsafe fn` in edition
//! 2024 that cannot be used under the workspace lints. Committed snapshots
//! match, so green-path assertions hold under every mode; run with
//! `INSTA_UPDATE=no` for fail-clean (never auto-bless) or
//! `INSTA_UPDATE=always` to regenerate approvals.
//!
//! Runs inside a [`tuiscotti::runner::TestContext`] (attempt-qualified
//! scratch isolation, child-only env) and finishes with a journal completion
//! marker; completion is asserted, not assumed.

use tuiscotti::command::{Command, Termination};
use tuiscotti::runner::{Journal, JournalStatus, TestContext};

/// Documented projection of a piped run for snapshot review: exit code (or
/// non-exit termination), per-stream byte lengths, then lossy stream bodies.
/// No environment, path, or timing data enters the projection, so it is stable
/// across machines and runners (the binary under test prints no paths for a
/// flag-parse error).
fn cli_projection(argv: &[&str], out: &tuiscotti::command::ProcessOutput) -> String {
    let exit_code = match out.code() {
        Some(code) => code.to_string(),
        None => "none".to_string(),
    };
    format!(
        "argv: tuiscotti {}\ntermination: {:?}\nexit_code: {}\ntruncated: {}\n\
         --- stdout ({} bytes) ---\n{}\n--- stderr ({} bytes) ---\n{}",
        argv.join(" "),
        out.status,
        exit_code,
        out.truncated,
        out.stdout.len(),
        out.stdout_lossy(),
        out.stderr.len(),
        out.stderr_lossy(),
    )
}

#[test]
fn cli_error() {
    let ctx = TestContext::current("cli-error").expect("test context");
    let mut journal = Journal::open(&ctx.journal_path()).expect("open journal");
    journal.append("start", "cli-error").expect("journal start");

    let argv = ["--bad-flag"];
    let out = Command::cargo_bin("tuiscotti").arg(argv[0]).run();
    journal
        .append("ran", &format!("status={:?}", out.status))
        .expect("journal");

    // Clap parse errors exit 2; the child is reaped (Exit, never a kill or
    // spawn failure) with complete output.
    assert_eq!(
        out.status,
        Termination::Exit(2),
        "bad flag must exit 2, got {:?} (error: {:?})",
        out.status,
        out.error
    );
    assert!(!out.truncated, "error output must be complete");
    assert!(out.stdout.is_empty(), "no stdout on parse error");
    let stderr = out.stderr_lossy();
    assert!(
        stderr.contains("Usage:"),
        "stderr carries usage text:\n{stderr}"
    );
    assert!(
        stderr.contains("--bad-flag"),
        "stderr names the offending flag:\n{stderr}"
    );

    // Piped run is fully reaped inside `run` (Exit status observed, pipes
    // drained): no leaked children by construction, and the runner touched no
    // global state (child env was never applied to this process).
    insta::assert_snapshot!("cli_error", cli_projection(&argv, &out));

    journal.complete("pass").expect("journal complete");
    match Journal::status(ctx.scratch_dir()) {
        JournalStatus::Complete { status } => assert_eq!(status, "pass"),
        JournalStatus::Incomplete { reason } => panic!("journal incomplete: {reason}"),
    }
}
