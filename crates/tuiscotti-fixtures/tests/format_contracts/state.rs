//! Canonical JSON, hidden data, generations, manifest, pipes (split from `format_contracts.rs`; shared helpers live in the root).

use super::capture::{self as cap, renderer};
use super::common::{self, menu_frame, streams_frame};
use super::menu_bundle;
use tuiscotti_fixtures::driver::Scenario;
use tuiscotti_fixtures::views::Theme;
use tuiscotti_render::formats::{
    ansi_normalized, canonical_json, capture_all, changed_pixels, generations_match,
    parse_canonical, pipe_projection, pipe_strict, require_same_generation, txt_projection,
};

/// Collect `dir`'s files as `root`-relative strings for manifest coverage.
fn collect(
    dir: &std::path::Path,
    root: &std::path::Path,
    out: &mut Vec<String>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, root, out)?;
        } else {
            out.push(path.strip_prefix(root)?.to_string_lossy().into_owned());
        }
    }
    Ok(())
}

// --- Canonical JSON ---------------------------------------------------------

#[test]
fn canonical_json_round_trips_with_version_and_provenance() {
    let frame = menu_frame(10, 4, Theme::Dark, Scenario::Empty);
    let json = canonical_json(&frame).expect("serialize");
    assert_eq!(
        json,
        cap::read_expected("menu-empty-10x4.json").expect("committed baseline")
    );
    let back = parse_canonical(&json).expect("parse");
    assert_eq!(back.to_json(), json, "lossless round-trip");
    tuiscotti_render::formats::json::assert_provenance_complete(&back).expect("provenance");
    assert_eq!(
        back.version,
        tuiscotti_render::formats::CANONICAL_JSON_VERSION
    );
}

#[test]
fn canonical_json_rejects_wrong_version_and_corruption() {
    let frame = menu_frame(10, 4, Theme::Dark, Scenario::Empty);
    let mut bad_version =
        serde_json::from_str::<serde_json::Value>(&frame.to_json()).expect("json");
    bad_version["version"] = serde_json::Value::from(99);
    assert!(
        parse_canonical(&bad_version.to_string()).is_err(),
        "wrong version rejected"
    );
    assert!(parse_canonical("{not json").is_err(), "corruption rejected");
    assert!(
        parse_canonical("{\"version\":3}").is_err(),
        "truncation rejected"
    );
}

// --- Hidden data ------------------------------------------------------------

#[test]
fn hidden_cells_hide_pixels_but_keep_source_text() {
    use tuiscotti::frame::{Cell, Mods};
    let mut frame = menu_frame(20, 5, Theme::Dark, Scenario::Empty);
    let mut hidden = Cell::blank(2, 2);
    hidden.symbol = "X".to_string();
    hidden.mods = Mods {
        hidden: true,
        ..Mods::default()
    };
    frame.set(hidden);
    let mut shown = frame.clone();
    let mut cell = Cell::blank(2, 2);
    cell.symbol = " ".to_string();
    shown.set(cell);
    // Source symbol survives in text projections (concealment is not redaction).
    assert!(txt_projection(&frame).contains('X'));
    assert!(ansi_normalized(&frame).contains('X'));
    assert!(canonical_json(&frame).expect("json").contains("\"X\""));
    // Pixels match the blank cell exactly.
    let px_hidden = renderer()
        .expect("renderer")
        .render_png(&frame)
        .expect("render");
    let px_shown = renderer()
        .expect("renderer")
        .render_png(&shown)
        .expect("render");
    assert!(
        changed_pixels(&px_hidden, &px_shown)
            .expect("diff")
            .is_empty(),
        "hidden glyph draws no ink"
    );
}

// --- Generations ------------------------------------------------------------

#[test]
fn every_capture_exports_one_identifiable_generation() {
    let bundle = menu_bundle().expect("capture bundle");
    let other = {
        let frame = streams_frame(60, 12, Theme::Dark, false);
        capture_all(&mut renderer().expect("renderer"), &frame, "streams").expect("capture")
    };
    assert_eq!(
        bundle.generation.frame_digest,
        menu_frame(40, 10, Theme::Dark, Scenario::Demo).digest()
    );
    assert!(!bundle.generation.id.is_empty());
    assert!(generations_match(&bundle.generation, &bundle.generation));
    assert!(!generations_match(&bundle.generation, &other.generation));
    require_same_generation(&bundle.generation, &other.generation).expect_err("mixed generations");
    // Deterministic: same frame + profile always yields the same id.
    let again = menu_bundle().expect("capture bundle");
    assert_eq!(bundle.generation, again.generation);
}

// --- SHA256SUMS manifest ------------------------------------------------------

#[test]
fn sha256sums_manifest_pins_every_approval() {
    // `tests/SHA256SUMS` (see `cargo xtask fixtures --bless-manifest`) pins every
    // committed approval byte: sorted `sha256sum` over `fixtures/expected`
    // + `visual/approved`. Any bless/unbless without a manifest refresh
    // fails here. Hashes recompute with the render crate's public SHA-256
    // helper (no new dependency just for the check).
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let manifest = std::fs::read_to_string(root.join("SHA256SUMS")).expect("manifest ships");
    let mut entries: Vec<(&str, &str)> = Vec::new();
    for (n, line) in manifest.lines().enumerate() {
        let (hash, path) = line
            .split_once("  ")
            .unwrap_or_else(|| panic!("line {}: not `sha256sum` format", n + 1));
        assert_eq!(hash.len(), 64, "line {}: short hash", n + 1);
        let bytes = std::fs::read(root.join(path))
            .unwrap_or_else(|_| panic!("line {}: {path} listed but missing", n + 1));
        assert_eq!(
            tuiscotti_render::profile::font_sha256(&bytes),
            hash,
            "{path}: bytes drifted from the manifest"
        );
        entries.push((hash, path));
    }
    assert!(!entries.is_empty(), "manifest pins nothing");
    let mut paths: Vec<&str> = entries.iter().map(|(_, p)| *p).collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted, "manifest entries are sorted");
    // Exact coverage: every approval pinned, nothing extra pinned.
    let mut actual = Vec::new();
    collect(&root.join("fixtures/expected"), &root, &mut actual).expect("collect expected");
    collect(&root.join("visual/approved"), &root, &mut actual).expect("collect approved");
    actual.sort_unstable();
    paths.sort_unstable();
    assert_eq!(paths, actual, "manifest covers exactly the approvals dirs");
}

// --- Pipes ------------------------------------------------------------------

#[test]
fn pipe_projection_accounts_invalid_utf8_and_truncation() {
    let raw = common::read_data("invalid-utf8.bin").expect("fixture data");
    let full = pipe_projection(&raw, 1024).expect("project");
    assert_eq!(
        full.text,
        cap::read_expected("pipe-invalid-utf8.txt").expect("committed baseline")
    );
    assert_eq!(full.input_bytes, raw.len());
    assert!(!full.truncated);
    assert!(full.replacements > 0, "invalid sequences counted");
    assert!(full.lossy());
    assert!(!full.id.is_empty(), "pipe capture identifiable");
    // Strict decoding names the first bad offset instead.
    let strict = pipe_strict(&raw).expect_err("strict rejects invalid UTF-8");
    assert_eq!(strict.offset, Some(13));
    // Truncation is explicit and char-boundary safe.
    let cut = pipe_projection(&raw, 20).expect("project");
    assert!(cut.truncated);
    assert!(cut.kept_bytes <= 20);
    assert_ne!(cut.text, full.text);
    // Empty input is clean, not an error.
    let empty = pipe_projection(&common::read_data("empty.txt").expect("fixture data"), 1024)
        .expect("project");
    assert!(!empty.lossy());
    assert_eq!(empty.text, "");
}
