//! `xtask`: typed Rust maintenance automation (G3).
//!
//! Zero-dependency safe Rust. Each module owns one subcommand; this root only
//! dispatches. Run via `cargo run -p xtask -- <subcommand>` (plain `cargo` is
//! already the Mise/mbx-managed entry point in this repo).

pub mod bench;
pub mod bench_envelope;
pub mod bench_probe;
pub mod bench_score;
pub mod brand;
pub mod deps;
pub mod docs;
pub mod fixtures;
pub mod fonts;
pub mod migrate;
pub mod package;
pub mod perf;
pub mod policy;
pub mod sha256;
pub mod util;

use std::path::Path;
use util::{Result, Status};

/// Global usage text.
pub const HELP: &str = "\
usage: cargo xtask <subcommand> [args]\n\
\n\
maintenance automation (each subcommand also accepts --help):\n\
  migrate    workspace migration-shape checks\n\
  policy     source policies: crates-only, no foreign scripts, line limits\n\
  fixtures   build fixture binaries, resolve authoritative artifacts\n\
  brand      old-brand scan over active paths\n\
  deps       dependency inspection (bans, git sources, lockfile)\n\
  docs       docs/examples checks (stale script refs, relative links)\n\
  perf       performance collection into target/xtask-perf/\n\
  bench      reproducible benchmark suite into benches/results/\n\
  package    packaging dry-run for publishable members\n\
  fonts      font-byte maintenance (verify | record)\n\
  help       print this text\n\
\nexit codes: 0 clean, 1 findings or failure, 2 usage error\n";

/// Dispatch the command line to one maintenance subcommand; returns the exit code.
pub fn run(command_line: &[String]) -> i32 {
    if std::env::var(util::GUARD_ENV).as_deref() == Ok("1") {
        eprintln!("xtask: refusing recursive invocation ({})", util::GUARD_ENV);
        return 1;
    }
    let root = match util::workspace_root() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("xtask: {err}");
            return 1;
        }
    };
    let cmd = command_line.get(1).map_or("--help", String::as_str);
    let args = command_line.get(2..).unwrap_or(&[]);
    match cmd {
        "--help" | "-h" | "help" => {
            println!("{HELP}");
            0
        }
        migrate::NAME => dispatch(&root, args, migrate::run),
        policy::NAME => dispatch(&root, args, policy::run),
        fixtures::NAME => dispatch(&root, args, fixtures::run),
        brand::NAME => dispatch(&root, args, brand::run),
        deps::NAME => dispatch(&root, args, deps::run),
        docs::NAME => dispatch(&root, args, docs::run),
        perf::NAME => dispatch(&root, args, perf::run),
        bench::NAME => dispatch(&root, args, bench::run),
        package::NAME => dispatch(&root, args, package::run),
        fonts::NAME => dispatch(&root, args, fonts::run),
        other => {
            eprintln!("xtask: unknown subcommand '{other}'\n\n{HELP}");
            2
        }
    }
}

fn dispatch(root: &Path, args: &[String], f: fn(&Path, &[String]) -> Result<Status>) -> i32 {
    match f(root, args) {
        Ok(status) => status.code(),
        Err(err) => {
            eprintln!("xtask: {err}");
            1
        }
    }
}
