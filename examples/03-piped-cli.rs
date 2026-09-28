//! 03: piped CLI — Command error cases are data, not panics.
//!
//! Run: `cargo run --example 03-piped-cli`
//!
//! Spawn failure and nonzero exit are distinct `Termination` variants with the
//! raw stdout/stderr bytes preserved separately.

use tuisnap::command::{Command, Termination};

fn main() {
    // Missing binary: SpawnError carries the OS detail, no exception thrown.
    let missing = Command::new("/nonexistent-tuisnap-binary-xyz").run();
    assert_eq!(missing.status, Termination::SpawnError);
    assert!(!missing.success());
    assert!(missing.error.as_deref().unwrap_or_default().len() > 5);

    // Failing child: exit code + split streams, byte-exact.
    let failed = Command::new("/bin/sh")
        .args(["-c", "printf 'out-line\\n'; printf 'err-line\\n' >&2; exit 3"])
        .run();
    assert_eq!(failed.status, Termination::Exit(3));
    assert_eq!(failed.code(), Some(3));
    assert_eq!(failed.stdout_lossy(), "out-line\n");
    assert_eq!(failed.stderr_lossy(), "err-line\n");
    assert!(!failed.truncated);

    println!(
        "EXAMPLE-03-OK spawn_error={:?} exit={} stdout={:?}",
        missing.status,
        failed.code().unwrap(),
        failed.stdout_lossy().trim(),
    );
}
