//! Compound consistency gates, frozen policy, and four-artifact export.

use super::{
    AssertError, FOUR_STEM, check_scenario_name, generation_id, png_generation, render_sample,
    snap_generation,
};
use std::path::{Path, PathBuf};
use tuiscotti_core::screen::Screen;
use tuiscotti_core::screen::canonical_string;
use tuiscotti_render::diff::{AlphaPolicy, compare_png_with_alpha};

/// Compound-consistency failure: mixed or unreadable generation bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsistencyError(pub String);

impl std::fmt::Display for ConsistencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "inconsistent compound snapshot: {}", self.0)
    }
}

impl std::error::Error for ConsistencyError {}

/// Strict compound gate (I04/C08): the approved canonical `.snap`, the approved
/// PNG `.snap`, and the PNG sidecar bytes must all carry the SAME generation.
/// Any mismatch — or any missing binding — is an error.
///
/// # Errors
///
/// Returns [`ConsistencyError`] when a generation binding is missing or
/// unreadable, or when the three bindings disagree.
pub fn check_consistent(dir: &Path, canonical: &str, png: &str) -> Result<(), ConsistencyError> {
    let c = snap_generation(&dir.join(format!("{canonical}.snap")))
        .ok_or_else(|| ConsistencyError(format!("{canonical}.snap: missing generation binding")))?;
    let p = snap_generation(&dir.join(format!("{png}.snap")))
        .ok_or_else(|| ConsistencyError(format!("{png}.snap: missing generation binding")))?;
    let sidecar = std::fs::read(dir.join(format!("{png}.snap.png")))
        .map_err(|e| ConsistencyError(format!("{png}.snap.png unreadable: {e}")))?;
    let t = png_generation(&sidecar)
        .ok_or_else(|| ConsistencyError(format!("{png}.snap.png: missing tEXt generation")))?;
    if c == p && p == t {
        Ok(())
    } else {
        Err(ConsistencyError(format!(
            "mixed compound baseline: canonical={c} png-meta={p} png-bytes={t}"
        )))
    }
}

// ---------------------------------------------------------------------------
// Policy: evolving review vs frozen roots (I06)
// ---------------------------------------------------------------------------

/// Reference policy: evolving snapshots go through Insta review; frozen roots
/// are read-only directories of approved canonical+PNG files.
#[derive(Debug, Clone)]
pub enum Policy {
    /// Native Insta pending/review flow ([`crate::assert_snapshot!`] /
    /// [`crate::assert_screenshot!`] with caller-fixed metadata).
    Evolving,
    /// Same flow with explicit snapshot and evidence directories (no env).
    /// Hermetic tests pass tempdirs here: `set_var` is an `unsafe fn` in
    /// edition 2024 and cannot be used under the workspace lints, so the
    /// `TUISCOTTI_SNAPSHOT_DIR` / `TUISCOTTI_EVIDENCE_DIR` overrides are
    /// unreachable in-process.
    EvolvingIn {
        /// Snapshot directory (approvals are read here, never written).
        snapshots: PathBuf,
        /// Candidate-evidence root (one partitioned bundle per assert).
        evidence: PathBuf,
    },
    /// Read-only root holding `<name>.canonical.txt` + `<name>.png` per
    /// scenario. The assert path fails on missing/corrupt/mismatched files,
    /// [`frozen_accept`] always errors, and no frozen path ever writes.
    Frozen {
        /// Approved-artifact root (never modified).
        root: PathBuf,
    },
}

/// Frozen-assertion failure: explicit, never silent, never self-healing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrozenError {
    /// Scenario name fails validation (mirrors `grouped::validate_name`).
    InvalidName(String),
    /// Approved file absent.
    Missing {
        /// Path that should have held the approval.
        path: PathBuf,
    },
    /// Approved file unreadable, non-UTF-8 canonical, or undecodable PNG.
    Corrupt {
        /// Approved file at fault.
        path: PathBuf,
        /// Why it is unusable.
        reason: String,
    },
    /// Approved content differs from the actual sample.
    Mismatch {
        /// Scenario name.
        name: String,
        /// What differs.
        detail: String,
    },
    /// Any accept attempt against a frozen root.
    AcceptRejected {
        /// The frozen root.
        root: PathBuf,
        /// Scenario someone tried to bless.
        name: String,
    },
}

impl std::fmt::Display for FrozenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrozenError::InvalidName(e) => write!(f, "invalid frozen name: {e}"),
            FrozenError::Missing { path } => {
                write!(f, "frozen approval missing: {}", path.display())
            }
            FrozenError::Corrupt { path, reason } => {
                write!(f, "frozen approval corrupt: {}: {reason}", path.display())
            }
            FrozenError::Mismatch { name, detail } => {
                write!(f, "frozen mismatch for {name:?}: {detail}")
            }
            FrozenError::AcceptRejected { root, name } => write!(
                f,
                "frozen root {} rejects acceptance of {name:?}",
                root.display()
            ),
        }
    }
}

impl std::error::Error for FrozenError {}

fn frozen_canonical_path(root: &Path, name: &str) -> PathBuf {
    root.join(format!("{name}.canonical.txt"))
}

fn frozen_png_path(root: &Path, name: &str) -> PathBuf {
    root.join(format!("{name}.png"))
}

fn read_approved(path: &Path) -> Result<Vec<u8>, FrozenError> {
    match std::fs::read(path) {
        Ok(b) => Ok(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(FrozenError::Missing {
            path: path.to_path_buf(),
        }),
        Err(e) => Err(FrozenError::Corrupt {
            path: path.to_path_buf(),
            reason: e.to_string(),
        }),
    }
}

/// Check canonical state against a frozen root. Fails on missing/corrupt files
/// and on content mismatch. Reads only.
///
/// # Errors
///
/// Returns [`FrozenError`] when the name is invalid, the approval is missing
/// or corrupt, or its content differs from the actual screen.
pub fn check_frozen_snapshot(root: &Path, name: &str, screen: &Screen) -> Result<(), FrozenError> {
    check_scenario_name(name).map_err(FrozenError::InvalidName)?;
    let path = frozen_canonical_path(root, name);
    let approved = read_approved(&path)?;
    let approved_text = String::from_utf8(approved).map_err(|e| FrozenError::Corrupt {
        path: path.clone(),
        reason: format!("canonical is not UTF-8: {e}"),
    })?;
    let actual = canonical_string(screen);
    if approved_text != actual {
        return Err(FrozenError::Mismatch {
            name: name.to_string(),
            detail: format!(
                "canonical differs: approved {} bytes, actual {} bytes",
                approved_text.len(),
                actual.len()
            ),
        });
    }
    Ok(())
}

/// Check canonical state plus PNG pixels against a frozen root. Also fails when
/// a tagged approved PNG disagrees with the canonical generation (untagged
/// legacy PNGs keep the pixel verdict). Reads only.
///
/// # Errors
///
/// Returns [`FrozenError`] when the canonical check fails, the approved PNG is
/// missing or undecodable, the pixels differ, or a PNG generation tag
/// disagrees with the canonical generation.
pub fn check_frozen_screenshot(
    root: &Path,
    name: &str,
    screen: &Screen,
) -> Result<(), FrozenError> {
    check_frozen_snapshot(root, name, screen)?;
    let path = frozen_png_path(root, name);
    let approved_png = read_approved(&path)?;
    image::load_from_memory(&approved_png).map_err(|e| FrozenError::Corrupt {
        path: path.clone(),
        reason: format!("PNG does not decode: {e}"),
    })?;
    let sample = render_sample(screen).map_err(|e| FrozenError::Corrupt {
        path: path.clone(),
        reason: format!("cannot render actual: {e}"),
    })?;
    let verdict = compare_png_with_alpha(&approved_png, &sample.png, AlphaPolicy::StraightRgba)
        .map_err(|e| FrozenError::Corrupt {
            path: path.clone(),
            reason: e.to_string(),
        })?;
    if !verdict.pixels_equal {
        return Err(FrozenError::Mismatch {
            name: name.to_string(),
            detail: format!(
                "PNG pixels differ: expected {:?}, actual {:?}",
                verdict.expected_dims, verdict.actual_dims
            ),
        });
    }
    let generation = generation_id(&sample.canonical);
    if let Some(tag) = png_generation(&approved_png)
        && tag != generation
    {
        return Err(FrozenError::Mismatch {
            name: name.to_string(),
            detail: format!("generation mismatch: canonical={generation} png-bytes={tag}"),
        });
    }
    Ok(())
}

/// Assert canonical state against a frozen root. Panics on any [`FrozenError`].
///
/// # Panics
///
/// Panics with the [`FrozenError`] message when [`check_frozen_snapshot`]
/// fails.
#[expect(
    clippy::panic,
    reason = "assert_* API panics by contract, like std assert"
)]
pub fn assert_frozen_snapshot(root: &Path, name: &str, screen: &Screen) {
    if let Err(e) = check_frozen_snapshot(root, name, screen) {
        panic!("tuiscotti frozen snapshot {name:?} failed: {e}");
    }
}

/// Assert canonical state plus PNG against a frozen root. Panics on any [`FrozenError`].
///
/// # Panics
///
/// Panics with the [`FrozenError`] message when [`check_frozen_screenshot`]
/// fails.
#[expect(
    clippy::panic,
    reason = "assert_* API panics by contract, like std assert"
)]
pub fn assert_frozen_screenshot(root: &Path, name: &str, screen: &Screen) {
    if let Err(e) = check_frozen_screenshot(root, name, screen) {
        panic!("tuiscotti frozen screenshot {name:?} failed: {e}");
    }
}

/// Frozen roots reject acceptance unconditionally: always returns
/// [`FrozenError::AcceptRejected`] and writes nothing.
///
/// # Errors
///
/// Always returns [`FrozenError::AcceptRejected`]; frozen roots never bless.
pub fn frozen_accept(root: &Path, name: &str) -> Result<(), FrozenError> {
    Err(FrozenError::AcceptRejected {
        root: root.to_path_buf(),
        name: name.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Four-artifact export + read-only frozen-tree importer (I07)
// ---------------------------------------------------------------------------

/// Paths written by [`emit_four`].
#[derive(Debug, Clone)]
pub struct EmittedPaths {
    /// Directory the artifacts were written to.
    pub dir: PathBuf,
    /// `<dir>/snapshot.ansi` (normalized SGR dump).
    pub ansi: PathBuf,
    /// `<dir>/snapshot.txt` (plain text).
    pub txt: PathBuf,
    /// `<dir>/snapshot.png` (authoritative PNG).
    pub png: PathBuf,
    /// `<dir>/snapshot.html` (standalone render).
    pub html: PathBuf,
}

/// Emit ANSI/TXT/PNG/HTML from one [`Screen`] in a single sample pass.
/// Byte-deterministic: the same screen always yields identical bytes.
///
/// # Errors
///
/// Returns [`AssertError`] when rendering fails or an artifact cannot be
/// written.
pub fn emit_four(screen: &Screen, dir: &Path) -> Result<EmittedPaths, AssertError> {
    let sample = render_sample(screen)?;
    let io = |p: &Path, e: std::io::Error| AssertError::Io(format!("{}: {e}", p.display()));
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    let paths = EmittedPaths {
        dir: dir.to_path_buf(),
        ansi: dir.join(format!("{FOUR_STEM}.ansi")),
        txt: dir.join(format!("{FOUR_STEM}.txt")),
        png: dir.join(format!("{FOUR_STEM}.png")),
        html: dir.join(format!("{FOUR_STEM}.html")),
    };
    std::fs::write(&paths.ansi, sample.ansi.as_bytes()).map_err(|e| io(&paths.ansi, e))?;
    std::fs::write(&paths.txt, sample.txt.as_bytes()).map_err(|e| io(&paths.txt, e))?;
    std::fs::write(&paths.png, &sample.png).map_err(|e| io(&paths.png, e))?;
    std::fs::write(&paths.html, sample.html.as_bytes()).map_err(|e| io(&paths.html, e))?;
    Ok(paths)
}
