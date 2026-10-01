//! First-class piped child processes (backlog R01–R03).
//!
//! [`Command`] is a small std-only builder for spawning a child with piped
//! stdio, running it to completion, and collecting [`ProcessOutput`]: separate
//! raw stdout/stderr bytes plus an honest [`Termination`] classification.
//!
//! Design points:
//! - No shell by default. [`Command::shell`] opts in to `/bin/sh -c` with the
//!   program as the script and builder args as positional parameters.
//! - Exit code, signal death, timeout kill, output-limit kill, and spawn
//!   failure are distinct [`Termination`] variants; none is conflated.
//! - Bytes are preserved exactly, including non-UTF-8; no stdout/stderr
//!   merge or invented cross-pipe ordering.
//! - All environment changes are child-only; the parent process env is never
//!   touched. [`isolated_env`] builds temp HOME/XDG/cwd fixtures.
//!
//! This module deliberately does not reimplement the `assert_cmd` ecosystem:
//! use [`Command::from_std`] / [`Command::std_command`] to interoperate with
//! [`std::process::Command`] instead.

mod build;
mod isolated;
mod run;
mod types;

pub use build::*;
pub use isolated::*;
pub use types::*;
