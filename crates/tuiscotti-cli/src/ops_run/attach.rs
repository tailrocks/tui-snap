//! `session attach`: best-effort human views of live sessions.
//!
//! Moved out of `ops_run.rs` so both files stay under the repo line gate;
//! behavior is unchanged.

use tuiscotti::proto::{self, EXIT_OP_ERROR};

use crate::ops_offline::op_error;

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

pub(super) fn cmd_session_attach(name: &str) -> i32 {
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
