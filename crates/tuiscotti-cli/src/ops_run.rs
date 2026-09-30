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

mod attach;

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
        SessionCmd::Attach { name } => attach::cmd_session_attach(&name),
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
