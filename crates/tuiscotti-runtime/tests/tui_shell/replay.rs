//! R14 bounded raw replay (split from `tui_shell.rs`; shared helpers live in the root).

use super::{cancel, contains, deadline};
use std::time::{Duration, Instant};
use tuiscotti_runtime::tui::Tui;
use tuiscotti_runtime::tui_shell::{
    MAX_REPLAY_BYTES, Recording, ReplayError, replay_bytes, replay_chunks, replay_recording,
};

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
    let whole = replay_bytes(&bytes, 30, 8).expect("replay_bytes succeeds");
    assert_eq!(whole.chunks, 1);
    // Every 2-way split, including mid-UTF-8 and mid-escape.
    for i in 1..bytes.len() {
        let split =
            replay_chunks([&bytes[..i], &bytes[i..]], 30, 8).expect("replay_chunks succeeds");
        assert_eq!(split.chunks, 2);
        assert_eq!(split.bytes_fed, bytes.len());
        assert_eq!(split.screen, whole.screen, "split at byte {i}");
        assert_eq!(split.state, whole.state, "split at byte {i}");
    }
    // One-byte chunks: maximal fragmentation.
    let ones: Vec<&[u8]> = bytes.chunks(1).collect();
    let frag = replay_chunks(ones, 30, 8).expect("replay_chunks succeeds");
    assert_eq!(frag.chunks, bytes.len());
    assert_eq!(frag.screen, whole.screen);
    assert_eq!(frag.state, whole.state);
}

/// Capture real PTY output bytes (own minimal reader, bounded).
fn capture_raw(argv0: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let params = termpane::process::SpawnParams::new(argv0)
        .args(args.iter())
        .env("ENV", "/dev/null");
    // One call: open + spawn + parent-slave-drop. Dropping our slave
    // handle before reading matters: a parent-held slave fd suppresses
    // master EOF/EIO on Linux, blocking the reader forever after child
    // exit (macOS returns regardless; Linux hung CI here).
    let (master, mut child) =
        termpane::pty::spawn_pty(&params, 80, 24).map_err(|e| format!("spawn_pty failed: {e}"))?;
    let mut reader = master
        .try_clone_reader()
        .map_err(|e| format!("try_clone_reader failed: {e}"))?;
    // Drain on a thread: a blocking PTY read cannot be preempted, so the
    // deadline lives on this thread, never behind a read that may not return.
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
            }
        }
        tx.send(out).ok();
    });
    let dl = deadline(10);
    loop {
        if child
            .try_wait()
            .map_err(|e| format!("try_wait failed: {e}"))?
            .is_some()
        {
            break;
        }
        // Kill before failing: a timed-out capture must not leak the child.
        if Instant::now() >= dl {
            child.kill().ok();
            child.wait().ok();
            return Err("raw capture timed out waiting for child exit".to_string());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Trailing bytes after exit, still bounded; kill on overrun so a
    // daemonized grandchild holding the slave cannot hang the suite.
    if let Ok(out) = rx.recv_timeout(dl.saturating_duration_since(Instant::now())) {
        child.wait().ok();
        Ok(out)
    } else {
        child.kill().ok();
        child.wait().ok();
        Err("raw capture timed out draining PTY output".to_string())
    }
}

#[test]
fn replay_recorded_pty_bytes_chunk_invariant() {
    let bytes = capture_raw(
        "/bin/sh",
        // POSIX octal: dash (Linux /bin/sh) does not interpret \xNN.
        &["-c", "printf 'X\\033[1mB\\033[0m\\n\\342\\202\\254\\n'"],
    )
    .expect("capture_raw succeeds");
    assert!(!bytes.is_empty());
    assert!(bytes.windows(3).any(|w| w == b"\xe2\x82\xac"));
    let whole = replay_bytes(&bytes, 80, 24).expect("replay_bytes succeeds");
    for i in 1..bytes.len() {
        let split =
            replay_chunks([&bytes[..i], &bytes[i..]], 80, 24).expect("replay_chunks succeeds");
        assert_eq!(split.screen, whole.screen, "split at byte {i}");
    }
    let ones: Vec<&[u8]> = bytes.chunks(1).collect();
    assert_eq!(
        replay_chunks(ones, 80, 24)
            .expect("replay_chunks succeeds")
            .screen,
        whole.screen
    );
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
        tx.send(capture_raw("/bin/sh", &["-c", "exit 0"])).ok();
    });
    let bytes = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("capture_raw hung on silent immediate child exit")
        .expect("capture_raw succeeds");
    assert!(bytes.is_empty(), "unexpected bytes: {bytes:?}");
}

#[test]
fn replay_input_never_fed_as_output() {
    let mut rec = Recording::new(40, 10);
    rec.push_output(b"KEEP").expect("push_output succeeds");
    // A clear-screen + text: if fed, "KEEP" would vanish.
    rec.push_input(b"\x1b[2J\x1b[HRED")
        .expect("push_input succeeds");
    rec.push_output(b"VISIBLE").expect("push_output succeeds");
    let via_rec = replay_recording(&rec, None).expect("replay_recording succeeds");
    let direct = replay_bytes(b"KEEPVISIBLE", 40, 10).expect("replay_bytes succeeds");
    assert_eq!(via_rec.screen, direct.screen);
    assert_eq!(via_rec.bytes_fed, b"KEEPVISIBLE".len());
    assert!(contains(&via_rec.screen, "KEEPVISIBLE").expect("screen rows readable"));
    // Re-chunked replay agrees too.
    assert_eq!(
        replay_recording(&rec, Some(2))
            .expect("Some succeeds")
            .screen,
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
    let s = Tui::new([
        "/bin/sh",
        "-c",
        "stty -echo; printf 'A\\033[31mB\\033[0m\\nEND\\n'",
    ])
    .env("ENV", "/dev/null")
    .size(40, 10)
    .spawn()
    .expect("spawn succeeds");
    let waited = s
        .wait_exit(deadline(10), &cancel())
        .expect("wait_exit succeeds");
    assert!(waited.status.success());
    // PTY ONLCR translates \n to \r\n.
    let expected = b"A\x1b[31mB\x1b[0m\r\nEND\r\n";
    let replayed = replay_bytes(expected, 40, 10).expect("replay_bytes succeeds");
    assert_eq!(replayed.screen, waited.observation.screen);
    s.close().expect("close succeeds");
}
