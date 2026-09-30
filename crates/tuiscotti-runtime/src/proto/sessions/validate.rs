//! Name/pid validation + containment-checked path joins.
//!
//! Split out of `sessions.rs` so each file stays under the repo line gate;
//! behavior is unchanged.

use std::path::{Path, PathBuf};

use super::super::OpError;
use super::types::PID_MAX;

pub(crate) fn validate_session_name(name: &str) -> Result<(), OpError> {
    if name.is_empty() || name.len() > 64 {
        return Err(OpError::new(
            "invalid-input",
            "session name must be 1..=64 chars",
        ));
    }
    // `daemon` is reserved for the retained-session daemon's own files
    // (`daemon.lock` single-flight, `daemon.pid`, `daemon.sock`): a session
    // by that name would collide with them (`daemon.lock` doubles as its
    // start reservation).
    if name == "daemon" {
        return Err(OpError::new(
            "invalid-input",
            "session name \"daemon\" is reserved",
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

/// Reject PID 0 (scheduler/idle, and group-selection in `kill`) and values
/// outside the `pid_t` range before any liveness or signal operation.
pub(crate) fn validate_pid(pid: u32) -> Result<(), OpError> {
    if pid == 0 {
        return Err(OpError::new(
            "invalid-input",
            "pid 0 is never a session child",
        ));
    }
    if pid > PID_MAX {
        return Err(OpError::new(
            "invalid-input",
            format!("pid {pid} is outside the pid_t range"),
        ));
    }
    Ok(())
}

/// Join a validated session name to the runtime dir, proving containment:
/// the result's parent is exactly `dir`, so prune/delete paths can never be
/// steered outside the registry by an untrusted payload name.
pub(super) fn checked_join(dir: &Path, file: &str) -> Result<PathBuf, OpError> {
    let path = dir.join(file);
    if path.parent() != Some(dir) {
        return Err(OpError::new(
            "invalid-input",
            format!("{} escapes the runtime dir", path.display()),
        ));
    }
    Ok(path)
}

pub(crate) fn checked_endpoint_path(dir: &Path, name: &str) -> Result<PathBuf, OpError> {
    validate_session_name(name)?;
    checked_join(dir, &format!("{name}.json"))
}

pub(crate) fn checked_aux_path(dir: &Path, name: &str, suffix: &str) -> Result<PathBuf, OpError> {
    validate_session_name(name)?;
    checked_join(dir, &format!("{name}.{suffix}"))
}

/// Containment-checked daemon file path (`daemon.{suffix}` for the fixed
/// `sock`/`pid`/`err`/`lock` literals). Bypasses [`validate_session_name`],
/// which reserves `daemon` for exactly these files; containment is still
/// proven by [`checked_join`].
#[cfg(unix)]
pub(crate) fn checked_daemon_path(dir: &Path, suffix: &str) -> Result<PathBuf, OpError> {
    checked_join(dir, &format!("daemon.{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::super::{cleanup, test_dir};
    use super::*;

    #[test]
    fn names_valid_including_dotted() {
        for good in ["a", "rt1", "a.b", "x.y.z", "v1.2-rc_3", "UPPER.lower-1_2"] {
            validate_session_name(good).expect(good);
        }
        assert!(validate_session_name(&"n".repeat(64)).is_ok());
    }

    #[test]
    fn names_invalid_rejected() {
        for bad in [
            "",
            ".",
            "..",
            "daemon",
            "a/b",
            "../evil",
            "a\\b",
            "sp ace",
            "semi;colon",
            "dollar$",
            &"x".repeat(65),
        ] {
            let e = validate_session_name(bad).expect_err("bad name accepted");
            assert_eq!(e.code, "invalid-input", "{bad:?}");
        }
    }

    #[test]
    fn checked_paths_stay_contained() {
        let dir = test_dir("paths");
        let ep = checked_endpoint_path(&dir, "a.b").expect("endpoint path");
        assert_eq!(ep.parent(), Some(dir.as_path()));
        let aux = checked_aux_path(&dir, "a.b", "log").expect("aux path");
        assert_eq!(aux.parent(), Some(dir.as_path()));
        assert!(checked_endpoint_path(&dir, "../evil").is_err());
        cleanup(&dir);
    }
}
