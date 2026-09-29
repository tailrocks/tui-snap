use super::*;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Executable resolution failure (N03, N04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// No candidate named an existing file. `searched` lists the env vars
    /// consulted; `values` lists values that were set but pointed nowhere.
    Missing {
        package: String,
        bin: String,
        searched: Vec<String>,
        values: Vec<(String, String)>,
    },
    /// Two distinct existing files were named. The caller must disambiguate;
    /// this adapter never silently picks one.
    Ambiguous {
        package: String,
        bin: String,
        candidates: Vec<PathBuf>,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::Missing {
                package,
                bin,
                searched,
                values,
            } => {
                write!(
                    f,
                    "cannot resolve binary '{bin}' of package '{package}': no candidate exists \
                     (searched vars: {}; set-but-missing: {:?}). Refusing to guess a target-dir path.",
                    searched.join(", "),
                    values
                )
            }
            ResolveError::Ambiguous {
                package,
                bin,
                candidates,
            } => {
                write!(
                    f,
                    "ambiguous binary '{bin}' of package '{package}': distinct existing candidates: {:?}",
                    candidates
                )
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// Resolve a binary target's executable (N03, N04).
///
/// Consults, in order, `NEXTEST_BIN_EXE_<bin>` (exact, then `-`→`_` form) and
/// `CARGO_BIN_EXE_<bin>` (exact, then `-`→`_` form). Nextest remaps these when
/// reusing archived builds, so they stay correct under archive/remap runs.
/// Identical paths dedupe (nextest sets both hyphen and underscore forms).
///
/// There is deliberately no `target/debug` probing and no nested `cargo build`:
/// both would silently use stale or source-relative paths.
/// Reads the process environment; see [`resolve_bin_with_map`] for the pure form.
pub fn resolve_bin(package: &str, bin: &str) -> Result<PathBuf, ResolveError> {
    let env: HashMap<String, String> = std::env::vars().collect();
    resolve_bin_with_map(package, bin, &env)
}

/// [`resolve_bin`] over an injected environment.
pub fn resolve_bin_with_map(
    package: &str,
    bin: &str,
    env: &HashMap<String, String>,
) -> Result<PathBuf, ResolveError> {
    let underscored = bin.replace('-', "_");
    let mut names = vec![
        format!("NEXTEST_BIN_EXE_{bin}"),
        format!("CARGO_BIN_EXE_{bin}"),
    ];
    if underscored != bin {
        names.push(format!("NEXTEST_BIN_EXE_{underscored}"));
        names.push(format!("CARGO_BIN_EXE_{underscored}"));
    }
    let mut values = Vec::new();
    let mut existing: Vec<PathBuf> = Vec::new();
    for name in &names {
        if let Some(v) = env.get(name) {
            let p = PathBuf::from(v);
            if p.is_file() {
                if !existing.contains(&p) {
                    existing.push(p);
                }
            } else {
                values.push((name.clone(), v.clone()));
            }
        }
    }
    if existing.len() > 1 {
        return Err(ResolveError::Ambiguous {
            package: package.to_string(),
            bin: bin.to_string(),
            candidates: existing,
        });
    }
    existing.pop().ok_or_else(|| ResolveError::Missing {
        package: package.to_string(),
        bin: bin.to_string(),
        searched: names,
        values,
    })
}
