//! Shared helpers: workspace root, file walking, child cargo.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Maintenance-command failure with a human-readable message.
#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self(err.to_string())
    }
}

/// Fallible maintenance result.
pub type Result<T> = std::result::Result<T, Error>;

/// Check outcome: `Pass` is clean, `Fail` reports findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// No findings.
    Pass,
    /// One or more findings; the caller maps this to exit 1.
    Fail,
}

impl Status {
    /// Combine two outcomes; any failure wins.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Pass, Self::Pass) => Self::Pass,
            _ => Self::Fail,
        }
    }

    /// Map to a process exit code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Self::Pass => 0,
            Self::Fail => 1,
        }
    }
}

/// Build an [`Error`] from a message.
pub fn fail(message: impl Into<String>) -> Error {
    Error(message.into())
}

/// True when `args` is exactly a help request.
#[must_use]
pub fn wants_help(args: &[String]) -> bool {
    matches!(args.first().map(String::as_str), Some("--help" | "-h"))
}

/// Locate the workspace root (parent of `crates/xtask`).
///
/// # Errors
///
/// Returns an error when the manifest directory has no grandparent.
pub fn workspace_root() -> Result<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let Some(crates) = manifest.parent() else {
        return Err(fail("xtask manifest dir has no parent"));
    };
    let Some(root) = crates.parent() else {
        return Err(fail("xtask manifest dir has no grandparent"));
    };
    Ok(root.to_path_buf())
}

/// Directory names never descended into while walking.
const SKIP_DIRS: &[&str] = &[".git", "target"];

/// Recursively list regular files under `root`, skipping `.git`/`target`.
///
/// Symlinks are never followed, so an mbx-managed `target` link is safe.
///
/// # Errors
///
/// Returns an error when a directory cannot be read.
pub fn walk_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries =
            fs::read_dir(&dir).map_err(|e| fail(format!("read {}: {e}", dir.display())))?;
        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                let skip = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| SKIP_DIRS.contains(&n));
                if !skip {
                    stack.push(path);
                }
            } else if file_type.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Read a UTF-8 text file into lines.
///
/// # Errors
///
/// Returns an error when the file cannot be read as UTF-8.
pub fn read_lines(path: &Path) -> Result<Vec<String>> {
    let text =
        fs::read_to_string(path).map_err(|e| fail(format!("read {}: {e}", path.display())))?;
    Ok(text.lines().map(str::to_string).collect())
}

/// Count physical lines in a file (byte-based; no UTF-8 requirement).
///
/// # Errors
///
/// Returns an error when the file cannot be read.
pub fn count_lines(path: &Path) -> Result<usize> {
    let bytes = fs::read(path).map_err(|e| fail(format!("read {}: {e}", path.display())))?;
    if bytes.is_empty() {
        return Ok(0);
    }
    let mut count = 0_usize;
    for byte in &bytes {
        if *byte == b'\n' {
            count += 1;
        }
    }
    if bytes.last() != Some(&b'\n') {
        count += 1;
    }
    Ok(count)
}

/// Render `path` relative to `root` for stable report output.
#[must_use]
pub fn display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Recursion-guard variable set on every child process xtask spawns.
pub const GUARD_ENV: &str = "TUISCOTTI_XTASK_ACTIVE";

/// Run `program` with `args` in `dir`, capturing stdout.
///
/// Sets [`GUARD_ENV`] so a nested xtask invocation aborts instead of recursing.
///
/// # Errors
///
/// Returns an error when the program cannot start, fails, or emits non-UTF-8.
pub fn run_program(dir: &Path, program: &OsString, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env(GUARD_ENV, "1")
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(fail(format!(
            "{} {} failed: {}",
            program.to_string_lossy(),
            args.join(" "),
            stderr.trim()
        )));
    }
    String::from_utf8(output.stdout).map_err(|e| fail(format!("output not UTF-8: {e}")))
}

/// Run `cargo` with `args` in `dir`, capturing stdout.
///
/// Honors `CARGO` when set. See [`run_program`] for the recursion guard.
///
/// # Errors
///
/// Returns an error when cargo cannot start, fails, or emits non-UTF-8.
pub fn run_cargo(dir: &Path, args: &[&str]) -> Result<String> {
    let cargo: OsString = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    run_program(dir, &cargo, args)
}
