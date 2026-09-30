//! Unit tests: bounded joins, the cargo-bin resolver, the reader's
//! termination-cause separation (LIFE-3), stdin-close mode qualification
//! (LIFE-1), worker-side signals (LIFE-2), exit-observation identity
//! (LIFE-8), and argv/env/cwd wiring (LIFE-9).

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

// LIFE-3: termination-cause separation (no PTY needed).

/// Scripted reads, then EOF.
struct ScriptReader {
    script: std::collections::VecDeque<std::io::Result<Vec<u8>>>,
}

impl std::io::Read for ScriptReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.script.pop_front() {
            Some(Ok(bytes)) => {
                let n = bytes.len().min(buf.len());
                buf[..n].copy_from_slice(&bytes[..n]);
                Ok(n)
            }
            Some(Err(e)) => Err(e),
            None => Ok(0),
        }
    }
}

/// LIFE-3: `Interrupted` retries (never EOF), data feeds, clean EOF ends.
#[test]
fn reader_retry_then_data_then_eof() {
    use super::capture::run_reader;
    use super::worker::Op;
    let interrupted = std::io::Error::new(std::io::ErrorKind::Interrupted, "signal");
    let reader = ScriptReader {
        script: std::collections::VecDeque::from([
            Err(interrupted),
            Ok(b"hi".to_vec()),
            Ok(Vec::new()),
        ]),
    };
    let (tx, rx) = std::sync::mpsc::sync_channel::<Op>(16);
    run_reader(Box::new(reader), &tx);
    drop(tx);
    let ops: Vec<Op> = rx.into_iter().collect();
    assert_eq!(ops.len(), 2, "Interrupted retries, then Feed + Eof");
    assert!(matches!(ops[0], Op::Feed(ref b) if b == b"hi"));
    assert!(matches!(ops[1], Op::Eof));
}

/// LIFE-3: a read error reports `ReadError` with the message — never EOF.
#[test]
fn reader_error_is_not_eof() {
    use super::capture::run_reader;
    use super::worker::Op;
    let reader = ScriptReader {
        // Raw EIO (errno 5 on Linux and macOS): the post-child-death read
        // error the old code conflated with EOF.
        script: std::collections::VecDeque::from([Err(std::io::Error::from_raw_os_error(5))]),
    };
    let (tx, rx) = std::sync::mpsc::sync_channel::<Op>(16);
    run_reader(Box::new(reader), &tx);
    drop(tx);
    let ops: Vec<Op> = rx.into_iter().collect();
    assert_eq!(ops.len(), 1);
    assert!(
        matches!(&ops[0], Op::ReadError(m) if m.contains("Input/output error")),
        "read error keeps its message, distinct from Eof"
    );
}

// PTY-backed unit tests (LIFE-1/2/8/9).
#[cfg(unix)]
mod pty_tests {
    use std::time::{Duration, Instant};

    use tuiscotti_core::screen::Screen;

    use crate::tui::{CancelToken, Tui};

    fn deadline(secs: u64) -> Instant {
        Instant::now() + Duration::from_secs(secs)
    }

    fn rows(screen: &Screen) -> Vec<String> {
        let mut out = Vec::with_capacity(screen.rows() as usize);
        for y in 0..screen.rows() {
            let mut s = String::new();
            for x in 0..screen.cols() {
                let c = screen.get(x, y).expect("cell in grid");
                if !c.continuation {
                    s.push_str(&c.symbol);
                }
            }
            out.push(s.trim_end().to_string());
        }
        out
    }

    fn contains(screen: &Screen, needle: &str) -> bool {
        rows(screen).iter().any(|r| r.contains(needle))
    }

    /// LIFE-1 GAP, canonical mode: closing stdin delivers EOF and `cat`
    /// exits cleanly.
    #[test]
    fn close_input_eofs_canonical_cat() {
        let s = Tui::new(["/bin/cat"]).spawn().expect("spawn cat");
        s.close_input().expect("close stdin");
        let w = s
            .wait_exit(deadline(5), &CancelToken::new())
            .expect("cat exits on EOF");
        assert!(w.status.success(), "status: {}", w.status);
        s.close().expect("close succeeds");
    }

    /// LIFE-1 GAP, raw mode: stdin close arrives as a VEOF byte, which is
    /// data (not EOF) without canonical mode — the child survives, and
    /// teardown still reaps it cleanly.
    #[test]
    fn close_input_raw_cat_is_data_not_eof() {
        use crate::tui::process_exists;
        let s = Tui::new(["/bin/sh", "-c", "stty raw; exec /bin/cat"])
            .spawn()
            .expect("spawn raw cat");
        let pid = s.pid().expect("pid known");
        let r0 = s.revision();
        s.close_input().expect("close stdin");
        s.wait_predicate(|o| o.revision > r0, deadline(5), &CancelToken::new())
            .expect("VEOF bytes round-trip as data");
        assert!(
            s.poll_exit().is_none(),
            "raw-mode child survives stdin close (VEOF is data, not EOF)"
        );
        s.close().expect("close reaps the live child");
        assert!(!process_exists(pid), "child {pid} reaped after close");
    }

    /// LIFE-2: a worker-delivered SIGTERM kills the child and the reaped
    /// signal death is reported honestly.
    #[test]
    fn worker_signal_kills_child() {
        use crate::tui::Signal;
        let s = Tui::new(["/bin/cat"]).spawn().expect("spawn cat");
        s.signal(Signal::Term).expect("signal delivers");
        let w = s
            .wait_exit(deadline(5), &CancelToken::new())
            .expect("child reaped after SIGTERM");
        assert!(!w.status.success());
        assert!(
            w.status.signal().is_some(),
            "signal death recorded, got {}",
            w.status
        );
        s.close().expect("close succeeds");
    }

    /// LIFE-8: `wait_exit` returns the published exit revision itself, and
    /// a later manual observation does not move it.
    #[test]
    fn wait_exit_returns_exit_revision() {
        use tuiscotti_core::screen::CaptureReason;
        let s = Tui::new(["/bin/sh", "-c", "exit 3"])
            .spawn()
            .expect("spawn exit-3");
        let w = s
            .wait_exit(deadline(5), &CancelToken::new())
            .expect("wait_exit succeeds");
        assert_eq!(w.observation.reason, CaptureReason::Exit);
        let _ = s.observe_now().expect("observe after exit works");
        let w2 = s
            .wait_exit(deadline(5), &CancelToken::new())
            .expect("second wait_exit succeeds");
        assert_eq!(
            w2.observation.revision, w.observation.revision,
            "exit revision is stable across later observations"
        );
        assert_eq!(w2.observation.reason, CaptureReason::Exit);
        w2.code(3).expect("code asserts");
        s.close().expect("close succeeds");
    }

    /// LIFE-8: a natural exit with the reader drained records `Eof`.
    #[test]
    fn natural_exit_drain_is_eof() {
        use crate::tui::DrainCause;
        let s = Tui::new(["/bin/sh", "-c", "exit 0"])
            .spawn()
            .expect("spawn exit-0");
        s.wait_exit(deadline(5), &CancelToken::new())
            .expect("wait_exit succeeds");
        let meta = s.meta().expect("meta published");
        assert_eq!(meta.drain, Some(DrainCause::Eof), "meta: {meta:?}");
        s.close().expect("close succeeds");
    }

    /// LIFE-9: `env_clear(true)` starts empty (no inheritance, no implicit
    /// `TERM`) then applies `.env(...)`. `printenv` reads raw values — the
    /// shell would substitute defaults at `$VAR` expansion.
    #[test]
    fn env_clear_starts_empty() {
        let s = Tui::new([
            "/bin/sh",
            "-c",
            "printenv TUISCOTTI_FOO; printenv PATH; printenv HOME; printenv TERM; exit 0",
        ])
        .env_clear(true)
        .env("TUISCOTTI_FOO", "foo")
        .spawn()
        .expect("spawn env probe");
        let w = s
            .wait_exit(deadline(5), &CancelToken::new())
            .expect("probe exits");
        assert!(w.status.success());
        let screen = &w.observation.screen;
        assert!(contains(screen, "foo"), "rows: {:?}", rows(screen));
        assert!(
            !contains(screen, "/"),
            "no inherited PATH/HOME leak paths, rows: {:?}",
            rows(screen)
        );
        assert!(
            !contains(screen, "xterm") && !contains(screen, "dumb"),
            "no TERM at all under env_clear, rows: {:?}",
            rows(screen)
        );
        s.close().expect("close succeeds");
    }

    /// LIFE-9: `env_remove` drops one inherited variable while the rest
    /// of the parent environment passes through untouched.
    #[test]
    fn env_remove_drops_one_var() {
        let path = std::env::var("PATH").unwrap_or_default();
        let home = std::env::var("HOME").unwrap_or_default();
        assert!(
            !path.is_empty() && !home.is_empty(),
            "test precondition: PATH and HOME set in the test environment"
        );
        let s = Tui::new(["/bin/sh", "-c", "printenv PATH; printenv HOME; exit 0"])
            .env_remove("PATH")
            .spawn()
            .expect("spawn env probe");
        let w = s
            .wait_exit(deadline(5), &CancelToken::new())
            .expect("probe exits");
        assert!(w.status.success());
        let screen = &w.observation.screen;
        assert!(
            contains(screen, &home),
            "untouched vars pass through, rows: {:?}",
            rows(screen)
        );
        assert!(
            !contains(screen, &path),
            "PATH removed, rows: {:?}",
            rows(screen)
        );
        s.close().expect("close succeeds");
    }

    /// LIFE-9: removing the inherited `TERM` lets the default fill in —
    /// the default applies exactly when the child would otherwise lack it.
    #[test]
    fn env_remove_term_falls_back_to_default() {
        let s = Tui::new(["/bin/sh", "-c", "printenv TERM; exit 0"])
            .env_remove("TERM")
            .spawn()
            .expect("spawn env probe");
        let w = s
            .wait_exit(deadline(5), &CancelToken::new())
            .expect("probe exits");
        assert!(w.status.success());
        assert!(
            contains(&w.observation.screen, "xterm-256color"),
            "default TERM fills the gap, rows: {:?}",
            rows(&w.observation.screen)
        );
        s.close().expect("close succeeds");
    }

    /// LIFE-9: non-UTF-8 argv passes through byte-exact without mangling or
    /// panicking; the child still spawns and exits.
    #[test]
    fn non_unicode_argv_spawns() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let raw = OsStr::from_bytes(b"\xff\xfe-binary-arg");
        let dbg = format!("{:?}", Tui::new(["/bin/echo"]).env("K", raw));
        assert!(dbg.contains("<redacted>"), "{dbg}");
        let s = Tui::new([OsStr::new("/bin/echo"), raw])
            .spawn()
            .expect("spawn echo with non-UTF-8 argv");
        let w = s
            .wait_exit(deadline(5), &CancelToken::new())
            .expect("echo exits");
        assert!(w.status.success());
        s.close().expect("close succeeds");
    }

    /// LIFE-9: a nonexistent target fails spawn with a typed error and no
    /// session (LIFE-4 failure path with nothing to roll back).
    #[test]
    fn nonexistent_target_fails_spawn() {
        let err = Tui::new(["/nonexistent-tuiscotti-target-xyz"])
            .spawn()
            .expect_err("missing binary must fail spawn");
        let msg = err.to_string();
        assert!(msg.contains("spawn failed"), "{msg}");
    }

    /// LIFE-9: an invalid cwd fails the spawn honestly (no panic, no
    /// orphaned session).
    #[test]
    fn invalid_cwd_fails_spawn() {
        let err = Tui::new(["/bin/cat"])
            .cwd("/nonexistent-tuiscotti-cwd-xyz".into())
            .spawn()
            .expect_err("bad cwd must fail spawn");
        let msg = err.to_string();
        assert!(msg.contains("spawn failed"), "{msg}");
    }
}
