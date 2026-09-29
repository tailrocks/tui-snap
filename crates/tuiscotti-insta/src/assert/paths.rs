//! Generation bindings and snapshot/evidence directory resolution.

use super::{EVIDENCE_DIR_ENV, GEN_DESC_PREFIX, Location, SNAPSHOT_DIR_ENV};
use std::path::{Path, PathBuf};
use tuiscotti_render::profile::Profile;

/// Content-derived generation binding: hex SHA-256 of the canonical text.
///
/// The same screen always yields the same generation; any canonical change yields a
/// different one. Embedded in snapshot descriptions AND the PNG `tEXt` chunk so the
/// two artifacts of one sample can be checked against each other.
#[must_use]
pub fn generation_id(canonical: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(canonical.as_bytes());
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub(crate) fn description_for(generation: &str, location: Location) -> String {
    format!(
        "{GEN_DESC_PREFIX}{generation} render {} at {}:{}",
        render_identity(),
        location.file,
        location.line
    )
}

/// Render identity the PNG verdict depends on: default profile name,
/// renderer version, and screenshot alpha policy. Recorded in every snapshot
/// description so a canonical-identical/render-different drift names its
/// cause. [`snap_generation`](super::snap_generation) only reads the first token, so this stays
/// parse-safe.
pub(crate) fn render_identity() -> String {
    let profile = Profile::default_profile();
    format!(
        "{}/rv{}/straight-rgba",
        profile.name,
        tuiscotti_render::profile::RENDERER_VERSION
    )
}

/// Default snapshot directory for a macro call: [`SNAPSHOT_DIR_ENV`] when set,
/// else the relative path `snapshots`. Insta joins a relative snapshot path
/// against the ASSERTION FILE's directory, and the facade assertions expand
/// at the caller — so this lands in `<caller-dir>/snapshots`, exactly Insta's
/// native default. No caller path is needed (or accepted: prefixing the
/// caller dir here would double-join).
#[must_use]
pub fn default_snapshot_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(SNAPSHOT_DIR_ENV) {
        return PathBuf::from(dir);
    }
    PathBuf::from("snapshots")
}

/// Candidate-evidence root: [`EVIDENCE_DIR_ENV`] when set, else
/// `target/tuisnap-evidence` under the enclosing workspace root (the test
/// working directory is a package dir under `cargo test`, so resolve upward;
/// falls back to the cwd when no workspace manifest is found).
#[must_use]
pub fn evidence_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(EVIDENCE_DIR_ENV) {
        return PathBuf::from(dir);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    workspace_root_of(&cwd)
        .join("target")
        .join("tuisnap-evidence")
}

/// Nearest enclosing cargo workspace root for `start`: the closest
/// ancestor-or-self whose `Cargo.toml` declares `[workspace]`. Returns
/// `start` unchanged when no workspace manifest is found.
fn workspace_root_of(start: &Path) -> PathBuf {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) {
            if text
                .lines()
                .any(|l| l.trim_start().starts_with("[workspace"))
            {
                return dir.to_path_buf();
            }
        }
        cur = dir.parent();
    }
    start.to_path_buf()
}
