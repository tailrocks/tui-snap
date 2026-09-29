use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use super::*;


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
