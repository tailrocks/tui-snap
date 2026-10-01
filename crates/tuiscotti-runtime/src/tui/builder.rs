//! [`Tui`] session builder: program, env, size, profile, spawn.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::atomic::AtomicBool;
#[cfg(unix)]
use std::sync::{Arc, Mutex};

use super::error::TuiError;
use super::limits::{MAX_COLS, MAX_ROWS, MIN_COLS, MIN_ROWS};
use super::profile::TerminalProfile;
use super::session::Session;
#[cfg(unix)]
use super::shared::Shared;
#[cfg(unix)]
use super::spawn::{StartedThreads, spawn_pty_child, start_session_threads};

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
    /// Ordered child-env ops: `Some` sets, `None` removes (LIFE-9, mirroring
    /// the piped `Command` surface).
    env: Vec<(OsString, Option<OsString>)>,
    env_clear: bool,
    cwd: Option<PathBuf>,
    profile: TerminalProfile,
}

impl std::fmt::Debug for Tui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted_env: Vec<(OsString, &str)> = self
            .env
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    if v.is_some() {
                        "<redacted>"
                    } else {
                        "<removed>"
                    },
                )
            })
            .collect();
        f.debug_struct("Tui")
            .field("program", &self.program)
            .field("extra_args", &self.extra_args)
            .field("size", &self.size)
            .field("env", &redacted_env)
            .field("env_clear", &self.env_clear)
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
            env_clear: false,
            cwd: None,
            profile: TerminalProfile::default(),
        }
    }

    /// Launch a cargo-built binary of this package by name. Resolved eagerly
    /// through the canonical [`crate::command::cargo_bin_path`] lookup.
    /// Resolution failure is an error here (not deferred to [`Tui::spawn`]),
    /// listing every location tried.
    ///
    /// # Errors
    ///
    /// Returns `TuiError::Spawn` if the binary cannot be resolved.
    pub fn cargo_bin(name: impl AsRef<OsStr>) -> Result<Self, TuiError> {
        let name = name.as_ref().to_os_string();
        resolve_cargo_bin(&name)?;
        Ok(Self {
            program: Program::CargoBin(name),
            extra_args: Vec::new(),
            size: (80, 24),
            env: Vec::new(),
            env_clear: false,
            cwd: None,
            profile: TerminalProfile::default(),
        })
    }

    /// Append one child argument.
    #[must_use]
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.extra_args.push(arg.as_ref().to_os_string());
        self
    }

    /// Append child arguments.
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

    /// Initial PTY/emulator size. Backend limits: 1..=1000 columns,
    /// 1..=1000 rows.
    #[must_use]
    pub fn size(mut self, cols: u16, rows: u16) -> Self {
        self.size = (cols, rows);
        self
    }

    /// Child-only environment entry. Never touches the parent environment.
    #[must_use]
    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.env.push((
            key.as_ref().to_os_string(),
            Some(value.as_ref().to_os_string()),
        ));
        self
    }

    /// Child-only environment entries. Never touches the parent environment.
    #[must_use]
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
    #[must_use]
    pub fn env_remove(mut self, key: impl AsRef<OsStr>) -> Self {
        self.env.push((key.as_ref().to_os_string(), None));
        self
    }

    /// Start the child with an empty environment (then apply `.env(...)`).
    /// Unlike the default path, no implicit `TERM` is added: a cleared
    /// environment carries exactly what the caller sets.
    #[must_use]
    pub fn env_clear(mut self, clear: bool) -> Self {
        self.env_clear = clear;
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
    /// Every failure after the child exists rolls back (kill + reap) so a
    /// failed spawn never orphans a live child (LIFE-4).
    ///
    /// # Errors
    ///
    /// Returns `TuiError` for bad sizes, rejected profiles, or spawn failures.
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
        // The backend does not fail a bad cwd (the child would silently run
        // in the parent directory), so validate up front: a session that
        // cannot start in its requested directory must not start at all.
        if let Some(cwd) = &self.cwd
            && !cwd.is_dir()
        {
            return Err(TuiError::Spawn(format!(
                "cwd {} is not a usable directory",
                cwd.display()
            )));
        }

        #[cfg(not(unix))]
        {
            let _ = (&argv, cols, rows);
            return Err(TuiError::Unsupported(
                "pty sessions require a Unix platform",
            ));
        }

        #[cfg(unix)]
        {
            let params = self.prepare_command(&argv);
            let spawned = spawn_pty_child(&params, cols, rows)?;
            let pid = spawned.pid;
            let shared = Arc::new(Shared::new());
            let StartedThreads {
                op_tx,
                ctl_tx,
                worker,
                reader_thread,
                writer_thread,
            } = start_session_threads(spawned, cols, rows, Arc::clone(&shared))?;
            let session = Session {
                op_tx: Mutex::new(Some(op_tx)),
                ctl_tx: Mutex::new(Some(ctl_tx)),
                shared,
                worker: Mutex::new(Some(worker)),
                reader: Mutex::new(Some(reader_thread)),
                writer: Mutex::new(Some(writer_thread)),
                closed: AtomicBool::new(false),
                pid,
            };
            // The session is usable only once the worker published revision 0.
            session.await_initial()?;
            Ok(session)
        }
    }

    fn resolve_argv(&self) -> Result<Vec<OsString>, TuiError> {
        match &self.program {
            Program::Argv(argv) => Ok(argv.clone()),
            Program::CargoBin(name) => Ok(vec![resolve_cargo_bin(name)?]),
        }
    }

    /// Build the child spawn params: program args, child-only env (with a
    /// default `TERM` unless overridden or the environment was cleared),
    /// and the child cwd.
    ///
    /// The builder records env ops in order, but the backend applies
    /// clear-then-removals-then-overrides; folding keeps last-op-per-key
    /// (an environment is a map, so only the last op per key is
    /// observable) and the outcome is identical either way.
    #[cfg(unix)]
    fn prepare_command(&self, argv: &[OsString]) -> termpane::process::SpawnParams {
        use std::collections::HashMap;
        let mut params = termpane::process::SpawnParams::new(argv[0].as_os_str());
        params = params.args(argv.iter().skip(1).chain(self.extra_args.iter()));
        if self.env_clear {
            params = params.env_clear();
        }
        let mut folded: HashMap<&OsStr, Option<&OsStr>> = HashMap::new();
        for (k, v) in &self.env {
            folded.insert(k.as_os_str(), v.as_deref());
        }
        // `HashMap` iteration order is unspecified, but entries are keyed
        // and each key appears once, so the resulting environment is
        // deterministic regardless of order.
        let mut term_set = false;
        for (k, v) in &folded {
            match v {
                Some(value) => {
                    if *k == OsStr::new("TERM") {
                        term_set = true;
                    }
                    params = params.env(k, value);
                }
                None => params = params.env_remove(k),
            }
        }
        if !term_set && !self.env_clear {
            params = params.env("TERM", self.profile.term.clone());
        }
        if let Some(cwd) = &self.cwd {
            params = params.current_dir(cwd);
        }
        params
    }
}

/// Resolve a cargo-built binary through the canonical
/// [`crate::command::cargo_bin_path`] lookup (env exact, env normalized,
/// next-to-exe, deps-parent, cwd `target/debug`/`target/release`). Shared by
/// eager [`Tui::cargo_bin`] and [`Tui::spawn`] so the two can never disagree
/// on lookup order.
fn resolve_cargo_bin(name: &OsStr) -> Result<OsString, TuiError> {
    crate::command::cargo_bin_path(name)
        .map(PathBuf::into_os_string)
        .map_err(|e| TuiError::Spawn(e.to_string()))
}

/// [`resolve_cargo_bin`] over an injected environment (pure form for tests).
#[cfg(test)]
pub(crate) fn resolve_cargo_bin_with_map(
    name: &OsStr,
    env: &std::collections::HashMap<String, String>,
) -> Result<OsString, TuiError> {
    crate::command::cargo_bin_path_with_map(name, env)
        .map(PathBuf::into_os_string)
        .map_err(|e| TuiError::Spawn(e.to_string()))
}
