//! [`Tui`] session builder: program, env, size, profile, spawn.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, mpsc};

use alacritty_terminal::term::Config as TermConfig;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use super::capture::run_reader;
use super::error::TuiError;
use super::limits::{MAX_COLS, MAX_ROWS, MIN_COLS, MIN_ROWS, PTY_LIFECYCLE};
use super::session::Session;
use super::shared::Shared;
use super::worker::{Op, run_worker};

/// Advertised terminal/protocol behavior for a session.
///
/// `spawn()` rejects any profile claiming a capability the backend cannot
/// implement — capabilities are never silently normalized away.
#[derive(Debug, Clone)]
pub struct TerminalProfile {
    /// `TERM` value exported to the child (child-only).
    pub term: String,
    /// Application may use SGR mouse (1006). Backend tracks it.
    pub mouse_sgr: bool,
    /// Application may use UTF-8 mouse (1005). Backend tracks it.
    pub mouse_utf8: bool,
    /// Application may use legacy X10 mouse (1000/1002/1003). Tracked.
    pub mouse_legacy: bool,
    /// Parse kitty progressive-enhancement flags (enables them in the
    /// emulator config so applications can negotiate them).
    pub kitty_keyboard: bool,
    /// Application may use bracketed paste (2004). Tracked.
    pub bracketed_paste: bool,
    /// Application may use focus tracking (1004). Tracked.
    pub focus_tracking: bool,
    /// Application may use the alternate screen (1049). Tracked.
    pub alt_screen: bool,
    /// Synchronized output (DEC 2026). **Backend lacks it**: `spawn()`
    /// fails when this is true, and `wait_frame` is unsupported.
    pub synchronized_output: bool,
    /// Per-cell blink (SGR 5/6). **Backend drops it**: `spawn()` fails
    /// when this is true rather than passing on a lossy grid.
    pub cell_blink: bool,
}

impl Default for TerminalProfile {
    fn default() -> Self {
        Self {
            term: "xterm-256color".to_string(),
            mouse_sgr: true,
            mouse_utf8: true,
            mouse_legacy: true,
            kitty_keyboard: true,
            bracketed_paste: true,
            focus_tracking: true,
            alt_screen: true,
            synchronized_output: false,
            cell_blink: false,
        }
    }
}

impl TerminalProfile {
    /// Reject profiles advertising what the backend lacks.
    fn check(&self) -> Result<(), TuiError> {
        if self.synchronized_output {
            return Err(TuiError::Unsupported(
                "profile advertises synchronized-output (DEC 2026): backend cannot track it",
            ));
        }
        if self.cell_blink {
            return Err(TuiError::Unsupported(
                "profile advertises per-cell blink: backend drops SGR 5/6",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
enum Program {
    Argv(Vec<OsString>),
    CargoBin(OsString),
}

/// PTY session builder: one program plus args, child-only env/cwd. The parent
/// process environment and working directory are never mutated.
///
/// `Debug` is secret-safe: env values are redacted, so tokens passed to the
/// child never leak into logs or snapshots.
pub struct Tui {
    program: Program,
    extra_args: Vec<OsString>,
    size: (u16, u16),
    env: Vec<(OsString, OsString)>,
    cwd: Option<PathBuf>,
    profile: TerminalProfile,
}

impl std::fmt::Debug for Tui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted_env: Vec<(OsString, &str)> = self
            .env
            .iter()
            .map(|(k, _)| (k.clone(), "<redacted>"))
            .collect();
        f.debug_struct("Tui")
            .field("program", &self.program)
            .field("extra_args", &self.extra_args)
            .field("size", &self.size)
            .field("env", &redacted_env)
            .field("cwd", &self.cwd)
            .field("profile", &self.profile)
            .finish()
    }
}

impl Tui {
    /// Launch `argv[0]` with `argv[1..]` as arguments. Native `OsStr`
    /// arguments: non-UTF-8 argv passes through byte-exact.
    pub fn new<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Self {
            program: Program::Argv(
                argv.into_iter()
                    .map(|a| a.as_ref().to_os_string())
                    .collect(),
            ),
            extra_args: Vec::new(),
            size: (80, 24),
            env: Vec::new(),
            cwd: None,
            profile: TerminalProfile::default(),
        }
    }

    /// Launch a cargo-built binary of this package by name. Resolved eagerly
    /// through the canonical [`crate::command::cargo_bin_path`] lookup.
    /// Resolution failure is an error here (not deferred to [`Tui::spawn`]),
    /// listing every location tried.
    pub fn cargo_bin(name: impl AsRef<OsStr>) -> Result<Self, TuiError> {
        let name = name.as_ref().to_os_string();
        resolve_cargo_bin(&name)?;
        Ok(Self {
            program: Program::CargoBin(name),
            extra_args: Vec::new(),
            size: (80, 24),
            env: Vec::new(),
            cwd: None,
            profile: TerminalProfile::default(),
        })
    }

    #[must_use]
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.extra_args.push(arg.as_ref().to_os_string());
        self
    }

    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.extra_args
            .extend(args.into_iter().map(|a| a.as_ref().to_os_string()));
        self
    }

    /// Initial PTY/emulator size. Backend limits: 2..=1000 columns,
    /// 1..=1000 rows (static 1x1 screens stay valid outside the PTY path).
    #[must_use]
    pub fn size(mut self, cols: u16, rows: u16) -> Self {
        self.size = (cols, rows);
        self
    }

    /// Child-only environment entry. Never touches the parent environment.
    #[must_use]
    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.env
            .push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }

    /// Child working directory. Takes [`PathBuf`] (not a generic) so
    /// `".into()"` call sites keep inferring without annotations.
    #[must_use]
    pub fn cwd(mut self, dir: PathBuf) -> Self {
        self.cwd = Some(dir);
        self
    }

    /// Advertised terminal behavior. Profiles claiming backend-unsupported
    /// capabilities are rejected by `spawn()`.
    #[must_use]
    pub fn profile(mut self, profile: TerminalProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Spawn the child in a new PTY and start the session threads.
    pub fn spawn(self) -> Result<Session, TuiError> {
        self.profile.check()?;
        let (cols, rows) = self.size;
        if !(MIN_COLS..=MAX_COLS).contains(&cols) {
            return Err(TuiError::Spawn(format!(
                "cols {cols} outside backend range {MIN_COLS}..={MAX_COLS}"
            )));
        }
        if !(MIN_ROWS..=MAX_ROWS).contains(&rows) {
            return Err(TuiError::Spawn(format!(
                "rows {rows} outside backend range {MIN_ROWS}..={MAX_ROWS}"
            )));
        }
        let argv = self.resolve_argv()?;
        if argv.is_empty() {
            return Err(TuiError::Spawn("empty argv".to_string()));
        }

        let cmd = self.prepare_command(&argv);
        let spawned = spawn_pty_child(cmd, cols, rows)?;
        let reader = spawned.reader;
        let writer = spawned.writer;
        let child = spawned.child;
        let pid = spawned.pid;
        let master = spawned.master;

        let (op_tx, op_rx) = mpsc::channel::<Op>();
        let shared = Arc::new(Shared::new());
        let term_config = TermConfig {
            kitty_keyboard: self.profile.kitty_keyboard,
            ..TermConfig::default()
        };

        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("tuisnap-tui-worker".to_string())
            .spawn(move || {
                run_worker(
                    master,
                    child,
                    writer,
                    term_config,
                    cols,
                    rows,
                    pid,
                    op_rx,
                    worker_shared,
                );
            })
            .map_err(|e| TuiError::Spawn(format!("worker spawn failed: {e}")))?;

        let feed_tx = op_tx.clone();
        let reader_thread = std::thread::Builder::new()
            .name("tuisnap-tui-reader".to_string())
            .spawn(move || run_reader(reader, feed_tx))
            .map_err(|e| TuiError::Spawn(format!("reader spawn failed: {e}")))?;

        let session = Session {
            op_tx: Some(op_tx),
            shared,
            worker: Mutex::new(Some(worker)),
            reader: Mutex::new(Some(reader_thread)),
            closed: AtomicBool::new(false),
            pid,
        };
        // The session is usable only once the worker published revision 0.
        session.await_initial()?;
        Ok(session)
    }

    fn resolve_argv(&self) -> Result<Vec<OsString>, TuiError> {
        match &self.program {
            Program::Argv(argv) => Ok(argv.clone()),
            Program::CargoBin(name) => Ok(vec![resolve_cargo_bin(name)?]),
        }
    }

    /// Build the child command: program args, child-only env (with a
    /// default `TERM` unless overridden), and the child cwd.
    fn prepare_command(&self, argv: &[OsString]) -> CommandBuilder {
        let mut cmd = CommandBuilder::new(&argv[0]);
        for a in argv.iter().skip(1).chain(self.extra_args.iter()) {
            cmd.arg(a);
        }
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        if cmd.get_env("TERM").is_none() {
            cmd.env("TERM", self.profile.term.clone());
        }
        if let Some(cwd) = &self.cwd {
            cmd.cwd(cwd);
        }
        cmd
    }
}

/// An opened PTY pair with the child spawned and I/O handles taken.
struct SpawnedPty {
    master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    reader: Box<dyn std::io::Read + Send>,
    writer: Box<dyn std::io::Write + Send>,
    pid: Option<u32>,
}

/// Open the PTY, spawn the child, and take I/O handles — all under the
/// process-global lifecycle guard, released before the threads start.
fn spawn_pty_child(cmd: CommandBuilder, cols: u16, rows: u16) -> Result<SpawnedPty, TuiError> {
    let _guard = PTY_LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| TuiError::Spawn(format!("openpty failed: {e:#}")))?;
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| TuiError::Spawn(format!("spawn failed: {e:#}")))?;
    // Drain discipline: take I/O handles before any wait can run.
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| TuiError::Spawn(format!("pty reader failed: {e:#}")))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| TuiError::Spawn(format!("pty writer failed: {e:#}")))?;
    child
        .try_wait()
        .map_err(|e| TuiError::Spawn(format!("child poll failed: {e:#}")))?;
    let pid = child.process_id();
    drop(_guard);
    Ok(SpawnedPty {
        master: pair.master,
        child,
        reader,
        writer,
        pid,
    })
}

/// Resolve a cargo-built binary through the canonical
/// [`crate::command::cargo_bin_path`] lookup (env exact, env normalized,
/// next-to-exe, deps-parent, cwd `target/debug`/`target/release`). Shared by
/// eager [`Tui::cargo_bin`] and [`Tui::spawn`] so the two can never disagree
/// on lookup order.
fn resolve_cargo_bin(name: &OsStr) -> Result<OsString, TuiError> {
    crate::command::cargo_bin_path(name)
        .map(|p| p.into_os_string())
        .map_err(|e| TuiError::Spawn(e.to_string()))
}

/// [`resolve_cargo_bin`] over an injected environment (pure form for tests).
#[cfg(test)]
pub(crate) fn resolve_cargo_bin_with_map(
    name: &OsStr,
    env: &std::collections::HashMap<String, String>,
) -> Result<OsString, TuiError> {
    crate::command::cargo_bin_path_with_map(name, env)
        .map(|p| p.into_os_string())
        .map_err(|e| TuiError::Spawn(e.to_string()))
}
