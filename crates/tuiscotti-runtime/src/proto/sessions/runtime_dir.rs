//! Runtime dir resolution + Unix identity probe.
//!
//! Split out of `sessions.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
#[cfg(unix)]
use std::sync::atomic::Ordering;

use super::super::OpError;

/// Runtime dir: `$TUISCOTTI_RUNTIME_DIR`, else `$XDG_RUNTIME_DIR/tuiscotti`, else a
/// per-uid temp dir. Created owner-only (0o700) on Unix.
#[cfg(any(test, feature = "test-overrides"))]
static RUNTIME_DIR_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Test-only runtime-dir override (F12: a truly test-only mechanism — this
/// function exists only under `cfg(test)` or the `test-overrides` feature,
/// so production builds cannot call it). No `unsafe`, unlike `set_var`,
/// which is an `unsafe fn` in edition 2024 and cannot be used under the
/// workspace lints. Checked before `$TUISCOTTI_RUNTIME_DIR` by
/// [`runtime_dir`]. Callers sharing a process must serialize (see the CLI
/// tests' `ENV_LOCK`); pass `None` to clear. Tests that can run
/// concurrently should spawn subprocesses with `TUISCOTTI_RUNTIME_DIR` in
/// the child environment instead (explicit per-process context).
#[cfg(any(test, feature = "test-overrides"))]
pub fn set_runtime_dir_override(dir: Option<PathBuf>) {
    *RUNTIME_DIR_OVERRIDE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = dir;
}

/// Resolve the runtime dir, creating it owner-only (0o700) on Unix.
///
/// # Errors
///
/// Returns [`OpError`] when the dir cannot be created or secured.
pub fn runtime_dir() -> Result<PathBuf, OpError> {
    #[cfg(any(test, feature = "test-overrides"))]
    if let Some(d) = RUNTIME_DIR_OVERRIDE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    {
        return ensure_runtime_dir(&d);
    }
    if let Ok(d) = std::env::var("TUISCOTTI_RUNTIME_DIR") {
        return ensure_runtime_dir(&PathBuf::from(d));
    }
    if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        return ensure_runtime_dir(&PathBuf::from(d).join("tuiscotti"));
    }
    ensure_runtime_dir(&std::env::temp_dir().join(format!("tuiscotti-{}", current_uid()?)))
}

/// Create the runtime dir and validate it before use: real directory (never a
/// symlink), owned by us, owner-only permissions. Never repairs or follows a
/// directory owned by someone else.
fn ensure_runtime_dir(dir: &Path) -> Result<PathBuf, OpError> {
    if dir.as_os_str().is_empty() {
        return Err(OpError::new(
            "invalid-input",
            "runtime dir must not be empty",
        ));
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| OpError::new("io", format!("runtime dir {}: {e}", dir.display())))?;
    let meta = std::fs::symlink_metadata(dir)
        .map_err(|e| OpError::new("io", format!("stat {}: {e}", dir.display())))?;
    if meta.file_type().is_symlink() {
        return Err(OpError::new(
            "invalid-input",
            format!("runtime dir {} must not be a symlink", dir.display()),
        ));
    }
    if !meta.file_type().is_dir() {
        return Err(OpError::new(
            "io",
            format!("runtime dir {} is not a directory", dir.display()),
        ));
    }
    #[cfg(unix)]
    secure_runtime_dir(dir, &meta)?;
    Ok(dir.to_path_buf())
}

/// Unix ownership/permission boundary for an existing runtime dir: the dir
/// must already be ours (no chmod of someone else's directory), then it is
/// tightened to 0o700 when needed.
#[cfg(unix)]
fn secure_runtime_dir(dir: &Path, meta: &std::fs::Metadata) -> Result<(), OpError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let me = current_uid()?;
    if meta.uid() != me {
        return Err(OpError::new(
            "owner-mismatch",
            format!(
                "runtime dir {} belongs to uid {}, not {me}",
                dir.display(),
                meta.uid()
            ),
        ));
    }
    if meta.permissions().mode() & 0o777 != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| OpError::new("io", format!("chmod 700 {}: {e}", dir.display())))?;
    }
    Ok(())
}

/// Counter disambiguating our temp-file probes within this process.
static UID_PROBE_CTR: AtomicU64 = AtomicU64::new(0);

/// Our own uid without `PATH` lookups, environment fallbacks, or a zero
/// default: a freshly created file is necessarily owned by us, so its owner
/// uid (via safe `MetadataExt`, no `unsafe`) is trusted OS identity. Final
/// wiring is `termpane::process::current_uid` once termpane is released.
#[cfg(unix)]
pub(crate) fn current_uid() -> Result<u32, OpError> {
    use std::os::unix::fs::MetadataExt;
    let tmp = std::env::temp_dir();
    for _ in 0..32 {
        let n = UID_PROBE_CTR.fetch_add(1, Ordering::Relaxed);
        let probe = tmp.join(format!(".tuiscotti-uid-{}-{n}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
        {
            Ok(f) => {
                let uid = f
                    .metadata()
                    .map_err(|e| OpError::new("io", format!("stat uid probe: {e}")))?
                    .uid();
                drop(f);
                if std::fs::remove_file(&probe).is_err() {
                    // Best-effort cleanup of our own 0-byte probe.
                }
                return Ok(uid);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(OpError::new("io", format!("uid probe: {e}")));
            }
        }
    }
    Err(OpError::new("io", "uid probe: no unique temp name"))
}

/// Non-Unix builds have no trusted uid probe; failing closed beats a zero.
#[cfg(not(unix))]
pub(crate) fn current_uid() -> Result<u32, OpError> {
    Err(OpError::new("unsupported", "uid identity needs Unix"))
}

#[cfg(test)]
mod tests {
    use super::super::{cleanup, test_dir};
    use super::*;

    #[test]
    fn runtime_dir_fresh_is_usable() {
        let dir = test_dir("fresh");
        let back = ensure_runtime_dir(&dir.join("rt")).expect("ensure");
        assert!(back.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&back).expect("stat").permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        cleanup(&dir);
    }

    #[test]
    fn runtime_dir_rejects_file_in_place() {
        let dir = test_dir("file");
        let file = dir.join("rt");
        std::fs::write(&file, b"x").expect("seed");
        assert!(ensure_runtime_dir(&file).is_err());
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_dir_rejects_symlink() {
        let dir = test_dir("symlink");
        let real = dir.join("real");
        std::fs::create_dir(&real).expect("real");
        let link = dir.join("rt");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let e = ensure_runtime_dir(&link).expect_err("symlink accepted");
        assert_eq!(e.code, "invalid-input");
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_dir_repairs_loose_perms() {
        use std::os::unix::fs::PermissionsExt;
        let dir = test_dir("perms");
        let rt = dir.join("rt");
        std::fs::create_dir(&rt).expect("mkdir");
        std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        ensure_runtime_dir(&rt).expect("ensure");
        let mode = std::fs::metadata(&rt).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn current_uid_stable_no_zero_fallback() {
        let a = current_uid().expect("uid");
        let b = current_uid().expect("uid");
        assert_eq!(a, b);
    }
}
