use super::*;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// Default bound for collecting pipe output after the child was reaped.
/// Covers slow close plus descendants that inherited the pipes.
const DEFAULT_DRAIN_DEADLINE: Duration = Duration::from_secs(5);

/// Program to spawn: direct path/argv0, or a cargo binary resolved eagerly.
#[derive(Debug, Clone)]
pub(crate) enum Program {
    Direct(OsString),
    /// `Err` holds the resolution failure; [`Command::run`] reports it as
    /// [`Termination::SpawnError`] instead of spawning.
    CargoBin {
        name: OsString,
        resolved: Result<PathBuf, SpawnError>,
    },
}

/// First-class piped child-process builder (backlog R01).
///
/// No shell is involved unless [`.shell(true)`](Command::shell) opts in.
/// Environment entries and the working directory apply to the child only.
///
/// `Debug` is secret-safe: env values and stdin bytes are redacted (lengths
/// shown), so tokens passed to the child never leak into logs or snapshots.
#[derive(Clone)]
pub struct Command {
    pub(crate) program: Program,
    pub(crate) args: Vec<OsString>,
    pub(crate) env: Vec<(OsString, Option<OsString>)>,
    pub(crate) env_clear: bool,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) stdin_bytes: Option<Vec<u8>>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) output_limit: Option<usize>,
    pub(crate) drain_deadline: Duration,
    pub(crate) shell: bool,
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted_env: Vec<(OsString, Option<&str>)> = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.as_ref().map(|_| "<redacted>")))
            .collect();
        f.debug_struct("Command")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("env", &redacted_env)
            .field("env_clear", &self.env_clear)
            .field("cwd", &self.cwd)
            .field("stdin_bytes", &self.stdin_bytes.as_ref().map(Vec::len))
            .field("timeout", &self.timeout)
            .field("output_limit", &self.output_limit)
            .field("drain_deadline", &self.drain_deadline)
            .field("shell", &self.shell)
            .finish()
    }
}

impl Command {
    /// Spawn `argv0` directly (PATH lookup applies to bare names), no shell.
    pub fn new(argv0: impl AsRef<OsStr>) -> Self {
        Command {
            program: Program::Direct(argv0.as_ref().to_os_string()),
            args: Vec::new(),
            env: Vec::new(),
            env_clear: false,
            cwd: None,
            stdin_bytes: None,
            timeout: None,
            output_limit: None,
            drain_deadline: DEFAULT_DRAIN_DEADLINE,
            shell: false,
        }
    }

    /// Spawn a binary built by cargo (see [`cargo_bin_path`] for the lookup
    /// order). Resolution happens now; if it fails, [`Command::run`] returns
    /// [`Termination::SpawnError`] with the searched locations.
    pub fn cargo_bin(name: impl AsRef<OsStr>) -> Self {
        let name = name.as_ref().to_os_string();
        let resolved = cargo_bin_path(&name);
        Command {
            program: Program::CargoBin { name, resolved },
            args: Vec::new(),
            env: Vec::new(),
            env_clear: false,
            cwd: None,
            stdin_bytes: None,
            timeout: None,
            output_limit: None,
            drain_deadline: DEFAULT_DRAIN_DEADLINE,
            shell: false,
        }
    }

    /// Import spawn configuration (program, args, env, cwd) from a
    /// [`std::process::Command`]. Timeout/stdin/limits/shell are runtime
    /// behavior of this type and are left at defaults.
    pub fn from_std(cmd: &std::process::Command) -> Self {
        let mut out = Command::new(cmd.get_program());
        out.args.extend(cmd.get_args().map(|a| a.to_os_string()));
        for (k, v) in cmd.get_envs() {
            out.env
                .push((k.to_os_string(), v.map(|v| v.to_os_string())));
        }
        out.cwd = cmd.get_current_dir().map(|p| p.to_path_buf());
        out
    }

    /// Append one argument.
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    /// Append several arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for a in args {
            self.args.push(a.as_ref().to_os_string());
        }
        self
    }

    /// Set one child-only environment variable.
    pub fn env(mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>) -> Self {
        self.env.push((
            key.as_ref().to_os_string(),
            Some(val.as_ref().to_os_string()),
        ));
        self
    }

    /// Set several child-only environment variables.
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        for (k, v) in vars {
            self.env
                .push((k.as_ref().to_os_string(), Some(v.as_ref().to_os_string())));
        }
        self
    }

    /// Remove one variable from the child's environment.
    pub fn env_remove(mut self, key: impl AsRef<OsStr>) -> Self {
        self.env.push((key.as_ref().to_os_string(), None));
        self
    }

    /// Start the child with an empty environment (then apply `.env(...)`).
    pub fn env_clear(mut self, clear: bool) -> Self {
        self.env_clear = clear;
        self
    }

    /// Set the child's working directory.
    pub fn current_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Bytes to write to the child's stdin, then EOF (pipe closed).
    ///
    /// Without this, stdin is null (immediate EOF). A child that exits
    /// without reading stdin does not fail the run; unwritten input is
    /// silently dropped.
    pub fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin_bytes = Some(bytes.into());
        self
    }

    /// Kill the child and report [`Termination::Timeout`] after this long.
    /// No timeout by default.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Cap captured bytes per stream (stdout and stderr independently).
    /// When a stream exceeds the cap the child is killed and the run reports
    /// [`Termination::OutputLimit`] with `truncated: true`. No cap by default.
    pub fn output_limit(mut self, bytes: usize) -> Self {
        self.output_limit = Some(bytes);
        self
    }

    /// Bound for collecting pipe output after the child was reaped (default
    /// 5s). On expiry the run returns what was captured with `truncated:
    /// true`; reader threads detach and finish if the pipes ever close.
    /// This is also the bound for descendants that inherited the pipes:
    /// this module reaps only the direct child, never the process group.
    pub fn drain_deadline(mut self, deadline: Duration) -> Self {
        self.drain_deadline = deadline;
        self
    }

    /// Opt in to `/bin/sh -c <program>` with builder args passed as
    /// positional parameters (`$1`, ...; `$0` is `sh`).
    pub fn shell(mut self, enable: bool) -> Self {
        self.shell = enable;
        self
    }
}

impl From<&std::process::Command> for Command {
    fn from(cmd: &std::process::Command) -> Self {
        Command::from_std(cmd)
    }
}
