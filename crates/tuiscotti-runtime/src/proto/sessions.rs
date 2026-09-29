use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Named sessions (A02): versioned endpoints, owner-only runtime dir
// ---------------------------------------------------------------------------

/// Endpoint file format version. A reader that sees another version refuses
/// the file instead of guessing.
pub const SESSION_ENDPOINT_VERSION: u32 = 1;

/// Backend that owns the named session's child.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionBackend {
    /// Plain piped child (this version). PTY-backed named sessions arrive
    /// with the daemon transport; the enum reserves the shape.
    Process,
    Pty,
}

/// Liveness of a named session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStatus {
    Running,
    Exited,
}

/// What `session list` reports per session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInfo {
    pub name: String,
    pub pid: u32,
    pub argv: Vec<String>,
    pub backend: SessionBackend,
    pub status: SessionStatus,
    pub started_unix: u64,
}

/// On-disk endpoint record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SessionEndpoint {
    pub(crate) version: u32,
    pub(crate) name: String,
    pub(crate) pid: u32,
    pub(crate) argv: Vec<String>,
    pub(crate) backend: SessionBackend,
    pub(crate) started_unix: u64,
    /// Owner uid when known (Unix with `pty` feature, via `id -u`).
    pub(crate) owner: Option<u32>,
}

/// Runtime dir: `$TUISNAP_RUNTIME_DIR`, else `$XDG_RUNTIME_DIR/tuisnap`, else a
/// per-uid temp dir. Created owner-only (0o700) on Unix.
static RUNTIME_DIR_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Test-only runtime-dir override (no `unsafe`, unlike `set_var`, which is an
/// `unsafe fn` in edition 2024 and cannot be used under the workspace lints).
/// Checked before `$TUISNAP_RUNTIME_DIR` by [`runtime_dir`]. Callers sharing a
/// process must serialize (see the CLI tests' `ENV_LOCK`); pass `None` to
/// clear. Never set in production code.
pub fn set_runtime_dir_override(dir: Option<PathBuf>) {
    *RUNTIME_DIR_OVERRIDE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = dir;
}

pub fn runtime_dir() -> Result<PathBuf, OpError> {
    if let Some(d) = RUNTIME_DIR_OVERRIDE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        return ensure_runtime_dir(&d);
    }
    let dir = if let Ok(d) = std::env::var("TUISNAP_RUNTIME_DIR") {
        PathBuf::from(d)
    } else if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(d).join("tuisnap")
    } else {
        std::env::temp_dir().join(format!("tuisnap-{}", current_uid()))
    };
    ensure_runtime_dir(&dir)
}

fn ensure_runtime_dir(dir: &Path) -> Result<PathBuf, OpError> {
    std::fs::create_dir_all(dir)
        .map_err(|e| OpError::new("io", format!("runtime dir {}: {e}", dir.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&dir)
            .map_err(|e| OpError::new("io", format!("stat {}: {e}", dir.display())))?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o700 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| OpError::new("io", format!("chmod 700 {}: {e}", dir.display())))?;
        }
    }
    Ok(dir.to_path_buf())
}

pub(crate) fn current_uid() -> u32 {
    #[cfg(all(unix, feature = "pty"))]
    {
        // No libc: `id -u` with a `$UID` fallback (the workspace forbids
        // `unsafe`).
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u32>().ok())
            .or_else(|| {
                std::env::var("UID")
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok())
            })
            .unwrap_or(0)
    }
    #[cfg(not(all(unix, feature = "pty")))]
    {
        0
    }
}

pub(crate) fn validate_session_name(name: &str) -> Result<(), OpError> {
    if name.is_empty() || name.len() > 64 {
        return Err(OpError::new(
            "invalid-input",
            "session name must be 1..=64 chars",
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        || name == "."
        || name == ".."
    {
        return Err(OpError::new(
            "invalid-input",
            "session name allows only [A-Za-z0-9_.-] and must not be . or ..",
        ));
    }
    Ok(())
}

pub(crate) fn endpoint_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.json"))
}

pub(crate) fn read_endpoint(dir: &Path, name: &str) -> Result<Option<SessionEndpoint>, OpError> {
    let path = endpoint_path(dir, name);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(OpError::new("io", format!("read {}: {e}", path.display()))),
    };
    let ep: SessionEndpoint = serde_json::from_slice(&bytes).map_err(|e| {
        OpError::new(
            "invalid-input",
            format!("{} is corrupt: {e}", path.display()),
        )
    })?;
    if ep.version != SESSION_ENDPOINT_VERSION {
        return Err(OpError::new(
            "version-mismatch",
            format!(
                "{}: endpoint v{} vs reader v{SESSION_ENDPOINT_VERSION}",
                path.display(),
                ep.version
            ),
        ));
    }
    if let Some(owner) = ep.owner {
        let me = current_uid();
        if owner != me {
            return Err(OpError::new(
                "owner-mismatch",
                format!("{name} belongs to uid {owner}, not {me}"),
            ));
        }
    }
    Ok(Some(ep))
}

/// Atomic endpoint write (tmp file + rename).
pub(crate) fn write_endpoint(dir: &Path, ep: &SessionEndpoint) -> Result<(), OpError> {
    let path = endpoint_path(dir, &ep.name);
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(ep)
        .map_err(|e| OpError::new("io", format!("encode {}: {e}", path.display())))?;
    std::fs::write(&tmp, &bytes)
        .map_err(|e| OpError::new("io", format!("write {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| OpError::new("io", format!("publish {}: {e}", path.display())))?;
    Ok(())
}

pub(crate) fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // Exit code of `kill -0`: portable, no extra deps.
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

pub(crate) fn kill_pid(pid: u32) -> Result<(), OpError> {
    #[cfg(all(unix, feature = "pty"))]
    {
        // No libc: `kill -TERM` (the workspace forbids `unsafe`). An
        // already-dead pid still succeeds, matching the old NotFound branch.
        let delivered = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !delivered && pid_alive(pid) {
            return Err(OpError::new("io", format!("SIGTERM {pid} failed")));
        }
        Ok(())
    }
    #[cfg(not(all(unix, feature = "pty")))]
    {
        #[cfg(unix)]
        {
            let st = std::process::Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status()
                .map_err(|e| OpError::new("io", format!("kill {pid}: {e}")))?;
            if st.success() {
                Ok(())
            } else {
                Err(OpError::new("io", format!("kill -TERM {pid} failed")))
            }
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
            Err(OpError::new("unsupported", "session stop needs Unix"))
        }
    }
}

pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
