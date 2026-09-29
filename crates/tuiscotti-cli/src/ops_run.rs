//! Run commands: `capture`, `record`, `session`.
//!
//! These spawn children. Child argv arrives as [`OsString`](std::ffi::OsString)
//! from `args_os` and spawns byte-exact; only the UTF-8 JSON records
//! (manifests, session endpoints) carry lossy projections, each documented at
//! the conversion.

use std::ffi::OsString;
use std::path::Path;

use tuiscotti::proto::{self, EXIT_OP_ERROR};

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

pub fn cmd_capture(out: &Path, timeout_ms: u64, argv: Vec<OsString>) -> i32 {
    if argv.is_empty() {
        eprintln!("error: pass the command after `--`");
        return EXIT_OP_ERROR;
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
        "argv": argv_display(&argv),
        "termination": format!("{:?}", result.status),
        "code": result.code(),
        "signal": result.signal(),
        "truncated": result.truncated,
        "elapsed_ms": result.elapsed.as_millis() as u64,
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
    println!("captured {:?} -> {}", result.status, out.display());
    match result.status {
        tuiscotti::command::Termination::Exit(c) => c,
        _ => EXIT_OP_ERROR,
    }
}

pub fn cmd_session(cmd: SessionCmd) -> i32 {
    match cmd {
        SessionCmd::Start { name, force, argv } => {
            match proto::session_start_os(&name, &argv, force) {
                Ok(info) => {
                    println!("started: {} (pid {})", info.name, info.pid);
                    0
                }
                Err(e) => op_error(&e),
            }
        }
        SessionCmd::Stop { name } => match proto::session_stop(&name) {
            Ok(info) => {
                println!("stopped: {} (was {:?})", info.name, info.status);
                0
            }
            Err(e) => op_error(&e),
        },
        SessionCmd::List => match proto::session_list() {
            Ok(sessions) => {
                if sessions.is_empty() {
                    println!("no sessions");
                }
                for s in sessions {
                    println!(
                        "{} pid={} {:?} started={} argv={:?}",
                        s.name, s.pid, s.status, s.started_unix, s.argv
                    );
                }
                0
            }
            Err(e) => op_error(&e),
        },
        SessionCmd::Prune => match proto::session_prune() {
            Ok(pruned) => {
                println!("pruned {} session(s)", pruned.len());
                for name in pruned {
                    println!("  {name}");
                }
                0
            }
            Err(e) => op_error(&e),
        },
        SessionCmd::Attach { name } => cmd_session_attach(&name),
    }
}

/// Best-effort human view of a named session: tails the session log as text
/// frames. Assertions remain on `Observation`s, never on this output.
/// Detached process sessions have no input transport (stdin is null), so
/// stdin bytes are drained and discarded; EOF on stdin detaches.
fn cmd_session_attach(name: &str) -> i32 {
    use std::io::Read;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let info = match proto::session_list() {
        Ok(list) => list.into_iter().find(|s| s.name == *name),
        Err(e) => return op_error(&e),
    };
    let Some(info) = info else {
        eprintln!("error: [not-found] no session {name:?}");
        return EXIT_OP_ERROR;
    };
    let dir = match proto::runtime_dir() {
        Ok(d) => d,
        Err(e) => return op_error(&e),
    };
    let log_path = dir.join(format!("{name}.log"));
    if !log_path.is_file() {
        eprintln!("error: [not-found] no log for session {name:?}");
        return EXIT_OP_ERROR;
    }
    println!(
        "attached: {} (pid {} {:?}) — best-effort human view; assertions stay on Observations",
        info.name, info.pid, info.status
    );
    println!("stdin is not delivered (process sessions have no input transport); EOF detaches");
    let eof = Arc::new(AtomicBool::new(false));
    let stdin_eof = Arc::clone(&eof);
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        let mut stdin = std::io::stdin().lock();
        let mut discarded: u64 = 0;
        loop {
            match stdin.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => discarded += n as u64,
                Err(_) => break,
            }
        }
        if discarded > 0 {
            eprintln!("note: discarded {discarded} input byte(s): no input transport");
        }
        stdin_eof.store(true, Ordering::SeqCst);
    });
    let mut offset: u64 = 0;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    loop {
        if eof.load(Ordering::SeqCst) {
            println!("detached: stdin EOF");
            return 0;
        }
        let bytes = std::fs::read(&log_path).unwrap_or_default();
        if bytes.len() as u64 > offset {
            use std::io::Write;
            let _ = out.write_all(&bytes[offset as usize..]);
            let _ = out.flush();
            offset = bytes.len() as u64;
        }
        let alive = proto::session_list()
            .map(|l| {
                l.iter()
                    .any(|s| s.name == *name && s.status == proto::SessionStatus::Running)
            })
            .unwrap_or(false);
        if !alive {
            println!("detached: session ended");
            return 0;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

pub fn cmd_record(out: &Path, max_events: u64, max_bytes: u64, argv: Vec<OsString>) -> i32 {
    if argv.is_empty() {
        eprintln!("error: pass the command after `--`");
        return EXIT_OP_ERROR;
    }
    let mut rec = match proto::Recorder::create(out, max_events, max_bytes) {
        Ok(r) => r,
        Err(e) => return op_error(&e),
    };
    let fail = |e: proto::OpError| {
        eprintln!("error: {e}");
        EXIT_OP_ERROR
    };
    if let Err(e) = rec.record("start", &format!("argv={:?}", argv_display(&argv))) {
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
    println!("recorded {} events -> {}", rec.events(), out.display());
    match result.status {
        tuiscotti::command::Termination::Exit(c) => c,
        _ => EXIT_OP_ERROR,
    }
}
