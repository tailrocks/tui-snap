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
        cli::Cmd::Trace { input, kind } => ops_offline::cmd_trace(&input, kind.as_deref()),
        cli::Cmd::Machine => machine::machine_main(),
    }
}
