//! Run commands: `capture`, `record`, `session`.
//!
//! These spawn children. Child argv arrives as [`OsString`](std::ffi::OsString)
//! from `args_os` and spawns byte-exact; only the UTF-8 JSON records
//! (manifests, session endpoints) carry lossy projections, each documented at
//! the conversion.
//!
//! Every stdout path goes through [`crate::write_stdout`] (buffered) or
//! [`crate::write_line`] (streaming): no `println!`, so a closed pipe is a
//! clean exit 0 instead of an EPIPE panic (exit 101).

use std::ffi::OsString;
use std::path::Path;

use tuiscotti::proto::{self, EXIT_OP_ERROR, EXIT_USAGE};

use crate::cli::SessionCmd;
use crate::ops_offline::op_error;

/// Lossy UTF-8 projection of child argv for JSON records (manifests,
/// endpoints). The BYTES spawned are always the exact [`OsString`] argv;
/// JSON cannot carry non-UTF-8, so records are explicitly diagnostic.
fn argv_display(argv: &[OsString]) -> Vec<String> {
    argv.iter()
        .map(|a| a.as_os_str().to_string_lossy().into_owned())
        .collect()
}

pub(crate) fn cmd_capture(out: &Path, timeout_ms: u64, argv: &[OsString]) -> i32 {
    if argv.is_empty() {
        eprintln!("error: pass the command after `--`");
        return EXIT_USAGE;
    }
    if let Err(e) = std::fs::create_dir_all(out) {
        eprintln!("error: mkdir {}: {e}", out.display());
        return EXIT_OP_ERROR;
    }
    let result = tuiscotti::command::Command::new(&argv[0])
        .args(&argv[1..])
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .run();
    if let Err(e) = std::fs::write(out.join("stdout.bin"), &result.stdout) {
        eprintln!("error: write stdout.bin: {e}");
        return EXIT_OP_ERROR;
    }
    if let Err(e) = std::fs::write(out.join("stderr.bin"), &result.stderr) {
        eprintln!("error: write stderr.bin: {e}");
        return EXIT_OP_ERROR;
    }
    let manifest = serde_json::json!({
        "argv": argv_display(argv),
        "termination": format!("{:?}", result.status),
        "code": result.code(),
        "signal": result.signal(),
        "truncated": result.truncated,
        "elapsed_ms": u64::try_from(result.elapsed.as_millis()).unwrap_or(u64::MAX),
        "stdout_bytes": result.stdout.len(),
        "stderr_bytes": result.stderr.len(),
    });
    if let Err(e) = std::fs::write(
        out.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    ) {
        eprintln!("error: write manifest.json: {e}");
        return EXIT_OP_ERROR;
    }
    let buf = format!("captured {:?} -> {}\n", result.status, out.display());
    let w = crate::write_stdout(&buf);
    if w != 0 {
        return w;
    }
    match result.status {
        tuiscotti::command::Termination::Exit(c) => c,
        _ => EXIT_OP_ERROR,
    }
}

pub(crate) fn cmd_daemon() -> i32 {
    proto::daemon_main()
}

/// Start a named session: piped by default, retained PTY with `--pty`.
fn cmd_session_start(
    name: &str,
    force: bool,
    pty: bool,
    cols: Option<u16>,
    rows: Option<u16>,
    argv: &[OsString],
) -> i32 {
    if argv.is_empty() {
        eprintln!("error: pass the command after `--`");
        return EXIT_USAGE;
    }
    let started = if pty {
        proto::session_start_pty(name, argv, force, cols, rows)
    } else {
        if cols.is_some() || rows.is_some() {
            eprintln!("error: --cols/--rows need --pty");
            return EXIT_USAGE;
        }
        proto::session_start_os(name, argv, force)
    };
    match started {
        Ok(info) => {
            let buf = format!("started: {} (pid {})\n", info.name, info.pid);
            crate::write_stdout(&buf)
        }
        Err(e) => op_error(&e),
    }
}

pub(crate) fn cmd_session(cmd: SessionCmd) -> i32 {
    match cmd {
        SessionCmd::Start {
            name,
            force,
            pty,
            cols,
            rows,
            argv,
        } => cmd_session_start(&name, force, pty, cols, rows, &argv),
        SessionCmd::Stop { name } => match proto::session_stop(&name) {
            Ok(info) => {
                let buf = format!("stopped: {} (pid {})\n", info.name, info.pid);
                crate::write_stdout(&buf)
            }
            Err(e) => op_error(&e),
        },
        SessionCmd::List => match proto::session_list() {
            Ok(sessions) => {
                let mut buf = String::new();
                if sessions.is_empty() {
                    crate::push_line(&mut buf, "no sessions");
                }
                for s in sessions {
                    crate::push_line(
                        &mut buf,
                        &format!(
                            "{} pid={} {:?}/{:?} started={} argv={:?}",
                            s.name, s.pid, s.backend, s.status, s.started_unix, s.argv
                        ),
                    );
                }
                crate::write_stdout(&buf)
            }
            Err(e) => op_error(&e),
        },
        SessionCmd::Prune => match proto::session_prune() {
            Ok(pruned) => {
                let mut buf = String::new();
                crate::push_line(&mut buf, &format!("pruned {} session(s)", pruned.len()));
                for name in pruned {
                    crate::push_line(&mut buf, &format!("  {name}"));
                }
                crate::write_stdout(&buf)
            }
            Err(e) => op_error(&e),
        },
        SessionCmd::Attach { name } => cmd_session_attach(&name),
        SessionCmd::Input {
            name,
            text,
            chord,
            bytes_b64,
        } => match proto::session_input(&name, text, chord, bytes_b64) {
            Ok(()) => {
                let buf = format!("input accepted: {name}\n");
                crate::write_stdout(&buf)
            }
            Err(e) => op_error(&e),
        },
        SessionCmd::Observe { name } => match proto::session_observe(&name) {
            Ok(obs) => {
                let mut buf = format!(
                    "revision={} reason={} {}x{}\n",
                    obs.revision, obs.reason, obs.screen.cols, obs.screen.rows
                );
                buf.push_str(&obs.screen.text);
                if !obs.screen.text.ends_with('\n') {
                    buf.push('\n');
                }
                crate::write_stdout(&buf)
            }
            Err(e) => op_error(&e),
        },
    }
}

/// Best-effort human view of a named session: tails the session log as text
/// frames. Assertions remain on `Observation`s, never on this output.
/// Detached process sessions have no input transport (stdin is null), so
/// stdin bytes are drained and discarded; EOF on stdin detaches.
/// Drain stdin on a thread (process sessions have no input transport,
/// so the bytes are discarded): returns the flag the drain sets on EOF.
fn spawn_stdin_drain() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    use std::io::Read;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let eof = Arc::new(AtomicBool::new(false));
    let stdin_eof = Arc::clone(&eof);
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        let mut stdin = std::io::stdin().lock();
        let mut discarded: u64 = 0;
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => discarded += n as u64,
            }
        }
        if discarded > 0 {
            eprintln!("note: discarded {discarded} input byte(s): no input transport");
        }
        stdin_eof.store(true, Ordering::SeqCst);
    });
    eof
}

fn cmd_session_attach(name: &str) -> i32 {
    // Piped sessions tail the log; retained PTY sessions poll observe
    // frames and forward stdin. Unknown names take the piped path so a
    // hostile `--name` still fails name validation first (exit 3).
    let backend = match proto::session_list() {
        Ok(list) => list
            .into_iter()
            .find(|s| s.name == *name)
            .map(|s| s.backend),
        Err(e) => return op_error(&e),
    };
    match backend {
        Some(proto::SessionBackend::Pty) => cmd_session_attach_pty(name),
        _ => cmd_session_attach_process(name),
    }
}

/// Best-effort human view of a retained PTY session: observe frames on
/// revision change, stdin bytes forwarded as input. EOF or session end
/// detaches. Assertions remain on `Observation`s, never on this output.
fn cmd_session_attach_pty(name: &str) -> i32 {
    use std::sync::atomic::Ordering;
    if let Some(code) = crate::write_line(&format!(
        "attached: {name} (pty) — best-effort human view; assertions stay on Observations"
    )) {
        return code;
    }
    if let Some(code) = crate::write_line("stdin is forwarded to the session; EOF detaches") {
        return code;
    }
    let eof = spawn_stdin_forward(name);
    let mut last_revision = None;
    loop {
        // Observe first so an attach that opens on EOF still renders one
        // frame before detaching.
        match proto::session_observe(name) {
            Ok(obs) => {
                if last_revision != Some(obs.revision) {
                    last_revision = Some(obs.revision);
                    let mut frame = obs.screen.text;
                    if !frame.ends_with('\n') {
                        frame.push('\n');
                    }
                    if let Some(code) = crate::write_bytes(frame.as_bytes()) {
                        return code;
                    }
                }
            }
            Err(e) if e.code == "not-found" => {
                // Exited or orphaned between polls; the status check below
                // reports which.
            }
            Err(e) => return op_error(&e),
        }
        if eof.load(Ordering::SeqCst) {
            return detach_verdict(name, "detached: stdin EOF");
        }
        if !pty_running(name) {
            return crate::write_line("detached: session ended").unwrap_or(0);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// EOF arrived (or forwarding broke): say which if the session is gone.
fn detach_verdict(name: &str, eof_msg: &str) -> i32 {
    if pty_running(name) {
        crate::write_line(eof_msg).unwrap_or(0)
    } else {
        crate::write_line("detached: session ended").unwrap_or(0)
    }
}

/// True while the named session lists as `Running`.
fn pty_running(name: &str) -> bool {
    proto::session_list().is_ok_and(|l| {
        l.iter()
            .any(|s| s.name == *name && s.status == proto::SessionStatus::Running)
    })
}

/// Forward stdin bytes to the PTY session until EOF or delivery failure;
/// returns the flag set when forwarding stops.
fn spawn_stdin_forward(name: &str) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    use std::io::Read;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    let name = name.to_string();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut stdin = std::io::stdin().lock();
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if proto::session_input_bytes(&name, &buf[..n]).is_err() {
                        break;
                    }
                }
            }
        }
        flag.store(true, Ordering::SeqCst);
    });
    done
}

fn cmd_session_attach_process(name: &str) -> i32 {
    use std::sync::atomic::Ordering;
    // Validated + containment-checked first: a hostile `--name` must not
    // steer the log path outside the runtime dir.
    let log_path = match proto::session_log_path(name) {
        Ok(p) => p,
        Err(e) => return op_error(&e),
    };
    let info = match proto::session_list() {
        Ok(list) => list.into_iter().find(|s| s.name == *name),
        Err(e) => return op_error(&e),
    };
    let Some(info) = info else {
        eprintln!("error: [not-found] no session {name:?}");
        return EXIT_OP_ERROR;
    };
    match std::fs::symlink_metadata(&log_path) {
        Ok(m) if m.file_type().is_file() => {}
        Ok(_) => {
            eprintln!("error: [invalid-input] log for session {name:?} is not a regular file");
            return EXIT_OP_ERROR;
        }
        Err(_) => {
            eprintln!("error: [not-found] no log for session {name:?}");
            return EXIT_OP_ERROR;
        }
    }
    if let Some(code) = crate::write_line(&format!(
        "attached: {} (pid {} {:?}) — best-effort human view; assertions stay on Observations",
        info.name, info.pid, info.status
    )) {
        return code;
    }
    if let Some(code) = crate::write_line(
        "stdin is not delivered (process sessions have no input transport); EOF detaches",
    ) {
        return code;
    }
    let eof = spawn_stdin_drain();
    // Bounded incremental tail (F12): each poll reads only new bytes
    // (never the whole file), capped per poll and over the attach's life.
    let mut tail = match proto::LogTail::open(&log_path) {
        Ok(t) => t,
        Err(e) => return op_error(&e),
    };
    let mut truncation_noted = false;
    loop {
        if eof.load(Ordering::SeqCst) {
            // A closed pipe here is also a clean exit 0 (same code either
            // way), so the writer result folds into the return.
            return crate::write_line("detached: stdin EOF").unwrap_or(0);
        }
        match tail.poll() {
            Ok(bytes) => {
                if !bytes.is_empty()
                    && let Some(code) = crate::write_bytes(&bytes)
                {
                    return code;
                }
            }
            Err(e) => return op_error(&e),
        }
        if tail.truncated() && !truncation_noted {
            truncation_noted = true;
            eprintln!("note: log tail truncated (cap reached or log replaced); following");
        }
        let alive = proto::session_list().is_ok_and(|l| {
            l.iter()
                .any(|s| s.name == *name && s.status == proto::SessionStatus::Running)
        });
        if !alive {
            return crate::write_line("detached: session ended").unwrap_or(0);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

pub(crate) fn cmd_record(out: &Path, max_events: u64, max_bytes: u64, argv: &[OsString]) -> i32 {
    if argv.is_empty() {
        eprintln!("error: pass the command after `--`");
        return EXIT_USAGE;
    }
    let mut rec = match proto::Recorder::create(out, max_events, max_bytes) {
        Ok(r) => r,
        Err(e) => return op_error(&e),
    };
    let fail = |e: proto::OpError| {
        eprintln!("error: {e}");
        EXIT_OP_ERROR
    };
    if let Err(e) = rec.record("start", &format!("argv={:?}", argv_display(argv))) {
        return fail(e);
    }
    let result = tuiscotti::command::Command::new(&argv[0])
        .args(&argv[1..])
        .run();
    if let Err(e) = rec.record(
        "output",
        &format!(
            "stdout={} stderr={} truncated={}",
            result.stdout.len(),
            result.stderr.len(),
            result.truncated
        ),
    ) {
        return fail(e);
    }
    if let Err(e) = rec.record(
        "exit",
        &format!(
            "termination={:?} code={:?} signal={:?}",
            result.status,
            result.code(),
            result.signal()
        ),
    ) {
        return fail(e);
    }
    if let Err(e) = rec.record("complete", &format!("events={}", rec.events() + 1)) {
        return fail(e);
    }
    let buf = format!("recorded {} events -> {}\n", rec.events(), out.display());
    let w = crate::write_stdout(&buf);
    if w != 0 {
        return w;
    }
    match result.status {
        tuiscotti::command::Termination::Exit(c) => c,
        _ => EXIT_OP_ERROR,
    }
}
