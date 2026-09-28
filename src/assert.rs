//! Public assertion facade: snapshot/screenshot macros + frozen policy (M2: I01, I02, I06, I07).
//!
//! - [`assert_snapshot!`]: styled canonical state ([`crate::insta_proto::insta_string`])
//!   through native Insta review, with a content-derived generation binding carried in
//!   the snapshot description.
//! - [`assert_screenshot!`]: canonical state PLUS an independently rendered PNG as ONE
//!   sample. Candidate evidence (PNG + ANSI/TXT/HTML from the same sample) is written
//!   BEFORE any failure; the PNG is compared by decoded pixels
//!   ([`crate::insta_proto::PngPixelComparator`]); a generation mismatch between the
//!   accepted canonical and PNG snapshots fails via [`check_consistent`].
//! - [`Policy`]: [`Policy::Evolving`] is the Insta review flow above;
//!   [`Policy::Frozen`] pins a read-only directory of approved canonical+PNG files that
//!   rejects acceptance and never self-heals.
//! - [`emit_four`] / [`import_frozen_v1`]: four-artifact (ANSI/TXT/PNG/HTML) export from
//!   one [`Screen`] and a read-only importer for classic/grouped four-file trees.
//!
//! Design notes (public-API gaps found while building this):
//! - The macros capture `file!()`/`line!()` and forward to `*_impl` functions, so the
//!   Insta assertion textually expands inside this module: the `.snap` `source:` field
//!   names `src/assert.rs`, not the caller. The caller location is therefore ALSO
//!   embedded in the snapshot description (`... at <file>:<line>`), which review tools
//!   display. The default snapshot directory is derived from the CALLER file
//!   (`<caller-dir>/snapshots`, mirroring Insta's native default); set
//!   [`SNAPSHOT_DIR_ENV`] to override.
//! - `insta_proto` carries no `check_consistent` (it only ever existed as a local helper
//!   in `tests/insta_spike.rs`), and this facade may not touch that module — so the
//!   canonical consistency gate lives here ([`check_consistent`]).
//! - Insta exposes no `Settings` switch for the update behavior; tests forbid auto-write
//!   with `INSTA_UPDATE=no` set in-process before the first assertion (Insta memoizes
//!   tool config per workspace binary).
//!
//! Environment:
//! - [`SNAPSHOT_DIR_ENV`]: explicit Insta snapshot directory (tests point it at a
//!   tempdir; unset means the caller-derived default above).
//! - [`EVIDENCE_DIR_ENV`]: candidate-evidence root for [`assert_screenshot!`] (default
//!   `target/tuisnap-evidence`). Files are `<name>.{png,ansi,txt,html}`.

use crate::diff::{compare_png_with_alpha, AlphaPolicy};
use crate::frame::{Frame, Provenance, FRAME_VERSION};
use crate::insta_proto::{insta_string, PngPixelComparator};
use crate::profile::{Profile, VENDORED_FACES};
use crate::render::Renderer;
use crate::screen::Screen;
use std::path::{Path, PathBuf};

/// Env var overriding the Insta snapshot directory for the facade macros.
pub const SNAPSHOT_DIR_ENV: &str = "TUISNAP_SNAPSHOT_DIR";
/// Env var overriding the candidate-evidence root for [`assert_screenshot!`].
pub const EVIDENCE_DIR_ENV: &str = "TUISNAP_EVIDENCE_DIR";
/// PNG `tEXt` keyword carrying the sample generation inside the PNG bytes.
pub const PNG_GEN_KEYWORD: &str = "tuisnap:generation";
/// Snapshot-description prefix carrying the sample generation (parse: first token).
pub const GEN_DESC_PREFIX: &str = "tuisnap generation ";
/// Suffix mapping a screenshot name to its PNG snapshot base (`<name>-img`).
pub const PNG_SNAPSHOT_SUFFIX: &str = "-img";
/// File stem used by [`emit_four`] (`snapshot.{ansi,txt,png,html}`).
pub const FOUR_STEM: &str = "snapshot";

/// Caller location captured by the facade macros.
#[derive(Debug, Clone, Copy)]
pub struct Location {
    /// `file!()` at the macro call site.
    pub file: &'static str,
    /// `line!()` at the macro call site.
    pub line: u32,
}

/// Assert styled canonical state through native Insta review (I01).
///
/// `$name` is the snapshot name, `$screen` a `&Screen`. An optional [`Policy`]
/// switches between the evolving review flow and a frozen root.
#[macro_export]
macro_rules! assert_snapshot {
    ($name:expr, $screen:expr) => {
        $crate::assert::assert_snapshot_impl(
            $name,
            $screen,
            $crate::assert::Location {
                file: file!(),
                line: line!(),
            },
        )
    };
    ($name:expr, $screen:expr, $policy:expr) => {
        $crate::assert::assert_snapshot_with_policy(
            $policy,
            $name,
            $screen,
            $crate::assert::Location {
                file: file!(),
                line: line!(),
            },
        )
    };
}

/// Assert canonical state plus an independently rendered PNG as one sample (I02).
///
/// Candidate evidence (`<name>.{png,ansi,txt,html}` under [`EVIDENCE_DIR_ENV`])
/// is written BEFORE any failure. The PNG snapshot is named `<name>-img`.
#[macro_export]
macro_rules! assert_screenshot {
    ($name:expr, $screen:expr) => {
        $crate::assert::assert_screenshot_impl(
            $name,
            $screen,
            $crate::assert::Location {
                file: file!(),
                line: line!(),
            },
        )
    };
    ($name:expr, $screen:expr, $policy:expr) => {
        $crate::assert::assert_screenshot_with_policy(
            $policy,
            $name,
            $screen,
            $crate::assert::Location {
                file: file!(),
                line: line!(),
            },
        )
    };
}

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

fn description_for(generation: &str, location: Location) -> String {
    format!(
        "{GEN_DESC_PREFIX}{generation} at {}:{}",
        location.file, location.line
    )
}

/// Snapshot directory for a macro call: [`SNAPSHOT_DIR_ENV`] when set, else
/// `<caller-dir>/snapshots` (mirrors Insta's native default, relative to the
/// CALLER file since the assertion textually expands here).
#[must_use]
pub fn snapshot_dir_for(caller_file: &str) -> PathBuf {
    if let Ok(dir) = std::env::var(SNAPSHOT_DIR_ENV) {
        return PathBuf::from(dir);
    }
    match Path::new(caller_file).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join("snapshots"),
        _ => PathBuf::from("snapshots"),
    }
}

/// Candidate-evidence root: [`EVIDENCE_DIR_ENV`] when set, else
/// `target/tuisnap-evidence` under the test working directory.
#[must_use]
pub fn evidence_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(EVIDENCE_DIR_ENV) {
        return PathBuf::from(dir);
    }
    Path::new("target").join("tuisnap-evidence")
}

/// Deterministic [`Frame`] from a [`Screen`] for rendering evidence.
///
/// Cells and cursor are preserved exactly; provenance is fixed with
/// `created_unix = 0` so every derived artifact is byte-deterministic for
/// identical screens (the timestamp is informational and excluded from gates).
#[must_use]
pub fn frame_from_screen(screen: &Screen) -> Frame {
    Frame {
        version: FRAME_VERSION,
        cols: screen.cols(),
        rows: screen.rows(),
        cells: screen.cells().to_vec(),
        cursor: *screen.cursor(),
        provenance: Provenance {
            tool: "tuisnap".to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            profile: "tuisnap-default".to_string(),
            source: "tuisnap-assert".to_string(),
            argv: Vec::new(),
            created_unix: 0,
        },
    }
}

/// One visual sample: canonical state plus all four rendered artifacts.
#[derive(Debug, Clone)]
pub struct Sample {
    /// Styled canonical state ([`crate::insta_proto::insta_string`]).
    pub canonical: String,
    /// Normalized SGR dump.
    pub ansi: String,
    /// Plain text.
    pub txt: String,
    /// Standalone HTML render (embeds the PNG + frame JSON).
    pub html: String,
    /// Authoritative PNG (untagged; see [`png_tag_generation`]).
    pub png: Vec<u8>,
}

/// Render/IO failure from sample rendering, evidence writing, or export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertError {
    /// The pinned renderer refused the frame.
    Render(String),
    /// Filesystem failure (path context included).
    Io(String),
}

impl std::fmt::Display for AssertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssertError::Render(e) => write!(f, "render error: {e}"),
            AssertError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for AssertError {}

impl From<crate::render::RenderError> for AssertError {
    fn from(e: crate::render::RenderError) -> Self {
        AssertError::Render(e.to_string())
    }
}

/// Render one sample from a screen: canonical projection plus all four artifacts
/// from a single [`Renderer`] pass over the default profile and vendored faces.
pub fn render_sample(screen: &Screen) -> Result<Sample, AssertError> {
    let frame = frame_from_screen(screen);
    let profile = Profile::default_profile();
    let mut renderer = Renderer::new(&profile, &VENDORED_FACES)?;
    let artifacts = renderer.render_artifacts(&frame, "tuisnap")?;
    Ok(Sample {
        canonical: insta_string(screen),
        ansi: artifacts.ansi,
        txt: artifacts.txt,
        html: artifacts.html,
        png: artifacts.png,
    })
}

/// Comparator helper for PNG snapshots: decoded-pixel equality under an explicit
/// [`AlphaPolicy`] (text snapshots keep stock Insta semantics).
#[must_use]
pub fn png_comparator(alpha: AlphaPolicy) -> PngPixelComparator {
    PngPixelComparator::new(alpha)
}

fn evolving_settings(location: Location, generation: &str) -> insta::Settings {
    let mut settings = insta::Settings::new();
    settings.set_snapshot_path(snapshot_dir_for(location.file));
    settings.set_prepend_module_to_snapshot(false);
    settings.set_description(description_for(generation, location));
    settings
}

/// `assert_snapshot!` implementation: canonical text through native Insta review.
/// Panics on mismatch (native Insta failure); never writes approvals itself.
pub fn assert_snapshot_impl(name: &str, screen: &Screen, location: Location) {
    let canonical = insta_string(screen);
    let generation = generation_id(&canonical);
    let settings = evolving_settings(location, &generation);
    let owned_name = name.to_string();
    settings.bind(|| {
        insta::assert_snapshot!(owned_name, canonical);
    });
}

/// `assert_screenshot!` implementation: canonical + PNG as one sample.
///
/// Order: render the sample, write candidate evidence, assert canonical, assert
/// PNG (decoded pixels), then run the compound generation gate. Any failure
/// panics; evidence is always on disk first.
pub fn assert_screenshot_impl(name: &str, screen: &Screen, location: Location) {
    let sample = render_sample(screen).unwrap_or_else(|e| {
        panic!("tuisnap assert_screenshot!({name:?}): cannot render sample: {e}")
    });
    let generation = generation_id(&sample.canonical);
    let png = png_tag_generation(&sample.png, &generation);
    write_evidence(name, &sample, &png).unwrap_or_else(|e| {
        panic!("tuisnap assert_screenshot!({name:?}): cannot write evidence: {e}")
    });
    let snapshot_dir = snapshot_dir_for(location.file);
    let canonical_name = name.to_string();
    let canonical_text = sample.canonical.clone();
    evolving_settings(location, &generation).bind(|| {
        insta::assert_snapshot!(canonical_name, canonical_text);
    });
    let png_base = format!("{name}{PNG_SNAPSHOT_SUFFIX}");
    let png_name = format!("{png_base}.png");
    let mut png_settings = evolving_settings(location, &generation);
    png_settings.set_comparator(Box::new(png_comparator(AlphaPolicy::StraightRgba)));
    png_settings.bind(|| {
        insta::assert_binary_snapshot!(png_name.as_str(), png);
    });
    if let Err(e) = check_consistent_lenient(&snapshot_dir, name, &png_base) {
        panic!("tuisnap assert_screenshot!({name:?}): {e}");
    }
}

fn write_evidence(name: &str, sample: &Sample, png: &[u8]) -> Result<(), AssertError> {
    let dir = evidence_dir();
    let io = |p: &Path, e: std::io::Error| AssertError::Io(format!("{}: {e}", p.display()));
    let base = dir.join(name);
    if let Some(parent) = base.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    }
    let png_path = dir.join(format!("{name}.png"));
    let ansi_path = dir.join(format!("{name}.ansi"));
    let txt_path = dir.join(format!("{name}.txt"));
    let html_path = dir.join(format!("{name}.html"));
    std::fs::write(&png_path, png).map_err(|e| io(&png_path, e))?;
    std::fs::write(&ansi_path, sample.ansi.as_bytes()).map_err(|e| io(&ansi_path, e))?;
    std::fs::write(&txt_path, sample.txt.as_bytes()).map_err(|e| io(&txt_path, e))?;
    std::fs::write(&html_path, sample.html.as_bytes()).map_err(|e| io(&html_path, e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// PNG generation tagging (tEXt chunk) + compound consistency gate
// ---------------------------------------------------------------------------

const PNG_SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = if crc & 1 == 1 { 0xEDB8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

/// Insert a `tEXt` generation chunk before `IEND`. Decoders ignore it (the pixel
/// verdict is unaffected); [`check_consistent`] reads it back.
///
/// Panics on malformed PNG input or a bad keyword (caller bug: renderer output
/// is always well-formed and the keyword is fixed).
#[must_use]
pub fn png_tag_generation(png: &[u8], generation: &str) -> Vec<u8> {
    assert!(png.starts_with(&PNG_SIG), "png_tag_generation: not a PNG");
    assert!(
        !PNG_GEN_KEYWORD.contains('\0') && PNG_GEN_KEYWORD.len() <= 79,
        "png_tag_generation: bad keyword"
    );
    assert!(
        !generation.contains('\0'),
        "png_tag_generation: generation contains NUL"
    );
    let mut data = Vec::new();
    data.extend_from_slice(PNG_GEN_KEYWORD.as_bytes());
    data.push(0);
    data.extend_from_slice(generation.as_bytes());
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(&data);
    let mut crc_input = b"tEXt".to_vec();
    crc_input.extend_from_slice(&data);
    chunk.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    assert!(
        png.len() > 12 && &png[png.len() - 8..png.len() - 4] == b"IEND",
        "png_tag_generation: PNG missing IEND"
    );
    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..png.len() - 12]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[png.len() - 12..]);
    out
}

/// Read back the generation tag, if any. `None` on malformed input or no tag
/// (legacy/foreign PNGs) — never panics.
#[must_use]
pub fn png_generation(png: &[u8]) -> Option<String> {
    if !png.starts_with(&PNG_SIG) || png.len() < 12 {
        return None;
    }
    let mut i = 8;
    while i + 8 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().ok()?) as usize;
        let typ = &png[i + 4..i + 8];
        if i + 8 + len + 4 > png.len() {
            return None;
        }
        if typ == b"tEXt" {
            let data = &png[i + 8..i + 8 + len];
            if let Some(z) = data.iter().position(|&b| b == 0) {
                if &data[..z] == PNG_GEN_KEYWORD.as_bytes() {
                    return Some(String::from_utf8_lossy(&data[z + 1..]).into_owned());
                }
            }
        }
        if typ == b"IEND" {
            break;
        }
        i += 8 + len + 4;
    }
    None
}

/// Generation binding parsed from a `.snap` description (`None` when absent).
fn snap_generation(snap_path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(snap_path).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(v) = line.trim().strip_prefix("description:") {
            let v = v.trim().trim_matches('"');
            let gen = v.strip_prefix(GEN_DESC_PREFIX)?;
            return gen.split_whitespace().next().map(str::to_string);
        }
    }
    None
}

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

/// Lenient gate used inside [`assert_screenshot_impl`]: legacy approvals without
/// any generation binding keep their per-artifact verdicts; bindings that are
/// ALL present but disagree fail.
fn check_consistent_lenient(
    dir: &Path,
    canonical: &str,
    png: &str,
) -> Result<(), ConsistencyError> {
    let c = snap_generation(&dir.join(format!("{canonical}.snap")));
    let p = snap_generation(&dir.join(format!("{png}.snap")));
    let t = std::fs::read(dir.join(format!("{png}.snap.png")))
        .ok()
        .and_then(|b| png_generation(&b));
    match (c, p, t) {
        (Some(c), Some(p), Some(t)) if c != p || p != t => Err(ConsistencyError(format!(
            "mixed compound baseline: canonical={c} png-meta={p} png-bytes={t}"
        ))),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Policy: evolving review vs frozen roots (I06)
// ---------------------------------------------------------------------------

/// Reference policy: evolving snapshots go through Insta review; frozen roots
/// are read-only directories of approved canonical+PNG files.
#[derive(Debug, Clone)]
pub enum Policy {
    /// Native Insta pending/review flow ([`assert_snapshot_impl`] /
    /// [`assert_screenshot_impl`]).
    Evolving,
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
pub fn check_frozen_snapshot(root: &Path, name: &str, screen: &Screen) -> Result<(), FrozenError> {
    check_scenario_name(name).map_err(FrozenError::InvalidName)?;
    let path = frozen_canonical_path(root, name);
    let approved = read_approved(&path)?;
    let approved_text = String::from_utf8(approved).map_err(|e| FrozenError::Corrupt {
        path: path.clone(),
        reason: format!("canonical is not UTF-8: {e}"),
    })?;
    let actual = insta_string(screen);
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
    if let Some(tag) = png_generation(&approved_png) {
        if tag != generation {
            return Err(FrozenError::Mismatch {
                name: name.to_string(),
                detail: format!("generation mismatch: canonical={generation} png-bytes={tag}"),
            });
        }
    }
    Ok(())
}

/// Assert canonical state against a frozen root. Panics on any [`FrozenError`].
pub fn assert_frozen_snapshot(root: &Path, name: &str, screen: &Screen) {
    if let Err(e) = check_frozen_snapshot(root, name, screen) {
        panic!("tuisnap frozen snapshot {name:?} failed: {e}");
    }
}

/// Assert canonical state plus PNG against a frozen root. Panics on any [`FrozenError`].
pub fn assert_frozen_screenshot(root: &Path, name: &str, screen: &Screen) {
    if let Err(e) = check_frozen_screenshot(root, name, screen) {
        panic!("tuisnap frozen screenshot {name:?} failed: {e}");
    }
}

/// Frozen roots reject acceptance unconditionally: always returns
/// [`FrozenError::AcceptRejected`] and writes nothing.
pub fn frozen_accept(root: &Path, name: &str) -> Result<(), FrozenError> {
    Err(FrozenError::AcceptRejected {
        root: root.to_path_buf(),
        name: name.to_string(),
    })
}

/// Policy-dispatched snapshot assertion (macro backend).
pub fn assert_snapshot_with_policy(
    policy: &Policy,
    name: &str,
    screen: &Screen,
    location: Location,
) {
    match policy {
        Policy::Evolving => assert_snapshot_impl(name, screen, location),
        Policy::Frozen { root } => assert_frozen_snapshot(root, name, screen),
    }
}

/// Policy-dispatched screenshot assertion (macro backend).
pub fn assert_screenshot_with_policy(
    policy: &Policy,
    name: &str,
    screen: &Screen,
    location: Location,
) {
    match policy {
        Policy::Evolving => assert_screenshot_impl(name, screen, location),
        Policy::Frozen { root } => assert_frozen_screenshot(root, name, screen),
    }
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

/// One imported four-artifact scenario.
#[derive(Debug, Clone)]
pub struct ImportedScenario {
    /// Scenario name (relative `/`-separated stem).
    pub name: String,
    /// `.ansi` bytes as UTF-8.
    pub ansi: String,
    /// `.txt` bytes as UTF-8.
    pub txt: String,
    /// `.html` bytes as UTF-8.
    pub html: String,
    /// `.png` bytes (decode-validated).
    pub png: Vec<u8>,
}

/// A read-only imported frozen tree: scenarios plus reported-but-tolerated entries.
#[derive(Debug, Clone, Default)]
pub struct FrozenTree {
    /// Fully validated scenarios, sorted by name.
    pub scenarios: Vec<ImportedScenario>,
    /// Reported, non-fatal entries: extra files (`extra file: <rel>`) and
    /// unknown embedded-frame fields (`unsupported field in <name>.html: <key>`).
    pub unsupported: Vec<String>,
}

/// Frozen-tree import failure: explicit, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// A scenario stem fails name validation.
    InvalidName(String),
    /// A scenario stem lacks one or more of the four artifacts.
    Incomplete {
        /// Scenario stem.
        name: String,
        /// Missing artifact extensions (without dot).
        missing: Vec<String>,
    },
    /// An artifact is present but unparsable (non-UTF-8 text, undecodable PNG).
    Corrupt {
        /// Artifact at fault.
        path: PathBuf,
        /// Why it is unusable.
        reason: String,
    },
    /// Filesystem failure (path context included).
    Io(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::InvalidName(e) => write!(f, "invalid scenario name: {e}"),
            ImportError::Incomplete { name, missing } => {
                write!(
                    f,
                    "scenario {name:?} incomplete, missing: {}",
                    missing.join(", ")
                )
            }
            ImportError::Corrupt { path, reason } => {
                write!(f, "artifact corrupt: {}: {reason}", path.display())
            }
            ImportError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Scenario-name validation, mirroring the `grouped` layout rules without
/// depending on its runtime: relative `/`-separated paths, no absolute paths,
/// no `.`/`..`/empty segments, no backslashes.
fn check_scenario_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("empty name".to_string());
    }
    if name.starts_with('/') || Path::new(name).is_absolute() {
        return Err(format!("{name:?}: absolute paths are not allowed"));
    }
    if name.contains('\\') {
        return Err(format!(
            "{name:?}: backslashes are not allowed (use `/` separators)"
        ));
    }
    for seg in name.split('/') {
        if seg.is_empty() {
            return Err(format!("{name:?}: empty path segment"));
        }
        if seg == ".." {
            return Err(format!("{name:?}: `..` segments are not allowed"));
        }
        if seg == "." {
            return Err(format!("{name:?}: `.` segments are not allowed"));
        }
    }
    Ok(())
}

fn collect_files(dir: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&d)
            .map_err(|e| ImportError::Io(format!("read {}: {e}", d.display())))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ImportError::Io(format!("read {}: {e}", d.display())))?
            .into_iter()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Known top-level keys of the frame JSON embedded in `.html` renders.
const KNOWN_FRAME_KEYS: &[&str] = &["version", "cols", "rows", "cells", "cursor", "provenance"];

/// Read-only import of a classic/grouped four-file tree (I07).
///
/// Groups `<stem>.{ansi,txt,png,html}` files (recursively, so nested `a/b`
/// scenarios work) into scenarios: stems are name-validated, every stem needs
/// all four artifacts, text parses as UTF-8, PNGs must decode. Anything else —
/// extra files, unknown embedded-frame JSON fields — is REPORTED in
/// [`FrozenTree::unsupported`], never fatal. Reads only; the tree is untouched.
pub fn import_frozen_v1(dir: &Path) -> Result<FrozenTree, ImportError> {
    const MEMBER_EXTS: [&str; 4] = ["ansi", "txt", "png", "html"];
    let mut members: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, PathBuf>,
    > = std::collections::BTreeMap::new();
    let mut unsupported = Vec::new();
    for path in collect_files(dir)? {
        let rel = path
            .strip_prefix(dir)
            .map_err(|e| ImportError::Io(format!("prefix {}: {e}", path.display())))?;
        // Join components with `/`: a literal backslash inside a filename stays a
        // backslash (and fails name validation) instead of becoming a separator.
        let rel: String = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let file = rel.rsplit('/').next().unwrap_or(&rel);
        if file.ends_with(".png.fidelity.json") {
            unsupported.push(format!("extra file: {rel}"));
            continue;
        }
        let (stem, ext) = match rel.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), e.to_string()),
            None => {
                unsupported.push(format!("extra file: {rel}"));
                continue;
            }
        };
        if !MEMBER_EXTS.contains(&ext.as_str()) {
            unsupported.push(format!("extra file: {rel}"));
            continue;
        }
        members.entry(stem).or_default().insert(ext, path);
    }
    let mut scenarios = Vec::new();
    for (stem, got) in &members {
        check_scenario_name(stem).map_err(ImportError::InvalidName)?;
        let missing: Vec<String> = MEMBER_EXTS
            .iter()
            .filter(|e| !got.contains_key(**e))
            .map(|e| (*e).to_string())
            .collect();
        if !missing.is_empty() {
            return Err(ImportError::Incomplete {
                name: stem.clone(),
                missing,
            });
        }
        let read_text = |ext: &str| -> Result<String, ImportError> {
            let p = &got[ext];
            let bytes = std::fs::read(p)
                .map_err(|e| ImportError::Io(format!("read {}: {e}", p.display())))?;
            String::from_utf8(bytes).map_err(|e| ImportError::Corrupt {
                path: p.clone(),
                reason: format!("not UTF-8: {e}"),
            })
        };
        let png_path = &got["png"];
        let png = std::fs::read(png_path)
            .map_err(|e| ImportError::Io(format!("read {}: {e}", png_path.display())))?;
        image::load_from_memory(&png).map_err(|e| ImportError::Corrupt {
            path: png_path.clone(),
            reason: format!("PNG does not decode: {e}"),
        })?;
        let html = read_text("html")?;
        report_embedded_fields(stem, &html, &mut unsupported);
        scenarios.push(ImportedScenario {
            name: stem.clone(),
            ansi: read_text("ansi")?,
            txt: read_text("txt")?,
            html,
            png,
        });
    }
    unsupported.sort();
    Ok(FrozenTree {
        scenarios,
        unsupported,
    })
}

/// Report unknown fields of the frame JSON embedded in an `.html` render.
/// Findings are non-fatal notes in `unsupported`.
fn report_embedded_fields(stem: &str, html: &str, unsupported: &mut Vec<String>) {
    const OPEN: &str = "<script type=\"application/json\">";
    const CLOSE: &str = "</script>";
    let Some(after) = html.split_once(OPEN).map(|(_, tail)| tail) else {
        unsupported.push(format!("no embedded frame JSON in {stem}.html"));
        return;
    };
    let Some((json_text, _)) = after.split_once(CLOSE) else {
        unsupported.push(format!("truncated embedded frame JSON in {stem}.html"));
        return;
    };
    let value: serde_json::Value = match serde_json::from_str(json_text) {
        Ok(v) => v,
        Err(e) => {
            unsupported.push(format!(
                "unparsable embedded frame JSON in {stem}.html: {e}"
            ));
            return;
        }
    };
    let serde_json::Value::Object(map) = value else {
        unsupported.push(format!(
            "embedded frame JSON in {stem}.html is not an object"
        ));
        return;
    };
    let mut unknown: Vec<&str> = map
        .keys()
        .filter(|k| !KNOWN_FRAME_KEYS.contains(&k.as_str()))
        .map(String::as_str)
        .collect();
    unknown.sort();
    for key in unknown {
        unsupported.push(format!("unsupported field in {stem}.html: {key}"));
    }
}
