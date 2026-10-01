use crate::command::cargo_bin_env_names;
use std::collections::HashMap;
use std::path::PathBuf;

/// Executable resolution failure (N03, N04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// No candidate named an existing file. `searched` lists the env vars
    /// consulted; `values` lists values that were set but pointed nowhere.
    Missing {
        /// Cargo package the binary belongs to.
        package: String,
        /// Binary target name that could not be resolved.
        bin: String,
        /// Environment variables consulted, in order.
        searched: Vec<String>,
        /// Set-but-missing `(variable, value)` pairs.
        values: Vec<(String, String)>,
    },
    /// Two distinct existing files were named. The caller must disambiguate;
    /// this adapter never silently picks one.
    Ambiguous {
        /// Cargo package the binary belongs to.
        package: String,
        /// Binary target name with multiple candidates.
        bin: String,
        /// Distinct existing candidate paths.
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
                    "ambiguous binary '{bin}' of package '{package}': distinct existing candidates: {candidates:?}"
                )
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// Resolve a binary target's executable (N03, N04).
///
/// Consults, in order, `NEXTEST_BIN_EXE_<bin>` (exact, then `-`→`_` form —
/// nextest sets both) and the canonical [`cargo_bin_env_names`] (exact, then
/// normalized). Nextest remaps these when reusing archived builds, so they
/// stay correct under archive/remap runs. Identical paths dedupe.
///
/// There is deliberately no `target/debug` probing and no nested `cargo build`:
/// both would silently use stale or source-relative paths.
/// Reads the process environment; see [`resolve_bin_with_map`] for the pure form.
/// # Errors
///
/// Returns [`ResolveError`] when no candidate exists or several do.
pub fn resolve_bin(package: &str, bin: &str) -> Result<PathBuf, ResolveError> {
    let env: HashMap<String, String> = std::env::vars().collect();
    resolve_bin_with_map(package, bin, &env)
}

/// [`resolve_bin`] over an injected environment.
/// # Errors
///
/// Returns [`ResolveError`] when no candidate exists or several do.
pub fn resolve_bin_with_map<S: std::hash::BuildHasher>(
    package: &str,
    bin: &str,
    env: &HashMap<String, String, S>,
) -> Result<PathBuf, ResolveError> {
    let underscored = bin.replace('-', "_");
    let cargo_names = cargo_bin_env_names(bin);
    let mut names = vec![format!("NEXTEST_BIN_EXE_{bin}"), cargo_names[0].clone()];
    if underscored != bin {
        names.push(format!("NEXTEST_BIN_EXE_{underscored}"));
    }
    names.extend(cargo_names.into_iter().skip(1));
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
