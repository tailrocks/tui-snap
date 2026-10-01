//! Clap-native command line: typed subcommands, options, and formats.
//!
//! The parent parser never consumes an argument after `--`: child argv fields
//! use `last = true` plus [`std::ffi::OsString`], so a child argument like
//! `--machine` always reaches the child (see the
//! `child_receives_double_dash_machine` regression test). Child argv is
//! parsed from `args_os` (no lossy UTF-8 conversion, no Unicode panic).
//!
//! Exit policy (see also `SYNTAX.md`):
//! - `0`: ok.
//! - `2`: CLI usage error (clap parse failure, unknown typed `--format` /
//!   `--kind`, empty `--format` list, or missing child argv after `--`).
//! - `3`: tool/op error ([`tuiscotti::proto::EXIT_OP_ERROR`]).
//! - `4`: verification disagreement ([`tuiscotti::proto::EXIT_VERIFY_FAIL`]).
//! - `capture`/`record` preserve the CHILD's exit code instead.

use clap::{Parser, Subcommand, ValueEnum};
use std::ffi::OsString;
use std::path::PathBuf;

/// Config responsibilities (kept in sync with `proto::CONFIG_DOCS`).
pub(crate) const INIT_HELP: &str = "\
Scaffold tuiscotti.toml, nextest config, and an example test.

Config responsibilities:
  tuiscotti.toml        Capture + assertion policy (viewport, terminal and
                       render profiles, gates, evidence dir). Owned by
                       tuiscotti; read by tests via the Rust API.
  .config/nextest.toml Scheduling only (profiles, retries, threads, groups).
                       Owned by cargo-nextest; tuiscotti never parses it.
  insta config         Snapshot review behaviour. Owned by Insta; tuiscotti
                       honours it and never auto-accepts in CI.";

#[derive(Parser, Debug)]
#[command(
    name = "tuiscotti",
    version,
    about = "TUI visual regression: capture, inspect, sessions, render, diff, review"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) cmd: Cmd,
}

/// Offline render output format. Typed, so an unknown `--format` is a usage
/// error (exit 2) with the valid set listed — never a silent skip.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum RenderFormat {
    /// Plain Unicode text, no escapes.
    Txt,
    /// Normalized VT/SGR screen dump.
    Ansi,
    /// Canonical frame JSON.
    Json,
    /// Static SVG render.
    Svg,
    /// Standalone offline HTML report.
    Html,
    /// Independently rendered PNG (+ `.fidelity.json` sidecar).
    Png,
}

impl RenderFormat {
    /// File extension for `--out <prefix>.<ext>` outputs.
    #[must_use]
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Ansi => "ansi",
            Self::Json => "json",
            Self::Svg => "svg",
            Self::Html => "html",
            Self::Png => "png",
        }
    }
}

/// Journal event kind filter for `trace`. Typed, so an unknown `--kind` is a
/// usage error (exit 2) with the valid set listed — never a silent no-match.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum TraceKind {
    /// Session start record (`argv=…`).
    Start,
    /// Captured output sizes record.
    Output,
    /// Child termination record.
    Exit,
    /// Journal completion record.
    Complete,
}

impl TraceKind {
    /// Journal `kind` string this variant filters on.
    #[must_use]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Output => "output",
            Self::Exit => "exit",
            Self::Complete => "complete",
        }
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum Cmd {
    /// Scaffold tuiscotti.toml, nextest config, and an example test.
    #[command(long_about = INIT_HELP)]
    Init {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Report toolchain, fonts, profiles, and environment.
    Doctor,
    /// Print the typed op-protocol JSON schema.
    Schema,
    /// Run a command and collect artifacts (preserves child exit code).
    Capture {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 60_000)]
        timeout_ms: u64,
        /// Child argv after `--` (never parsed as tool flags).
        #[arg(last = true)]
        argv: Vec<OsString>,
    },
    /// View artifacts offline. Never executes anything in the directory.
    Inspect {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Render a canonical frame.json to offline artifacts.
    Render {
        #[arg(long)]
        input: PathBuf,
        #[arg(long = "format")]
        formats: Vec<RenderFormat>,
        #[arg(long, default_value = "shot")]
        out: String,
        #[arg(long)]
        font_file: Option<PathBuf>,
    },
    /// Compare two PNGs by decoded pixels (exit 4 on mismatch).
    Diff {
        #[arg(long)]
        expected: PathBuf,
        #[arg(long)]
        actual: PathBuf,
    },
    /// List offline verdicts (exit 4 when any verdict fails).
    Review {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Approve one snapshot: actual → approved (explicit, per-name only;
    /// frozen roots reject).
    Accept {
        /// Snapshot name (e.g. `home`, `pages/overview`).
        name: String,
        /// Snapshot store root (holds `approved/` + `actual/`).
        #[arg(long, default_value = ".")]
        store: PathBuf,
    },
    /// Write a standalone offline HTML report from verdicts.
    Report {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "tuiscotti visual report")]
        title: String,
    },
    /// Read-only import of a frozen four-artifact tree (writes nothing).
    Import {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Manage named sessions (versioned endpoints, owner-only runtime dir).
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
    /// Run a command with bounded event recording (preserves exit code).
    Record {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 10_000)]
        max_events: u64,
        #[arg(long, default_value_t = 10_000_000)]
        max_bytes: u64,
        /// Child argv after `--` (never parsed as tool flags).
        #[arg(last = true)]
        argv: Vec<OsString>,
    },
    /// View a recorded journal offline.
    Trace {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        kind: Option<TraceKind>,
    },
    /// Machine interface: op JSON per line on stdin, one envelope per line
    /// on stdout. Exit 0 when every op succeeded, else 3.
    Machine,
    /// Retained-session daemon entry (hidden: autostarted, never typed).
    #[command(name = "__daemon", hide = true)]
    DaemonInternal,
}

#[derive(Subcommand, Debug)]
pub(crate) enum SessionCmd {
    /// Start a named session (detached child + endpoint file).
    Start {
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = false)]
        force: bool,
        /// Retain a PTY session behind the daemon (input/observe/attach
        /// across invocations) instead of a piped child.
        #[arg(long, default_value_t = false)]
        pty: bool,
        /// PTY width in cells (pairs with `--rows`; default 80x24).
        #[arg(long)]
        cols: Option<u16>,
        /// PTY height in cells (pairs with `--cols`).
        #[arg(long)]
        rows: Option<u16>,
        /// Child argv after `--` (never parsed as tool flags).
        #[arg(last = true)]
        argv: Vec<OsString>,
    },
    /// Stop a named session and remove its endpoint.
    Stop {
        #[arg(long)]
        name: String,
    },
    /// List named sessions with liveness.
    List,
    /// Remove endpoints whose process already exited.
    Prune,
    /// Attach to a named session (best-effort human view; EOF detaches).
    Attach {
        #[arg(long)]
        name: String,
    },
    /// Send input to a retained PTY session (exactly one payload).
    Input {
        #[arg(long)]
        name: String,
        /// Literal text to type.
        #[arg(long)]
        text: Option<String>,
        /// Key chord to press.
        #[arg(long)]
        chord: Option<String>,
        /// Raw bytes, base64.
        #[arg(long)]
        bytes_b64: Option<String>,
    },
    /// Print a retained PTY session's current screen text.
    Observe {
        #[arg(long)]
        name: String,
    },
}

/// Dispatch a parsed [`Cli`] to its `ops_*` handler; returns the exit code.
pub(crate) fn run(cli: Cli) -> i32 {
    match cli.cmd {
        Cmd::Init { dir, force } => crate::ops_setup::cmd_init(&dir, force),
        Cmd::Doctor => crate::ops_setup::cmd_doctor(),
        Cmd::Schema => crate::ops_setup::cmd_schema(),
        Cmd::Capture {
            out,
            timeout_ms,
            argv,
        } => crate::ops_run::cmd_capture(&out, timeout_ms, &argv),
        Cmd::Inspect { dir } => crate::ops_offline::cmd_inspect(&dir),
        Cmd::Render {
            input,
            formats,
            out,
            font_file,
        } => crate::ops_offline::cmd_render(&input, &formats, &out, font_file.as_deref()),
        Cmd::Diff { expected, actual } => crate::ops_offline::cmd_diff(&expected, &actual),
        Cmd::Review { dir } => crate::ops_offline::cmd_review(&dir),
        Cmd::Accept { name, store } => crate::ops_offline::cmd_accept(&store, &name),
        Cmd::Report { dir, out, title } => crate::ops_offline::cmd_report(&dir, &out, &title),
        Cmd::Import { dir } => crate::ops_offline::cmd_import(&dir),
        Cmd::Session { cmd } => crate::ops_run::cmd_session(cmd),
        Cmd::Record {
            out,
            max_events,
            max_bytes,
            argv,
        } => crate::ops_run::cmd_record(&out, max_events, max_bytes, &argv),
        Cmd::Trace { input, kind } => crate::ops_offline::cmd_trace(&input, kind),
        Cmd::Machine => crate::machine::machine_main(),
        Cmd::DaemonInternal => crate::ops_run::cmd_daemon(),
    }
}
