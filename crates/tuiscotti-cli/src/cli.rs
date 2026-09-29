//! Clap-native command line: typed subcommands, options, and formats.
//!
//! The parent parser never consumes an argument after `--`: child argv fields
//! use `last = true` plus [`std::ffi::OsString`], so a child argument like
//! `--machine` always reaches the child (see the `child_receives_dashdash`
//! regression test). Child argv is parsed from `args_os` (no lossy UTF-8
//! conversion, no Unicode panic).
//!
//! Exit policy (see also `SYNTAX.md`):
//! - `0`: ok.
//! - `2`: CLI usage error (clap).
//! - `3`: tool/op error ([`tuiscotti::proto::EXIT_OP_ERROR`]).
//! - `4`: verification disagreement ([`tuiscotti::proto::EXIT_VERIFY_FAIL`]).
//! - `capture`/`record` preserve the CHILD's exit code instead.

use clap::{Parser, Subcommand, ValueEnum};
use std::ffi::OsString;
use std::path::PathBuf;

/// Config responsibilities (kept in sync with `proto::CONFIG_DOCS`).
pub const INIT_HELP: &str = "\
Scaffold tui-snap.toml, nextest config, and an example test.

Config responsibilities:
  tui-snap.toml        Capture + assertion policy (viewport, terminal and
                       render profiles, gates, evidence dir). Owned by
                       tui-snap; read by tests via the Rust API.
  .config/nextest.toml Scheduling only (profiles, retries, threads, groups).
                       Owned by cargo-nextest; tui-snap never parses it.
  insta config         Snapshot review behaviour. Owned by Insta; tui-snap
                       honours it and never auto-accepts in CI.";

#[derive(Parser, Debug)]
#[command(
    name = "tuisnap",
    version,
    about = "TUI visual regression: capture, inspect, sessions, render, diff, review"
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

/// Offline render output format. Typed, so an unknown `--format` is a usage
/// error (exit 2) with the valid set listed — never a silent skip.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum RenderFormat {
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
    pub fn extension(self) -> &'static str {
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

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Scaffold tui-snap.toml, nextest config, and an example test.
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
        #[arg(long, default_value = "tuisnap visual report")]
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
        kind: Option<String>,
    },
    /// Machine interface: op JSON per line on stdin, one envelope per line
    /// on stdout. Exit 0 when every op succeeded, else 3.
    Machine,
}

#[derive(Subcommand, Debug)]
pub enum SessionCmd {
    /// Start a named session (detached child + endpoint file).
    Start {
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = false)]
        force: bool,
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
}
