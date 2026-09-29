//! Piped-process Command tests (backlog R01–R03).
//!
//! Portable macOS/Linux via `/bin/sh` + `printf`; no network, std only.

use std::path::PathBuf;
use std::time::{Duration, Instant};
use tuiscotti::command::{Command, Termination, cargo_bin_path, isolated_env};

#[test]
fn separate_stdout_stderr_bytes() {
    let out = Command::new("/bin/sh")
        .args(["-c", "printf 'out\\n'; printf 'err\\n' >&2"])
        .run();
    assert_eq!(out.status, Termination::Exit(0));
    assert!(out.success());
    assert_eq!(out.stdout, b"out\n");
    assert_eq!(out.stderr, b"err\n");
    assert!(!out.truncated);
}

#[test]
fn non_utf8_bytes_preserved() {
    let out = Command::new("/bin/sh")
        // POSIX octal: dash (Linux /bin/sh) does not interpret \xNN.
        .args(["-c", "printf '\\377\\376\\000A'; printf '\\200\\201' >&2"])
        .run();
    assert_eq!(out.status, Termination::Exit(0));
    assert_eq!(out.stdout, vec![0xff, 0xfe, 0x00, b'A']);
    assert_eq!(out.stderr, vec![0x80, 0x81]);
    // Lossy views must not corrupt the raw bytes.
    assert_eq!(out.stdout.len(), 4);
}

#[test]
fn stdin_write_then_eof() {
    let input = b"hello\n\xff\x00world\n".to_vec();
    let out = Command::new("cat").stdin(input.clone()).run();
    assert_eq!(out.status, Termination::Exit(0));
    assert_eq!(out.stdout, input);
    assert!(out.stderr.is_empty());
}

#[test]
fn stdin_defaults_to_immediate_eof() {
    let out = Command::new("cat").timeout(Duration::from_secs(10)).run();
    assert_eq!(out.status, Termination::Exit(0));
    assert!(out.stdout.is_empty());
}

#[test]
fn large_output_both_pipes_no_deadlock() {
    // 1.5MB on each pipe concurrently; sequential drain would deadlock.
    let script = "yes OUT 2>/dev/null | head -c 1500000 >&2 & \
                  yes ERR 2>/dev/null | head -c 1500000; wait";
    let out = Command::new("/bin/sh")
        .args(["-c", script])
        .timeout(Duration::from_secs(60))
        .run();
    assert_eq!(out.status, Termination::Exit(0));
    assert!(!out.truncated);
    assert_eq!(out.stdout.len(), 1_500_000, "stdout short");
    assert_eq!(out.stderr.len(), 1_500_000, "stderr short");
}

#[test]
fn nonzero_exit_is_honest() {
    let out = Command::new("/bin/sh").args(["-c", "exit 3"]).run();
    assert_eq!(out.status, Termination::Exit(3));
    assert_eq!(out.code(), Some(3));
    assert!(!out.success());
}

#[test]
#[cfg(unix)]
fn signal_death_is_distinct_from_exit() {
    let out = Command::new("/bin/sh").args(["-c", "kill -TERM $$"]).run();
    assert_eq!(out.status, Termination::Signal(15)); // SIGTERM
    assert_eq!(out.signal(), Some(15));
    assert!(!out.success());
}

#[test]
fn timeout_kills_and_reports_timeout() {
    let start = Instant::now();
    let out = Command::new("sleep")
        .arg("30")
        .timeout(Duration::from_millis(300))
        .run();
    assert_eq!(out.status, Termination::Timeout);
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn output_limit_truncates_with_truthful_flag() {
    let out = Command::new("/bin/sh")
        .args(["-c", "yes OVERFLOW 2>/dev/null | head -c 1000000"])
        .output_limit(4096)
        .timeout(Duration::from_secs(30))
        .run();
    assert_eq!(out.status, Termination::OutputLimit);
    assert!(out.truncated);
    assert!(out.stdout.len() <= 4096);
}

#[test]
fn late_output_bounded_by_drain_deadline() {
    // Grandchild inherits the pipes and holds them 10s; the direct child
    // exits 0 at once. Drain must give up fast, not hang.
    let start = Instant::now();
    let out = Command::new("/bin/sh")
        .args(["-c", "sleep 10 &"])
        .drain_deadline(Duration::from_millis(300))
        .timeout(Duration::from_secs(30))
        .run();
    assert_eq!(out.status, Termination::Exit(0));
    assert!(out.truncated, "held-open pipes must report truncation");
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn no_shell_by_default() {
    // A shell would run this; direct spawn must fail (space in argv0).
    let out = Command::new("echo hello").run();
    assert_eq!(out.status, Termination::SpawnError);
    assert!(out.error.is_some());
}

#[test]
fn shell_opt_in_passes_positional_params() {
    let out = Command::new("printf '<%s>' \"$1\"")
        .shell(true)
        .arg("hi")
        .run();
    assert_eq!(out.status, Termination::Exit(0));
    assert_eq!(out.stdout, b"<hi>");
}

#[test]
fn spawn_error_reports_detail() {
    let out = Command::new("/nonexistent-binary-xyz").run();
    assert_eq!(out.status, Termination::SpawnError);
    assert!(out.error.as_ref().is_some_and(|e| !e.detail().is_empty()));
    assert!(!out.truncated);
}

#[test]
fn cargo_bin_resolves_tuisnap_itself() {
    let path = cargo_bin_path("tuisnap").expect("tuisnap binary resolvable");
    assert!(path.is_file(), "not a file: {}", path.display());
    let out = Command::cargo_bin("tuisnap").arg("--version").run();
    assert_eq!(out.status, Termination::Exit(0));
    assert!(out.stdout_lossy().contains("tuisnap"));
}

#[test]
fn cargo_bin_missing_reports_searched_paths() {
    let out = Command::cargo_bin("tuisnap-no-such-bin-xyz").run();
    assert_eq!(out.status, Termination::SpawnError);
    let err = out.error.expect("searched locations reported");
    assert_eq!(
        err.kind(),
        tuiscotti::command::SpawnErrorKind::BinaryNotFound
    );
    assert!(!err.searched().is_empty(), "searched paths: {err}");
    assert!(err.detail().contains("tuisnap-no-such-bin-xyz"), "{err}");
}

#[test]
fn env_is_child_only() {
    // `remove_var` is an `unsafe fn` in edition 2024, so instead of forcing a
    // clean precondition, assert the parent value is unchanged by the spawn
    // (strictly stronger: holds regardless of ambient state).
    let before = std::env::var("TUISNAP_PIPED_PROBE").ok();
    let out = Command::new("/bin/sh")
        .args(["-c", "printf '%s' \"$TUISNAP_PIPED_PROBE\""])
        .env("TUISNAP_PIPED_PROBE", "child-value")
        .run();
    assert_eq!(out.stdout, b"child-value");
    assert_eq!(
        std::env::var("TUISNAP_PIPED_PROBE").ok(),
        before,
        "parent env must be untouched"
    );
}

#[test]
fn isolated_home_is_respected() {
    let parent_home = std::env::var("HOME").unwrap_or_default();
    let iso = isolated_env().expect("fixture");
    assert_ne!(iso.home(), PathBuf::from(&parent_home));
    assert!(iso.home().is_dir());
    assert!(iso.cwd().is_dir());

    let cmd = iso.apply(
        Command::new("/bin/sh").args([
            "-c",
            "printf '%s\\n%s\\n%s\\n%s\\n%s' \"$HOME\" \"$PWD\" \"$XDG_CONFIG_HOME\" \"$XDG_CACHE_HOME\" \"$TMPDIR\"",
        ]),
    );
    let out = cmd.run();
    assert_eq!(out.status, Termination::Exit(0));
    let text = out.stdout_lossy();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], iso.home().to_str().expect("utf8 path"));
    // $PWD is canonical (/private/var/...); temp_dir() may use a symlink.
    let canon_cwd = iso.cwd().canonicalize().expect("cwd exists");
    assert_eq!(lines[1], canon_cwd.to_str().expect("utf8 path"));
    assert_eq!(
        lines[2],
        iso.home()
            .join(".config")
            .to_str()
            .expect("utf8 path")
            .to_string()
    );
    assert_eq!(
        lines[3],
        iso.home()
            .join(".cache")
            .to_str()
            .expect("utf8 path")
            .to_string()
    );
    assert_eq!(lines[4], iso.tmp().to_str().expect("utf8 path"));
    // Parent env untouched by the fixture.
    assert_eq!(std::env::var("HOME").unwrap_or_default(), parent_home);

    // Fixture root removed on drop.
    let root = iso.root().to_path_buf();
    drop(iso);
    assert!(!root.exists(), "temp root must be cleaned up");
}

#[test]
fn isolated_env_scrubs_dylib_path_unless_preserved() {
    // Live child check uses LD_LIBRARY_PATH: macOS SIP strips DYLD_* from
    // /bin/sh children even when preserved, so DYLD_* is asserted at the
    // builder level below instead (portable, no platform bypass).
    let probe = "printf '%s' \"$LD_LIBRARY_PATH\"";
    let out = isolated_env()
        .expect("isolated env fixture")
        .apply(
            Command::new("/bin/sh")
                .args(["-c", probe])
                .env("LD_LIBRARY_PATH", "x"),
        )
        .run();
    assert!(
        out.stdout.is_empty(),
        "LD_LIBRARY_PATH must be scrubbed by default, got {:?}",
        out.stdout_lossy()
    );
    let kept = isolated_env()
        .expect("isolated env fixture")
        .preserve_dylib_path(true)
        .apply(
            Command::new("/bin/sh")
                .args(["-c", probe])
                .env("LD_LIBRARY_PATH", "x"),
        )
        .run();
    assert_eq!(kept.stdout, b"x");

    // Builder-level: default apply() removes all dylib vars; preserve leaves
    // them alone (parent value inherited).
    for var in [
        "LD_LIBRARY_PATH",
        "DYLD_LIBRARY_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
    ] {
        let scrubbed = isolated_env()
            .expect("isolated env fixture")
            .apply(Command::new("x"))
            .std_command();
        let entry: Vec<_> = scrubbed
            .get_envs()
            .filter(|(k, _)| *k == std::ffi::OsStr::new(var))
            .collect();
        assert_eq!(entry.len(), 1, "{var} must be mapped, got {entry:?}");
        assert_eq!(entry[0].1, None, "{var} must be removed by default");

        let preserved = isolated_env()
            .expect("isolated env fixture")
            .preserve_dylib_path(true)
            .apply(Command::new("x"))
            .std_command();
        assert!(
            preserved
                .get_envs()
                .all(|(k, _)| k != std::ffi::OsStr::new(var)),
            "{var} must be untouched when preserved"
        );
    }
}

#[test]
fn std_command_interop_round_trip() {
    let mut std_cmd = std::process::Command::new("/bin/sh");
    std_cmd
        .args(["-c", "printf '%s:%s' \"$FROM_STD\" \"$PWD_SUFFIX\""])
        .env("FROM_STD", "yes");
    let out = Command::from_std(&std_cmd).run();
    assert_eq!(out.status, Termination::Exit(0));
    assert!(out.stdout_lossy().starts_with("yes:"));

    // Borrowed From impl.
    let via_from: Command = (&std_cmd).into();
    let exported = via_from.std_command();
    assert_eq!(exported.get_program(), std::ffi::OsStr::new("/bin/sh"));

    // Accessor preserves spawn configuration.
    let exported = Command::new("prog").arg("a").env("K", "V").std_command();
    let args: Vec<_> = exported.get_args().collect();
    assert_eq!(args, vec![std::ffi::OsStr::new("a")]);
}
