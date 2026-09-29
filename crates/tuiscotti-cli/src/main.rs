//! `tuisnap`: capture, inspect, sessions, render, diff, review/report.
//!
//! ```text
//! tuisnap init --dir .                  # scaffold tui-snap.toml + nextest config + example
//! tuisnap doctor                         # toolchain / fonts / profile / env report
//! tuisnap schema                         # print the op-protocol JSON schema
//! tuisnap capture --out shots/home -- ./my-tui --flag
//! tuisnap inspect --dir shots/home      # offline view; never executes
//! tuisnap render --input shot.frame.json --format png --out shot
//! tuisnap diff --expected a.png --actual b.png
//! tuisnap review --dir verdicts          # list verdicts; fails on any fail
//! tuisnap accept --store shots home      # approve one snapshot (explicit, per-name)
//! tuisnap report --dir verdicts --out report.html
//! tuisnap import --dir frozen           # read-only frozen-tree import
//! tuisnap session start --name demo -- ./my-tui
//! tuisnap record --out trace.jsonl -- ./my-tui
//! tuisnap trace --input trace.jsonl
//! tuisnap machine < ops.jsonl          # typed op protocol over stdio
//! ```
//!
//! Exit statuses: 0 ok; 2 CLI usage error; 3 op error
//! ([`tuiscotti::proto::EXIT_OP_ERROR`]); 4 verification disagreement
//! ([`tuiscotti::proto::EXIT_VERIFY_FAIL`]). `capture`/`record` preserve the
//! child's exit code instead. Full grammar in `SYNTAX.md`.

mod cli;
mod machine;
mod ops_offline;
mod ops_run;
mod ops_setup;

use clap::Parser;

/// Write a complete report to stdout without panicking on a closed pipe.
///
/// `println!` panics with EPIPE (`tuisnap doctor | head -c0` exits 101), so
/// pure-report commands buffer their output and flush once here: a broken
/// pipe is a clean exit 0 (the reader went away; nothing is lost), any other
/// error is reported on stderr with an op-error status.
pub(crate) fn write_stdout(text: &str) -> i32 {
    use std::io::Write as _;
    let mut out = std::io::stdout().lock();
    let res = out.write_all(text.as_bytes()).and_then(|()| out.flush());
    match res {
        Ok(()) => 0,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(e) => {
            eprintln!("error: stdout: {e}");
            tuiscotti::proto::EXIT_OP_ERROR
        }
    }
}

/// Write one line to stdout without panicking on a closed pipe.
///
/// Streaming counterpart to [`write_stdout`] for commands that emit output
/// incrementally (`machine`, `trace`, `session attach`): returns `None` on
/// success, `Some(exit_code)` when the caller must stop — `0` on a broken
/// pipe (the reader went away; nothing is lost) or
/// [`tuiscotti::proto::EXIT_OP_ERROR`] on any other stdout error.
pub(crate) fn write_line(line: &str) -> Option<i32> {
    use std::io::Write as _;
    let mut out = std::io::stdout().lock();
    let res = writeln!(out, "{line}").and_then(|()| out.flush());
    match res {
        Ok(()) => None,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Some(0),
        Err(e) => {
            eprintln!("error: stdout: {e}");
            Some(tuiscotti::proto::EXIT_OP_ERROR)
        }
    }
}

/// Append one `\n`-terminated line to a report buffer.
///
/// Exists so buffered commands never touch `writeln!`'s must-use `Result`
/// (the workspace denies both `let _ =` on must-use values and `unwrap`).
pub(crate) fn push_line(buf: &mut String, line: &str) {
    buf.push_str(line);
    buf.push('\n');
}

/// Write raw bytes to stdout without panicking on a closed pipe.
///
/// Byte counterpart to [`write_line`] for the `session attach` log tail
/// (log bytes are not necessarily UTF-8): `None` on success, `Some(code)`
/// when the caller must stop.
pub(crate) fn write_bytes(bytes: &[u8]) -> Option<i32> {
    use std::io::Write as _;
    let mut out = std::io::stdout().lock();
    let res = out.write_all(bytes).and_then(|()| out.flush());
    match res {
        Ok(()) => None,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Some(0),
        Err(e) => {
            eprintln!("error: stdout: {e}");
            Some(tuiscotti::proto::EXIT_OP_ERROR)
        }
    }
}

/// Flush a partial report, then report an op failure on stderr.
pub(crate) fn fail_flushed(buf: &str, msg: &str) -> i32 {
    let w = write_stdout(buf);
    if w != 0 {
        return w;
    }
    eprintln!("error: {msg}");
    tuiscotti::proto::EXIT_OP_ERROR
}

fn main() {
    // Native args_os parsing: child argv stays OsString end-to-end, and the
    // parser never sees values after `--` as its own flags.
    let cli = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => e.exit(), // clap usage error, exit 2
    };
    std::process::exit(run(cli));
}

fn run(cli: cli::Cli) -> i32 {
    match cli.cmd {
        cli::Cmd::Init { dir, force } => ops_setup::cmd_init(&dir, force),
        cli::Cmd::Doctor => ops_setup::cmd_doctor(),
        cli::Cmd::Schema => ops_setup::cmd_schema(),
        cli::Cmd::Capture {
            out,
            timeout_ms,
            argv,
        } => ops_run::cmd_capture(&out, timeout_ms, argv),
        cli::Cmd::Inspect { dir } => ops_offline::cmd_inspect(&dir),
        cli::Cmd::Render {
            input,
            formats,
            out,
            font_file,
        } => ops_offline::cmd_render(&input, &formats, &out, font_file.as_deref()),
        cli::Cmd::Diff { expected, actual } => ops_offline::cmd_diff(&expected, &actual),
        cli::Cmd::Review { dir } => ops_offline::cmd_review(&dir),
        cli::Cmd::Accept { name, store } => ops_offline::cmd_accept(&store, &name),
        cli::Cmd::Report { dir, out, title } => ops_offline::cmd_report(&dir, &out, &title),
        cli::Cmd::Import { dir } => ops_offline::cmd_import(&dir),
        cli::Cmd::Session { cmd } => ops_run::cmd_session(cmd),
        cli::Cmd::Record {
            out,
            max_events,
            max_bytes,
            argv,
        } => ops_run::cmd_record(&out, max_events, max_bytes, argv),
        cli::Cmd::Trace { input, kind } => ops_offline::cmd_trace(&input, kind),
        cli::Cmd::Machine => machine::machine_main(),
    }
}
