//! Shell/state/replay/guardian tests (backlog R09, R12, R13, R14): real
//! PTY, real processes, bounded timeouts.

#![cfg(feature = "pty")]

use std::time::{Duration, Instant};

use tuisnap::frame::Rgb;
use tuisnap::screen::{Maybe, Screen};
use tuisnap::tui::{process_exists, CancelToken, Tui};
use tuisnap::tui_shell::{
    assert_bells_eq, assert_clipboard_empty, assert_clipboard_latest_eq, assert_default_colors,
    assert_hyperlink_present, assert_mode_set, assert_mode_unset, assert_palette_entry,
    assert_scrollback_contains, assert_title_eq, replay_bytes, replay_chunks, replay_recording,
    Containment, Guardian, GuardianReport, Markers, Recording, ReplayError, Shell, ShellError,
    TermSnapshot, MAX_REPLAY_BYTES,
};

fn deadline(secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(secs)
}

fn cancel() -> CancelToken {
    CancelToken::new()
}

fn rows(screen: &Screen) -> Vec<String> {
    let mut out = Vec::with_capacity(screen.rows() as usize);
    for y in 0..screen.rows() {
        let mut s = String::new();
        for x in 0..screen.cols() {
            let c = screen
                .get(x, y)
                .unwrap_or_else(|| panic!("missing cell {x},{y}"));
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

/// Pids whose full command line contains `token`.
fn pgrep(token: &str) -> Vec<u32> {
    let out = std::process::Command::new("pgrep")
        .args(["-f", token])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .filter_map(|p| p.parse::<u32>().ok())
        .collect()
}

fn pkill(token: &str) {
    let _ = std::process::Command::new("pkill")
        .args(["-f", token])
        .output();
}

fn wait_gone(token: &str, secs: u64) {
    let dl = deadline(secs);
    while Instant::now() < dl {
        if pgrep(token).is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "processes matching {token:?} still alive: {:?}",
        pgrep(token)
    );
}

fn wait_found(token: &str, secs: u64) -> Vec<u32> {
    let dl = deadline(secs);
    loop {
        let pids = pgrep(token);
        if !pids.is_empty() || Instant::now() >= dl {
            return pids;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ---------------------------------------------------------------------------
// R12: shell sessions + command markers
// ---------------------------------------------------------------------------

#[test]
fn shell_run_echo() {
    let shell = Shell::sh().unwrap();
    assert_eq!(shell.markers(), Markers::Available);
    let r = shell.run("echo hello-42", deadline(10)).unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.markers, Markers::Available);
    assert!(!r.truncated);
    assert!(r.output_span.iter().any(|l| l.contains("hello-42")));
    shell.into_session().close().unwrap();
}

#[test]
fn shell_cmd_exit_is_not_child_exit() {
    let shell = Shell::sh().unwrap();
    let r = shell.run("(exit 3)", deadline(10)).unwrap();
    assert_eq!(r.exit_code, 3);
    // The shell (direct child) is still alive: exit 3 was the command's.
    assert!(shell.session().poll_exit().is_none());
    let r = shell.run("false", deadline(10)).unwrap();
    assert_eq!(r.exit_code, 1);
    let r = shell.run("printf 'a\\nb\\nc\\n'", deadline(10)).unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.output_span, vec!["a", "b", "c"]);
    assert!(!r.truncated);
    shell.into_session().close().unwrap();
}

#[test]
fn shell_rejects_bad_commands() {
    let shell = Shell::sh().unwrap();
    assert!(matches!(
        shell.run("", deadline(5)),
        Err(ShellError::BadCommand(_))
    ));
    assert!(matches!(
        shell.run("echo a\necho b", deadline(5)),
        Err(ShellError::BadCommand(_))
    ));
    shell.into_session().close().unwrap();
}

#[test]
fn shell_unavailable_refuses_to_guess() {
    let session = Tui::new(["/bin/cat"]).size(40, 10).spawn().unwrap();
    let mut shell = Shell::wrap(session);
    assert_eq!(shell.markers(), Markers::Unavailable);
    // No integration: run refuses instead of inferring spans from echo text.
    assert!(matches!(
        shell.run("echo hi", deadline(5)),
        Err(ShellError::NoIntegration(_))
    ));
    // Handshake against cat echoes but never confirms: bounded failure.
    assert!(shell.setup(deadline(2)).is_err());
    assert_eq!(shell.markers(), Markers::Unavailable);
    shell.into_session().close().unwrap();
}

#[test]
fn shell_truncation_flag() {
    let shell = Shell::sh_sized(80, 6).unwrap();
    let r = shell
        .run(
            "awk 'BEGIN{for(i=1;i<=30;i++)print \"line\"i}'",
            deadline(10),
        )
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert!(r.truncated, "start marker scrolled off: {r:?}");
    assert!(r.output_span.len() <= 6, "span: {:?}", r.output_span);
    assert_eq!(r.output_span.last().map(String::as_str), Some("line30"));
    shell.into_session().close().unwrap();
}

#[test]
fn shell_final_state_preserved_after_exit() {
    let shell = Shell::sh().unwrap();
    // `exit` kills the shell before the end attestation: the run times out.
    assert!(shell.run("exit 0", deadline(3)).is_err());
    let waited = shell.wait_shell_exit(deadline(10), &cancel()).unwrap();
    assert!(waited.status.success());
    // Final grid + state survive the child.
    assert!(contains(&waited.observation.screen, "__TUISNAP_C__"));
    shell.into_session().close().unwrap();
}

// ---------------------------------------------------------------------------
// R13: terminal-state assertions
// ---------------------------------------------------------------------------

#[test]
fn live_title_bells_modes_palette() {
    let mut s = Tui::new(["/bin/cat"]).size(60, 12).spawn().unwrap();
    // Trailing newline: cat only echoes full lines.
    s.send_text("\x1b]2;HelloTitle\x07\n").unwrap();
    let obs = s
        .wait_predicate(
            |o| o.state.title == Maybe::Known("HelloTitle".to_string()),
            deadline(10),
            &cancel(),
        )
        .unwrap();
    let snap = TermSnapshot::from_observation(&obs);
    assert_title_eq(&snap, "HelloTitle").unwrap();
    assert!(assert_title_eq(&snap, "Nope").is_err());

    s.send_text("\x07\n").unwrap();
    let obs = s
        .wait_predicate(
            |o| matches!(o.state.bells, Maybe::Known(n) if n == 1),
            deadline(10),
            &cancel(),
        )
        .unwrap();
    let snap = TermSnapshot::from_observation(&obs);
    assert_bells_eq(&snap, 1).unwrap();
    assert!(assert_bells_eq(&snap, 2).is_err());

    assert_mode_unset(&snap, 1049).unwrap();
    assert!(assert_mode_set(&snap, 1049).is_err());

    assert_palette_entry(&snap, 1, Rgb::from_indexed(1)).unwrap();
    s.send_text("\x1b]4;1;rgb:ff/00/00\x1b\\\n").unwrap();
    let obs = s
        .wait_predicate(
            |o| match &o.state.palette {
                Maybe::Known(p) => p
                    .iter()
                    .any(|(i, c)| *i == 1 && *c == Rgb { r: 255, g: 0, b: 0 }),
                _ => false,
            },
            deadline(10),
            &cancel(),
        )
        .unwrap();
    let snap = TermSnapshot::from_observation(&obs);
    assert_palette_entry(&snap, 1, Rgb { r: 255, g: 0, b: 0 }).unwrap();
    s.close().unwrap();
}

#[test]
fn live_unsupported_state_reported_not_fabricated() {
    let mut s = Tui::new(["/bin/cat"]).size(40, 10).spawn().unwrap();
    let obs = s.observe_now().unwrap();
    let snap = TermSnapshot::from_observation(&obs);
    assert_eq!(snap.defaults, Maybe::Unsupported);
    assert_eq!(snap.clipboard, Maybe::Unsupported);
    assert_eq!(snap.hyperlinks, Maybe::Unsupported);
    assert_eq!(snap.scrollback, Maybe::Unsupported);
    for err in [
        assert_hyperlink_present(&snap, "https://example.test/"),
        assert_scrollback_contains(&snap, "x"),
        assert_default_colors(&snap, None, None),
        assert_clipboard_latest_eq(&snap, "x"),
    ] {
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("unsupported"), "message: {msg}");
    }
    s.close().unwrap();
}

#[test]
fn replay_full_state_asserts() {
    let mut bytes: Vec<u8> = b"\x1b]2;ReTitle\x07\x07\x1b]4;2;rgb:00/ff/00\x1b\\".to_vec();
    bytes.extend_from_slice(b"\x1b]10;rgb:ff/00/00\x1b\\\x1b]11;rgb:00/00/ff\x1b\\");
    bytes.extend_from_slice(b"\x1b]52;c;aGVsbG8=\x1b\\");
    bytes.extend_from_slice(b"\x1b]8;;https://example.test/x\x1b\\LINK\x1b]8;;\x1b\\\r\n");
    for i in 1..=15 {
        bytes.extend_from_slice(format!("line{i:02}\r\n").as_bytes());
    }
    let r = replay_bytes(&bytes, 40, 10).unwrap();
    assert_eq!(r.bytes_fed, bytes.len());
    assert_title_eq(&r.state, "ReTitle").unwrap();
    assert!(assert_title_eq(&r.state, "Nope").is_err());
    assert_bells_eq(&r.state, 1).unwrap();
    assert_palette_entry(&r.state, 2, Rgb { r: 0, g: 255, b: 0 }).unwrap();
    assert_default_colors(
        &r.state,
        Some(Rgb { r: 255, g: 0, b: 0 }),
        Some(Rgb { r: 0, g: 0, b: 255 }),
    )
    .unwrap();
    assert_clipboard_latest_eq(&r.state, "hello").unwrap();
    assert!(assert_clipboard_empty(&r.state).is_err());
    assert_hyperlink_present(&r.state, "https://example.test/x").unwrap();
    assert!(assert_hyperlink_present(&r.state, "https://other.test/").is_err());
    assert_scrollback_contains(&r.state, "line01").unwrap();
    assert!(assert_scrollback_contains(&r.state, "missing-needle").is_err());
    assert_mode_unset(&r.state, 1049).unwrap();
}

#[test]
fn replay_empty_state_known_not_guessed() {
    let r = replay_bytes(b"hi", 20, 5).unwrap();
    assert_default_colors(&r.state, None, None).unwrap();
    assert_clipboard_empty(&r.state).unwrap();
    let err = assert_title_eq(&r.state, "x").unwrap_err().to_string();
    assert!(err.contains("unknown"), "message: {err}");
    let err = assert_hyperlink_present(&r.state, "x")
        .unwrap_err()
        .to_string();
    assert!(err.contains("not present"), "message: {err}");
}

// ---------------------------------------------------------------------------
// R14: bounded raw replay
// ---------------------------------------------------------------------------

/// Bytes exercising mid-UTF-8 (€ = 3 bytes) and mid-escape splits.
fn tricky_bytes() -> Vec<u8> {
    "AB\x1b[1mCD\x1b[0m \u{20ac} \u{20ac}\x1b[31mR\x1b[m\r\nZ"
        .as_bytes()
        .to_vec()
}

#[test]
fn replay_chunk_invariance_all_split_points() {
    let bytes = tricky_bytes();
    let whole = replay_bytes(&bytes, 30, 8).unwrap();
    assert_eq!(whole.chunks, 1);
    // Every 2-way split, including mid-UTF-8 and mid-escape.
    for i in 1..bytes.len() {
        let split = replay_chunks([&bytes[..i], &bytes[i..]], 30, 8).unwrap();
        assert_eq!(split.chunks, 2);
        assert_eq!(split.bytes_fed, bytes.len());
        assert_eq!(split.screen, whole.screen, "split at byte {i}");
        assert_eq!(split.state, whole.state, "split at byte {i}");
    }
    // One-byte chunks: maximal fragmentation.
    let ones: Vec<&[u8]> = bytes.chunks(1).collect();
    let frag = replay_chunks(ones, 30, 8).unwrap();
    assert_eq!(frag.chunks, bytes.len());
    assert_eq!(frag.screen, whole.screen);
    assert_eq!(frag.state, whole.state);
}

/// Capture real PTY output bytes (own minimal reader, bounded).
fn capture_raw(argv0: &str, args: &[&str]) -> Vec<u8> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::Read;
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(argv0);
    for a in args {
        cmd.arg(a);
    }
    cmd.env("ENV", "/dev/null");
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    // Drop our slave handle before reading: a parent-held slave fd
    // suppresses master EOF/EIO on Linux, blocking the reader forever
    // after child exit (macOS returns regardless; Linux hung CI here).
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    // Drain on a thread: a blocking PTY read cannot be preempted, so the
    // deadline lives on this thread, never behind a read that may not return.
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        let _ = tx.send(out);
    });
    let dl = deadline(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        // Kill before failing: a timed-out capture must not leak the child.
        if Instant::now() >= dl {
            let _ = child.kill();
            let _ = child.wait();
            panic!("raw capture timed out waiting for child exit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Trailing bytes after exit, still bounded; kill on overrun so a
    // daemonized grandchild holding the slave cannot hang the suite.
    match rx.recv_timeout(dl.saturating_duration_since(Instant::now())) {
        Ok(out) => {
            let _ = child.wait();
            out
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("raw capture timed out draining PTY output");
        }
    }
}

#[test]
fn replay_recorded_pty_bytes_chunk_invariant() {
    let bytes = capture_raw(
        "/bin/sh",
        // POSIX octal: dash (Linux /bin/sh) does not interpret \xNN.
        &["-c", "printf 'X\\033[1mB\\033[0m\\n\\342\\202\\254\\n'"],
    );
    assert!(!bytes.is_empty());
    assert!(bytes.windows(3).any(|w| w == b"\xe2\x82\xac"));
    let whole = replay_bytes(&bytes, 80, 24).unwrap();
    for i in 1..bytes.len() {
        let split = replay_chunks([&bytes[..i], &bytes[i..]], 80, 24).unwrap();
        assert_eq!(split.screen, whole.screen, "split at byte {i}");
    }
    let ones: Vec<&[u8]> = bytes.chunks(1).collect();
    assert_eq!(replay_chunks(ones, 80, 24).unwrap().screen, whole.screen);
}

#[test]
fn capture_raw_bounded_when_child_exits_silently() {
    // Regression for the Linux-CI hang (run 36482350762): capture_raw once
    // blocked forever in read() because the parent held the slave fd open,
    // suppressing master EOF/EIO. A silent immediate exit is the worst case
    // (no bytes, pure EOF dependence); run it off-thread so a regression
    // fails fast instead of hanging the suite.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(capture_raw("/bin/sh", &["-c", "exit 0"]));
    });
    let bytes = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("capture_raw hung on silent immediate child exit");
    assert!(bytes.is_empty(), "unexpected bytes: {bytes:?}");
}

#[test]
fn replay_input_never_fed_as_output() {
    let mut rec = Recording::new(40, 10);
    rec.push_output(b"KEEP").unwrap();
    // A clear-screen + text: if fed, "KEEP" would vanish.
    rec.push_input(b"\x1b[2J\x1b[HRED").unwrap();
    rec.push_output(b"VISIBLE").unwrap();
    let via_rec = replay_recording(&rec, None).unwrap();
    let direct = replay_bytes(b"KEEPVISIBLE", 40, 10).unwrap();
    assert_eq!(via_rec.screen, direct.screen);
    assert_eq!(via_rec.bytes_fed, b"KEEPVISIBLE".len());
    assert!(contains(&via_rec.screen, "KEEPVISIBLE"));
    // Re-chunked replay agrees too.
    assert_eq!(
        replay_recording(&rec, Some(2)).unwrap().screen,
        direct.screen
    );
}

#[test]
fn replay_bounded() {
    assert!(matches!(
        replay_bytes(b"x", 0, 10),
        Err(ReplayError::InvalidSize(_))
    ));
    let mut rec = Recording::new(10, 10);
    assert!(matches!(
        replay_recording(&rec, Some(0)),
        Err(ReplayError::InvalidChunks(_))
    ));
    let big = vec![0u8; MAX_REPLAY_BYTES + 1];
    assert!(matches!(
        rec.push_output(&big),
        Err(ReplayError::TooLarge { .. })
    ));
    assert!(matches!(
        rec.push_input(&big),
        Err(ReplayError::TooLarge { .. })
    ));
    assert!(matches!(
        replay_bytes(&big, 80, 24),
        Err(ReplayError::TooLarge { .. })
    ));
}

#[test]
fn replay_matches_live_session() {
    let mut s = Tui::new([
        "/bin/sh",
        "-c",
        "stty -echo; printf 'A\\033[31mB\\033[0m\\nEND\\n'",
    ])
    .env("ENV", "/dev/null")
    .size(40, 10)
    .spawn()
    .unwrap();
    let waited = s.wait_exit(deadline(10), &cancel()).unwrap();
    assert!(waited.status.success());
    // PTY ONLCR translates \n to \r\n.
    let expected = b"A\x1b[31mB\x1b[0m\r\nEND\r\n";
    let replayed = replay_bytes(expected, 40, 10).unwrap();
    assert_eq!(replayed.screen, waited.observation.screen);
    s.close().unwrap();
}

// ---------------------------------------------------------------------------
// R09: scoped guardian
// ---------------------------------------------------------------------------

#[test]
fn guardian_contains_group() {
    let session = Tui::new(["/bin/sh", "-c", "trap '' TERM; sleep 29371"])
        .env("ENV", "/dev/null")
        .size(40, 10)
        .spawn()
        .unwrap();
    let child_pid = session.pid().unwrap();
    assert!(!wait_found("29371", 5).is_empty());
    let guardian = Guardian::wrap(session);
    let report: GuardianReport = guardian.finish(deadline(10)).unwrap();
    assert_eq!(report.child_pid, Some(child_pid));
    assert_eq!(report.containment, Containment::Full);
    assert!(report.teardown_error.is_none());
    assert!(pgrep("29371").is_empty());
    assert!(!process_exists(child_pid));
}

#[test]
fn guardian_escape_boundary_setsid_outlives() {
    let script = "python3 -c 'import os; os.setsid(); os.execvp(\"sleep\",[\"sleep\",\"29372\"])' & exec sleep 29373";
    let session = Tui::new(["/bin/sh", "-c", script])
        .env("ENV", "/dev/null")
        .size(40, 10)
        .spawn()
        .unwrap();
    let escaped = wait_found("29372", 10);
    assert!(!escaped.is_empty(), "escapee never started");
    assert!(!wait_found("29373", 5).is_empty());
    let report = Guardian::wrap(session).finish(deadline(10)).unwrap();
    // Same-group child contained...
    assert!(pgrep("29373").is_empty());
    // ...but the setsid grandchild outlives: the documented boundary.
    let still = pgrep("29372");
    assert_eq!(still, escaped);
    assert!(!GuardianReport::escape_boundary_note().is_empty());
    // Prove the mechanism: the escapee is in a different process group.
    let out = std::process::Command::new("ps")
        .args(["-o", "pgid=", "-p", &still[0].to_string()])
        .output()
        .unwrap();
    let escapee_pgid: i32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    assert_ne!(Some(escapee_pgid), report.pgid);
    // Bounded cleanup of the deliberate escapee.
    pkill("29372");
    wait_gone("29372", 5);
}

#[test]
fn guardian_drop_contains() {
    {
        let session = Tui::new(["/bin/sh", "-c", "trap '' TERM; sleep 29374"])
            .env("ENV", "/dev/null")
            .size(40, 10)
            .spawn()
            .unwrap();
        assert!(!wait_found("29374", 5).is_empty());
        let _guardian = Guardian::wrap(session);
    }
    wait_gone("29374", 5);
}

#[test]
fn guardian_exited_child_degrades_without_killing() {
    // Wrap after the child was reaped: identity unresolvable, kill nothing.
    let session = Tui::new(["/bin/sh", "-c", "exit 0"])
        .env("ENV", "/dev/null")
        .size(40, 10)
        .spawn()
        .unwrap();
    session.wait_exit(deadline(10), &cancel()).unwrap();
    let report = Guardian::wrap(session).finish(deadline(5)).unwrap();
    assert!(report.signalled.is_empty());
    assert!(matches!(
        report.containment,
        Containment::Unknown { .. } | Containment::Full
    ));
}
