//! Generation bindings and resolved snapshot identity.
//!
//! Insta resolves a snapshot file from the ASSERTION site: the caller's
//! manifest dir (workspace), the caller's file parent, the `snapshot_path`
//! setting, and the active `snapshot_suffix` (`{name}@{suffix}`). The facade
//! macros expand the Insta assertions at the caller, so the same inputs must
//! drive the compound gate — [`SnapshotIdentity`] is that one resolved
//! identity, computed from the identical inputs and used end-to-end for the
//! Insta settings, the consistency gate, and the evidence manifest.

use super::{EVIDENCE_DIR_ENV, GEN_DESC_PREFIX, Location, SNAPSHOT_DIR_ENV};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use tuiscotti_render::profile::{MissingGlyphPolicy, RenderProfile};

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
        write!(s, "{b:02x}").unwrap_or_default();
    }
    s
}

/// Compound sample binding: `v2-<hex>` over the canonical text, the render
/// identity, and the PNG payload bytes.
///
/// [`generation_id`] binds the canonical text alone, so a stale PNG rendered
/// under a different profile (or different pixels entirely) still matches by
/// generation. The binding additionally covers WHAT RENDERED the sample
/// (profile name, renderer version, alpha policy via
/// [`render_identity`]) and the exact PNG
/// payload (`png` here is the untagged renderer output; the tag chunk carries
/// the binding but never participates in it). Deterministic: identical
/// screen + profile + renderer always yield the identical binding. Run and
/// attempt identity NEVER participate — retries of the same sample share one
/// binding while landing in distinct evidence partitions.
#[must_use]
pub fn sample_binding(canonical: &str, render_identity: &str, png: &[u8]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(b"tuiscotti-sample-binding/1\n");
    h.update(canonical.as_bytes());
    h.update(b"\n");
    h.update(render_identity.as_bytes());
    h.update(b"\n");
    h.update(png);
    let digest = h.finalize();
    let mut s = String::with_capacity(3 + digest.len() * 2);
    s.push_str("v2-");
    for b in digest {
        write!(s, "{b:02x}").unwrap_or_default();
    }
    s
}

pub(crate) fn description_for(binding: &str, location: Location) -> String {
    format!(
        "{GEN_DESC_PREFIX}{binding} render {} at {}:{}",
        render_identity(),
        location.file,
        location.line
    )
}

/// Strict mirror of the legacy sample path ([`super::render_sample`]:
/// default profile, vendored faces, default fallback chain, cursor shown,
/// blink frozen-visible, placeholder missing-glyphs). The identity and the
/// frozen-gate cache key derive from this ONE profile, so they can never
/// describe different render inputs than the sample renderer consumes.
///
/// Built once per process: every input is a `&'static` asset, so a cached
/// value is identical to a fresh one (and [`RenderProfile::vendored`] itself
/// verifies its pins on first construction).
pub(crate) fn default_sample_profile() -> RenderProfile<'static> {
    static PROFILE: std::sync::OnceLock<RenderProfile<'static>> = std::sync::OnceLock::new();
    PROFILE
        .get_or_init(|| RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder))
        .clone()
}

/// Render identity the PNG verdict depends on, for an explicit strict
/// profile: profile name, renderer version, screenshot alpha policy, and
/// the full strict content hash (styled-face pins, ordered fallback chain,
/// geometry, scale, palette, cursor/blink/missing policies, version).
/// Recorded in every snapshot description so a
/// canonical-identical/render-different drift names its cause, and covered
/// by [`sample_binding`]. Only the first description token is binding-parsed,
/// so this stays parse-safe.
#[must_use]
pub fn render_identity_for(rp: &RenderProfile<'_>) -> String {
    format!(
        "{}/rv{}/straight-rgba/profile-{}",
        rp.name(),
        rp.renderer_version(),
        rp.hash()
    )
}

/// Render identity of the pinned sample path
/// ([`render_identity_for`] over the legacy-mirror strict profile).
///
/// Deterministic over static inputs, so it is rendered once per process:
/// every `assert_screenshot!` calls this three times (bundle binding plus
/// two snapshot descriptions) and every `assert_snapshot!` once.
#[must_use]
pub fn render_identity() -> String {
    static IDENTITY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    IDENTITY
        .get_or_init(|| render_identity_for(&default_sample_profile()))
        .clone()
}

/// One resolved snapshot identity: the absolute directory Insta reads/writes
/// for this assertion plus the resolved (suffixed) file stems of the
/// canonical and PNG snapshots. Computed from the same inputs Insta itself
/// uses (caller manifest dir, caller file, `snapshot_path`, active suffix),
/// so the Insta settings and the compound gate can never disagree about
/// WHICH files they mean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotIdentity {
    /// Absolute snapshot directory (caller-derived default or override).
    pub dir: PathBuf,
    /// Resolved canonical file stem (`{name}` or `{name}@{suffix}`).
    pub canonical: String,
    /// Resolved PNG file stem (`{name}-img` or `{name}-img@{suffix}`).
    pub png_base: String,
}

impl SnapshotIdentity {
    /// Absolute path of the canonical `.snap` file.
    #[must_use]
    pub fn canonical_snap(&self) -> PathBuf {
        self.dir.join(format!("{}.snap", self.canonical))
    }

    /// Absolute path of the PNG `.snap` metadata file.
    #[must_use]
    pub fn png_snap(&self) -> PathBuf {
        self.dir.join(format!("{}.snap", self.png_base))
    }

    /// Absolute path of the PNG sidecar bytes.
    #[must_use]
    pub fn png_sidecar(&self) -> PathBuf {
        self.dir.join(format!("{}.snap.png", self.png_base))
    }
}

/// Active Insta snapshot suffix from the ambient settings, if any (public
/// API: [`insta::Settings::snapshot_suffix`]). The facade honors an outer
/// suffix for parameterized tests; the identity applies it EXACTLY as Insta
/// 1.48 does for explicit names (`{name}@{suffix}`, both text and binary).
#[must_use]
pub fn active_snapshot_suffix() -> Option<String> {
    insta::Settings::clone_current()
        .snapshot_suffix()
        .map(str::to_string)
}

/// Apply the resolved suffix to a snapshot stem, mirroring Insta.
#[must_use]
pub fn suffixed_name(name: &str, suffix: Option<&str>) -> String {
    match suffix {
        Some(s) => format!("{name}@{s}"),
        None => name.to_string(),
    }
}

/// Map a snapshot stem to its file stem the way Insta does (`/` and `\`
/// become `__`). The facade validates scenario names on the evidence path;
/// the snapshot path mirrors Insta exactly so the gate reads what Insta
/// wrote.
#[must_use]
pub fn snapshot_file_stem(stem: &str) -> String {
    stem.replace(['/', '\\'], "__")
}

/// Resolve the snapshot identity for a macro call: `manifest_dir` is the
/// CALLER's `env!("CARGO_MANIFEST_DIR")`, `location` the caller
/// `file!()`/`line!()`, `override_dir` the [`SNAPSHOT_DIR_ENV`] value when
/// set (read by the macro; `None` in tests means the default).
///
/// The default directory is `<caller-dir>/snapshots` as an ABSOLUTE path
/// (`<manifest>/<caller-parent>/snapshots`), which is exactly where Insta
/// joins the relative default — so the Insta settings (given this absolute
/// path) and the compound gate resolve to the same files whether the
/// process runs from the package dir or the workspace root. A relative
/// override is joined onto the caller dir the same way; an absolute
/// override is used verbatim.
#[must_use]
pub fn resolve_snapshot_identity(
    manifest_dir: &str,
    location: Location,
    name: &str,
    override_dir: Option<&Path>,
) -> SnapshotIdentity {
    let caller_parent = Path::new(location.file)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let caller_dir = Path::new(manifest_dir).join(caller_parent);
    let dir = match override_dir {
        Some(o) if o.is_absolute() => o.to_path_buf(),
        Some(o) => caller_dir.join(o),
        None => caller_dir.join("snapshots"),
    };
    let suffix = active_snapshot_suffix();
    SnapshotIdentity {
        dir,
        canonical: snapshot_file_stem(&suffixed_name(name, suffix.as_deref())),
        png_base: snapshot_file_stem(&suffixed_name(
            &super::png_snapshot_base(name),
            suffix.as_deref(),
        )),
    }
}

/// Resolve the identity against an EXPLICIT snapshot directory
/// ([`Policy::EvolvingIn`](super::Policy::EvolvingIn)). Same suffix/file
/// rules as [`resolve_snapshot_identity`]; a relative base is joined onto
/// the caller dir so the gate cannot disagree with Insta's own join.
#[must_use]
pub fn resolve_snapshot_identity_in(
    manifest_dir: &str,
    location: Location,
    snapshots: &Path,
    name: &str,
) -> SnapshotIdentity {
    resolve_snapshot_identity(manifest_dir, location, name, Some(snapshots))
}

/// Snapshot-directory override from the environment, if set (read by the
/// facade macros; pure resolution takes it as a parameter so tests never
/// need `set_var`).
#[must_use]
pub fn snapshot_dir_override() -> Option<PathBuf> {
    std::env::var_os(SNAPSHOT_DIR_ENV).map(PathBuf::from)
}

/// Candidate-evidence root: [`EVIDENCE_DIR_ENV`] when set, else
/// `target/tuiscotti-evidence` under the enclosing workspace root (the test
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
        .join("tuiscotti-evidence")
}

/// Nearest enclosing cargo workspace root for `start`: the closest
/// ancestor-or-self whose `Cargo.toml` declares `[workspace]`. Returns
/// `start` unchanged when no workspace manifest is found.
fn workspace_root_of(start: &Path) -> PathBuf {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml"))
            && text
                .lines()
                .any(|l| l.trim_start().starts_with("[workspace"))
        {
            return dir.to_path_buf();
        }
        cur = dir.parent();
    }
    start.to_path_buf()
}
