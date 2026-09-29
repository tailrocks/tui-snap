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

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// How a child process run ended.
///
/// Every outcome is distinct: a timeout kill reports [`Termination::Timeout`]
/// even though the OS-level status is signal death, and an output-limit kill
/// reports [`Termination::OutputLimit`]. Only genuine child signal death —
/// where this module did not kill the child — reports [`Termination::Signal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    /// Child exited normally with this status code.
    Exit(i32),
    /// Child died from this signal without being killed by this module.
    ///
    /// Unix only; on other platforms signal death is reported as
    /// [`Termination::Exit`] with the process status code.
    Signal(i32),
    /// The configured [`Command::timeout`] elapsed; the child was killed.
    Timeout,
    /// A stream exceeded [`Command::output_limit`]; the child was killed.
    OutputLimit,
    /// The child could not be spawned (missing binary, bad cwd, ...).
    /// Detail is in [`ProcessOutput::error`].
    SpawnError,
}

impl Termination {
    /// True only for `Exit(0)`.
    #[must_use]
    pub fn success(&self) -> bool {
        matches!(self, Termination::Exit(0))
    }

    /// Exit code when the child exited normally.
    #[must_use]
    pub fn code(&self) -> Option<i32> {
        match self {
            Termination::Exit(c) => Some(*c),
            _ => None,
        }
    }

    /// Signal number when the child died from a signal on its own.
    #[must_use]
    pub fn signal(&self) -> Option<i32> {
        match self {
            Termination::Signal(s) => Some(*s),
            _ => None,
        }
    }
}

/// Collected result of one [`Command::run`].
///
/// `stdout`/`stderr` hold raw bytes exactly as read: no UTF-8 validation,
/// no newline translation, no cross-stream interleaving.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    /// Raw bytes read from the child's stdout.
    pub stdout: Vec<u8>,
    /// Raw bytes read from the child's stderr.
    pub stderr: Vec<u8>,
    /// How the run ended.
    pub status: Termination,
    /// True iff produced bytes were not captured: a stream exceeded
    /// [`Command::output_limit`], or [`Command::drain_deadline`] expired while
    /// pipes were still open (typically descendants inheriting them).
    ///
    /// Killing the child on [`Termination::Timeout`] does not by itself set
    /// this: it is set only when bytes the child wrote were dropped.
    pub truncated: bool,
    /// Wall time from spawn (or spawn attempt) until output collection ended.
    pub elapsed: Duration,
    /// Auxiliary detail: spawn-failure message for [`Termination::SpawnError`].
    pub error: Option<String>,
}

impl ProcessOutput {
    /// True only for `status == Exit(0)`.
    #[must_use]
    pub fn success(&self) -> bool {
        self.status.success()
    }

    /// Exit code when the child exited normally.
    #[must_use]
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }

    /// Signal number when the child died from a signal on its own.
    #[must_use]
    pub fn signal(&self) -> Option<i32> {
        self.status.signal()
    }

    /// Best-effort UTF-8 view of stdout (lossy; [`Self::stdout`] is intact).
    #[must_use]
    pub fn stdout_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Best-effort UTF-8 view of stderr (lossy; [`Self::stderr`] is intact).
    #[must_use]
    pub fn stderr_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Resolution of a `cargo_bin` target dir, for error messages.
fn cargo_bin_candidates(name: &OsStr) -> Vec<PathBuf> {
    let mut out = Vec::new();
    // 1. Current executable's directory layout: tests live in
    //    target/<profile>/deps/, binaries in target/<profile>/.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(deps) = exe.parent() {
            out.push(deps.join(name));
            if deps.file_name().is_some_and(|n| n == "deps") {
                if let Some(profile) = deps.parent() {
                    out.push(profile.join(name));
                }
            }
        }
    }
    // 2. target/<profile>/<name> relative to the process working directory
    //    (covers `cargo test` from the package root).
    for profile in ["debug", "release"] {
        out.push(PathBuf::from("target").join(profile).join(name));
    }
    out
}

/// Resolve the path of a cargo-built binary named `name`.
///
/// Lookup order (runtime only, so remapped paths are honored and no stale
/// build-time absolute path is ever baked in — backlog N03):
/// 1. `CARGO_BIN_EXE_<name>` from the process environment, when set.
/// 2. Next to the current executable, then next to its parent when the
///    executable lives in a `deps/` directory (integration-test layout).
/// 3. `target/debug/<name>` and `target/release/<name>` under the cwd.
///
/// Callers that prefer the test crate's compile-time path can instead write
/// `Command::new(env!("CARGO_BIN_EXE_<name>"))` in the test itself; that
/// `env!` must expand in the test crate, not in this library.
///
/// Returns the first candidate that exists, else an error listing every
/// location that was searched.
pub fn cargo_bin_path(name: impl AsRef<OsStr>) -> Result<PathBuf, String> {
    let name = name.as_ref();
    let file = Path::new(name).file_name().unwrap_or(name);
    let var = format!("CARGO_BIN_EXE_{}", file.to_string_lossy());
    let mut searched = Vec::new();
    if let Ok(p) = std::env::var(&var) {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        searched.push(p);
    }
    for c in cargo_bin_candidates(file) {
        if c.is_file() {
            return Ok(c);
        }
        searched.push(c);
    }
    Err(format!(
        "binary `{}` not found; set {var} or build it first (searched: {})",
        file.to_string_lossy(),
        searched
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Default bound for collecting pipe output after the child was reaped.
/// Covers slow close plus descendants that inherited the pipes.
const DEFAULT_DRAIN_DEADLINE: Duration = Duration::from_secs(5);

/// How often the supervisor polls the child while waiting.
const POLL_INTERVAL: Duration = Duration::from_millis(2);

/// Program to spawn: direct path/argv0, or a cargo binary resolved eagerly.
#[derive(Debug, Clone)]
enum Program {
    Direct(OsString),
    /// `Err` holds the resolution failure; [`Command::run`] reports it as
    /// [`Termination::SpawnError`] instead of spawning.
    CargoBin {
        name: OsString,
        resolved: Result<PathBuf, String>,
    },
}

/// First-class piped child-process builder (backlog R01).
///
/// No shell is involved unless [`.shell(true)`](Command::shell) opts in.
/// Environment entries and the working directory apply to the child only.
#[derive(Debug, Clone)]
pub struct Command {
    program: Program,
    args: Vec<OsString>,
    env: Vec<(OsString, Option<OsString>)>,
    env_clear: bool,
    cwd: Option<PathBuf>,
    stdin_bytes: Option<Vec<u8>>,
    timeout: Option<Duration>,
    output_limit: Option<usize>,
    drain_deadline: Duration,
    shell: bool,
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

    /// Export the spawn configuration as a [`std::process::Command`]
    /// (program, args, env, cwd, shell mapping). Stdin bytes, timeout,
    /// output limits, and the drain deadline are [`Command::run`] behavior
    /// and are not represented in the returned value.
    pub fn std_command(&self) -> std::process::Command {
        let mut cmd = match &self.program {
            Program::Direct(p) => std::process::Command::new(p),
            Program::CargoBin { name, resolved } => std::process::Command::new(
                resolved
                    .as_ref()
                    .map(|p| p.as_os_str())
                    .unwrap_or(name.as_os_str()),
            ),
        };
        if self.shell {
            // Rebuild as /bin/sh -c <script> sh <args...>: the program is the
            // script, builder args become positional parameters.
            let script = match &self.program {
                Program::Direct(p) => p.clone(),
                Program::CargoBin { name, resolved } => resolved
                    .as_ref()
                    .map(|p| p.as_os_str().to_os_string())
                    .unwrap_or_else(|_| name.clone()),
            };
            cmd = std::process::Command::new("/bin/sh");
            cmd.arg("-c").arg(script).arg("sh");
        }
        cmd.args(&self.args);
        if self.env_clear {
            cmd.env_clear();
        }
        for (k, v) in &self.env {
            match v {
                Some(v) => {
                    cmd.env(k, v);
                }
                None => {
                    cmd.env_remove(k);
                }
            }
        }
        if let Some(cwd) = &self.cwd {
            cmd.current_dir(cwd);
        }
        cmd
    }

    /// Spawn the child, collect output deadlock-safely, and return the result.
    ///
    /// stdout/stderr drain on dedicated threads while stdin is written, so a
    /// child filling both pipes (or a large stdin while pipes fill) cannot
    /// deadlock against buffer limits. Infallible: even spawn failure is
    /// data ([`Termination::SpawnError`] with [`ProcessOutput::error`]).
    pub fn run(&self) -> ProcessOutput {
        let start = Instant::now();
        let mut out = ProcessOutput {
            stdout: Vec::new(),
            stderr: Vec::new(),
            status: Termination::SpawnError,
            truncated: false,
            elapsed: Duration::ZERO,
            error: None,
        };
        if let Program::CargoBin {
            resolved: Err(e), ..
        } = &self.program
        {
            out.error = Some(e.clone());
            out.elapsed = start.elapsed();
            return out;
        }
        let mut cmd = self.std_command();
        cmd.stdin(if self.stdin_bytes.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        // `kill_on_drop` is intentionally NOT used: the supervisor below owns
        // the full lifecycle (timeout/limit kill, reap, bounded drain).
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                out.error = Some(e.to_string());
                out.elapsed = start.elapsed();
                return out;
            }
        };

        let limit_hit = Arc::new(AtomicBool::new(false));
        let stdout_rx = spawn_drain(
            child.stdout.take().expect("piped stdout"),
            self.output_limit,
            Arc::clone(&limit_hit),
        );
        let stderr_rx = spawn_drain(
            child.stderr.take().expect("piped stderr"),
            self.output_limit,
            Arc::clone(&limit_hit),
        );
        if let Some(input) = self.stdin_bytes.clone() {
            let mut stdin = child.stdin.take().expect("piped stdin");
            std::thread::spawn(move || {
                // Broken pipe only means the child exited without reading
                // stdin; the child's termination status stays authoritative.
                let _ = stdin.write_all(&input);
            });
        }

        let deadline = self.timeout.map(|t| start + t);
        let status = loop {
            if limit_hit.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                out.truncated = true;
                break Termination::OutputLimit;
            }
            match child.try_wait() {
                Ok(Some(st)) => {
                    // A limit observed while the child exited concurrently
                    // still wins: bytes were dropped either way.
                    if limit_hit.load(Ordering::SeqCst) {
                        out.truncated = true;
                        break Termination::OutputLimit;
                    }
                    break classify(st);
                }
                Ok(None) => {}
                Err(e) => {
                    // try_wait failing after a live spawn should not happen;
                    // kill defensively and report what we know.
                    let _ = child.kill();
                    let _ = child.wait();
                    out.error = Some(format!("wait failed: {e}"));
                    break Termination::SpawnError;
                }
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                let _ = child.kill();
                let _ = child.wait();
                break Termination::Timeout;
            }
            std::thread::sleep(POLL_INTERVAL);
        };
        out.status = status;

        // Bounded late-output collection after the reap (R02): descendants
        // may still hold the pipes open, so each stream gets drain_deadline.
        let drain = self.drain_deadline;
        match stdout_rx.recv_timeout(drain) {
            Ok(bytes) => out.stdout = bytes,
            Err(_) => out.truncated = true,
        }
        match stderr_rx.recv_timeout(drain) {
            Ok(bytes) => out.stderr = bytes,
            Err(_) => out.truncated = true,
        }
        out.elapsed = start.elapsed();
        out
    }
}

impl From<&std::process::Command> for Command {
    fn from(cmd: &std::process::Command) -> Self {
        Command::from_std(cmd)
    }
}

#[cfg(unix)]
fn classify(status: ExitStatus) -> Termination {
    use std::os::unix::process::ExitStatusExt;
    if let Some(sig) = status.signal() {
        Termination::Signal(sig)
    } else {
        Termination::Exit(status.code().unwrap_or(-1))
    }
}

#[cfg(not(unix))]
fn classify(status: ExitStatus) -> Termination {
    // No signal reporting outside unix; do not invent one.
    Termination::Exit(status.code().unwrap_or(-1))
}

/// Drain one pipe on a thread; enforce the per-stream cap.
fn spawn_drain(
    mut pipe: impl Read + Send + 'static,
    limit: Option<usize>,
    limit_hit: Arc<AtomicBool>,
) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break, // EOF: all writers closed.
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(max) = limit {
                        if buf.len() > max {
                            buf.truncate(max);
                            limit_hit.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                }
                Err(_) => break, // Pipe error: return what we have.
            }
        }
        let _ = tx.send(buf);
    });
    rx
}

/// Dynamic-library search paths scrubbed from isolated children by default.
const DYLIB_VARS: &[&str] = &[
    "LD_LIBRARY_PATH",
    "DYLD_LIBRARY_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
];

/// Isolated process fixture: temp HOME/XDG/cwd plus child-only env (R03).
///
/// Created by [`isolated_env`]. Owns a `0700` temp root (removed on drop
/// unless [`IsolatedEnv::into_path`] keeps it) with `home/`, `work/` (cwd),
/// `tmp/` (TMPDIR), and XDG dirs. [`IsolatedEnv::apply`] points a [`Command`]
/// at them without touching the parent environment.
#[derive(Debug)]
pub struct IsolatedEnv {
    root: PathBuf,
    preserve_dylib_path: bool,
    keep: bool,
}

impl IsolatedEnv {
    /// Create the fixture; same as [`isolated_env`].
    pub fn new() -> std::io::Result<Self> {
        isolated_env()
    }

    /// Keep the parent's dynamic-library search paths in the child instead of
    /// scrubbing them. Default false (scrubbed). Opt in deliberately when the
    /// child under test cannot start without them (e.g. rustup shims or a
    /// toolchain libdir). macOS SIP still strips `DYLD_*` for system binaries
    /// regardless of this setting; that is platform behavior, not this API.
    pub fn preserve_dylib_path(mut self, preserve: bool) -> Self {
        self.preserve_dylib_path = preserve;
        self
    }

    /// Temp root owning all fixture dirs.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Temp HOME assigned to the child.
    #[must_use]
    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// Temp working directory assigned as the child's cwd.
    #[must_use]
    pub fn cwd(&self) -> PathBuf {
        self.root.join("work")
    }

    /// Temp TMPDIR assigned to the child.
    #[must_use]
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// Env entries [`IsolatedEnv::apply`] sets on the child.
    #[must_use]
    pub fn envs(&self) -> Vec<(OsString, OsString)> {
        let home = self.home();
        vec![
            (OsString::from("HOME"), home.as_os_str().to_os_string()),
            (
                OsString::from("XDG_CONFIG_HOME"),
                home.join(".config").as_os_str().to_os_string(),
            ),
            (
                OsString::from("XDG_CACHE_HOME"),
                home.join(".cache").as_os_str().to_os_string(),
            ),
            (
                OsString::from("XDG_DATA_HOME"),
                home.join(".local").join("share").as_os_str().to_os_string(),
            ),
            (
                OsString::from("TMPDIR"),
                self.tmp().as_os_str().to_os_string(),
            ),
        ]
    }

    /// Point `cmd` at this fixture: temp HOME/XDG/TMPDIR, cwd, and — unless
    /// [`.preserve_dylib_path(true)`](IsolatedEnv::preserve_dylib_path) —
    /// scrubbed dynamic-library search paths. Parent env untouched.
    #[must_use]
    pub fn apply(&self, mut cmd: Command) -> Command {
        for (k, v) in self.envs() {
            cmd = cmd.env(k, v);
        }
        cmd = cmd.current_dir(self.cwd());
        if !self.preserve_dylib_path {
            for var in DYLIB_VARS {
                cmd = cmd.env_remove(var);
            }
        }
        cmd
    }

    /// Keep the temp root on drop; returns it for inspection.
    #[must_use]
    pub fn into_path(mut self) -> PathBuf {
        self.keep = true;
        self.root.clone()
    }
}

impl Drop for IsolatedEnv {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// Create an [`IsolatedEnv`] fixture: a unique `0700` temp root with `home/`,
/// `work/`, `tmp/`, and the XDG dirs pre-created. Std-only unique naming
/// (pid + nanos + counter); retries on collision.
pub fn isolated_env() -> std::io::Result<IsolatedEnv> {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let base = std::env::temp_dir();
    let pid = std::process::id();
    let mut last_err = None;
    for _ in 0..100 {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = base.join(format!("tuisnap-env-{pid}-{nanos}-{n}"));
        match std::fs::create_dir(&root) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700));
                }
                let env = IsolatedEnv {
                    root,
                    preserve_dylib_path: false,
                    keep: false,
                };
                for d in [
                    env.home(),
                    env.home().join(".config"),
                    env.home().join(".cache"),
                    env.home().join(".local").join("share"),
                    env.cwd(),
                    env.tmp(),
                ] {
                    std::fs::create_dir_all(&d)?;
                }
                return Ok(env);
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::AlreadyExists, "temp dir collision")
    }))
}
