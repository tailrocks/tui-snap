//! Format contracts: the six distinct projections on pure views.
//!
//! ASCII (7-bit diagnostic) vs TXT (plain Unicode) vs ANSI (normalized SGR)
//! vs PNG (opaque RGB pixels) vs HTML (static offline, no JavaScript) vs
//! canonical JSON (versioned state + provenance). Includes the negative
//! battery: loss/truncation reporting, ANSI-only changes, whitespace,
//! hidden data, HTML injection, and mixed generations.

#[path = "common/mod.rs"]
mod common;

use common::{menu_frame, protocol_frame, renderer, streams_frame};
use tuiscotti_fixtures::driver::Scenario;
use tuiscotti_fixtures::views::Theme;
use tuiscotti_render::formats::{
    ansi_normalized, ascii_projection, assert_no_escapes, assert_normalized_sgr, assert_opaque_rgb,
    assert_seven_bit, assert_static_offline, canonical_json, capture_all, changed_pixels,
    generation_for, generations_match, html_static, parse_canonical, pipe_projection, pipe_strict,
    require_same_generation, txt_projection,
};

/// Capture every format of the menu demo in one bundle.
fn menu_bundle() -> tuiscotti_render::formats::CaptureBundle {
    let frame = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    capture_all(&mut renderer(), &frame, "menu demo").expect("capture")
}

// --- ASCII: 7-bit diagnostic ------------------------------------------------

#[test]
fn ascii_is_seven_bit_with_exact_substitution_accounting() {
    let frame = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    let ascii = ascii_projection(&frame);
    assert_seven_bit(&ascii.text).expect("7-bit output");
    assert!(ascii.lossy(), "box borders must count as substitutions");
    assert!(
        ascii.substitutions.len() > 20,
        "every border cell recorded, got {}",
        ascii.substitutions.len()
    );
    for sub in &ascii.substitutions {
        assert_seven_bit(&sub.replacement).expect("7-bit replacement");
        assert_ne!(sub.original, sub.replacement);
    }
    // Geometry preserved: same row count, same display width per row.
    assert_eq!(ascii.text.lines().count(), 10);
    assert_eq!(
        ascii.text,
        common::read_expected("menu-demo-40x10.ascii.txt")
    );
}

#[test]
fn ascii_lossless_only_when_nothing_substituted() {
    let frame = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    let ascii = ascii_projection(&frame);
    assert!(
        ascii.lossless_text().is_none(),
        "lossy ASCII has no lossless text"
    );
    // Pure-ASCII content projects loss-free.
    let plain = tuiscotti::ratatui::draw_frame(12, 3, common::prov("ascii"), |f| {
        use ratatui::widgets::Paragraph;
        f.render_widget(Paragraph::new("hello ascii"), f.area());
    });
    let artifact = ascii_projection(&plain);
    assert!(!artifact.lossy());
    assert_eq!(artifact.lossless_text(), Some(artifact.text.as_str()));
    assert_seven_bit(&artifact.text).expect("7-bit");
}

#[test]
fn ascii_reports_wide_and_combining_loss_per_cell() {
    let frame = streams_frame(60, 12, Theme::Dark, false);
    let ascii = ascii_projection(&frame);
    assert_seven_bit(&ascii.text).expect("7-bit");
    assert!(ascii.lossy());
    // Wide CJK lead cells emit exactly two ASCII columns.
    let cjk: Vec<_> = ascii
        .substitutions
        .iter()
        .filter(|s| s.original.chars().any(|c| c == '日' || c == '本'))
        .collect();
    assert!(!cjk.is_empty(), "CJK substitutions recorded");
    for sub in cjk {
        assert_eq!(sub.replacement.len(), 2, "wide cell keeps 2 columns");
    }
}

// --- TXT: plain Unicode -----------------------------------------------------

#[test]
fn txt_is_plain_unicode_matching_baselines() {
    let menu = txt_projection(&menu_frame(40, 10, Theme::Dark, Scenario::Demo));
    assert_no_escapes(&menu).expect("no escapes");
    assert_eq!(menu, common::read_expected("menu-demo-40x10.txt"));
    let streams = txt_projection(&streams_frame(60, 12, Theme::Dark, false));
    assert_no_escapes(&streams).expect("no escapes");
    assert_eq!(streams, common::read_expected("streams-demo-60x12.txt"));
    let protocol = txt_projection(&protocol_frame(50, 12, Theme::Dark, false));
    assert_no_escapes(&protocol).expect("no escapes");
    assert_eq!(protocol, common::read_expected("protocol-demo-50x12.txt"));
}

#[test]
fn txt_whitespace_policy_trims_tails_keeps_interior() {
    let frame = streams_frame(60, 12, Theme::Dark, false);
    let txt = txt_projection(&frame);
    for line in txt.lines() {
        assert!(!line.ends_with(' '), "no trailing blanks: {line:?}");
    }
    assert!(txt.contains("padded   cells   here"), "interior kept");
    assert!(!txt.ends_with('\n'), "no trailing newline");
    assert_eq!(txt.lines().count(), 12, "one line per grid row");
}

// --- ANSI: normalized SGR ---------------------------------------------------

#[test]
fn ansi_is_normalized_not_raw_transcript() {
    let bundle = menu_bundle();
    assert_normalized_sgr(&bundle.ansi).expect("normalized SGR only");
    // Content rides along; style changes the bytes.
    assert!(bundle.ansi.contains("autosave"));
    assert!(bundle.ansi.contains("\x1b["), "SGR runs present");
    let light = ansi_normalized(&menu_frame(40, 10, Theme::Light, Scenario::Demo));
    assert_normalized_sgr(&light).expect("normalized SGR only");
    assert_ne!(bundle.ansi, light, "theme change moves ANSI bytes");
}

#[test]
fn ansi_matches_committed_approvals_for_all_views() {
    for (name, frame) in [
        (
            "menu-demo-40x10",
            menu_frame(40, 10, Theme::Dark, Scenario::Demo),
        ),
        (
            "streams-demo-60x12",
            streams_frame(60, 12, Theme::Dark, false),
        ),
        (
            "protocol-demo-50x12",
            protocol_frame(50, 12, Theme::Dark, false),
        ),
    ] {
        let bundle = capture_all(&mut renderer(), &frame, name).expect("capture");
        let approved = common::read_expected(&format!("{name}.ansi"));
        assert_normalized_sgr(&approved).expect("committed ANSI stays normalized");
        assert_eq!(bundle.ansi, approved, "{name}: ANSI drifted");
    }
}

#[test]
fn ansi_only_style_change_moves_ansi_but_not_txt() {
    let dark = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    let light = menu_frame(40, 10, Theme::Light, Scenario::Demo);
    assert_eq!(
        txt_projection(&dark),
        txt_projection(&light),
        "same content: TXT still"
    );
    assert_ne!(
        ansi_normalized(&dark),
        ansi_normalized(&light),
        "new palette: ANSI moves"
    );
    // Underline-across-spaces is style-only too.
    let plain = streams_frame(60, 12, Theme::Dark, false);
    assert_eq!(txt_projection(&plain).lines().count(), 12);
}

// --- PNG: opaque pixels -----------------------------------------------------

#[test]
fn png_is_opaque_rgb_and_deterministic() {
    let frame = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    let mut first = renderer();
    let a = first.render_png(&frame).expect("render");
    let info = assert_opaque_rgb(&a).expect("opaque RGB evidence");
    assert_eq!(
        (info.width, info.height),
        common::profile().image_size(40, 10)
    );
    let mut second = renderer();
    let b = second.render_png(&frame).expect("render");
    assert_eq!(a, b, "deterministic bytes for identical frame + profile");
}

#[test]
fn changed_pixels_beats_reencode_assumptions() {
    let a = menu_frame(40, 10, Theme::Dark, Scenario::Demo);
    let mut model_b = tuiscotti_fixtures::views::menu::Model::demo(Theme::Dark);
    model_b.selected = 1;
    let b = tuiscotti::ratatui::draw_frame(40, 10, common::prov("menu-view"), |f| {
        tuiscotti_fixtures::views::menu::render(f, &model_b);
    });
    let pa = renderer().render_png(&a).expect("render");
    let pa2 = renderer().render_png(&a).expect("render");
    assert!(
        changed_pixels(&pa, &pa2).expect("diff").is_empty(),
        "re-encode of identical pixels: zero changed pixels"
    );
    let pb = renderer().render_png(&b).expect("render");
    let changed = changed_pixels(&pa, &pb).expect("diff");
    assert!(!changed.is_empty(), "selection move changes decoded pixels");
    let total = common::profile().image_size(40, 10);
    assert!(
        changed.len() < (total.0 * total.1) as usize / 2,
        "change is localized, not a full repaint: {} px",
        changed.len()
    );
    // Dimension mismatch is an error, never a diff.
    let tiny = renderer()
        .render_png(&menu_frame(10, 4, Theme::Dark, Scenario::Empty))
        .expect("render");
    assert!(changed_pixels(&pa, &tiny).is_err());
}

// --- HTML: static offline, no JavaScript ------------------------------------

#[test]
fn html_is_static_offline_with_png_embed() {
    let bundle = menu_bundle();
    assert_static_offline(&bundle.html).expect("static offline");
    assert!(
        !bundle.html.to_lowercase().contains("<script"),
        "no script at all"
    );
    assert!(
        bundle.html.contains("data:image/png;base64,"),
        "offline PNG embed"
    );
    assert!(bundle.html.contains("autosave"), "selectable text present");
    assert!(
        bundle.html.contains(&bundle.generation.id),
        "generation labeled"
    );
}

#[test]
fn html_matches_committed_approvals_for_all_views() {
    for (name, frame) in [
        (
            "menu-demo-40x10",
            menu_frame(40, 10, Theme::Dark, Scenario::Demo),
        ),
        (
            "streams-demo-60x12",
            streams_frame(60, 12, Theme::Dark, false),
        ),
        (
            "protocol-demo-50x12",
            protocol_frame(50, 12, Theme::Dark, false),
        ),
    ] {
        let bundle = capture_all(&mut renderer(), &frame, name).expect("capture");
        let approved = common::read_expected(&format!("{name}.html"));
        assert_static_offline(&approved).expect("committed HTML stays static offline");
        assert_eq!(bundle.html, approved, "{name}: HTML drifted");
    }
}

#[test]
fn html_injection_is_escaped_not_executed() {
    let mut model = tuiscotti_fixtures::views::menu::Model::demo(Theme::Dark);
    model.error = Some("</script><script>alert(1)</script>".to_string());
    let frame = tuiscotti::ratatui::draw_frame(48, 14, common::prov("inject"), |f| {
        tuiscotti_fixtures::views::menu::render(f, &model);
    });
    let generation = generation_for(&frame, "test").id;
    let html = html_static(
        &frame,
        &common::profile(),
        "\"><img src=x onerror=alert(1)>",
        None,
        &generation,
    );
    assert_static_offline(&html).expect("injection neutralized");
    assert!(
        !html.to_lowercase().contains("<script"),
        "no script element smuggled"
    );
    assert!(html.contains("&lt;script&gt;"), "payload escaped");
    // The validator itself bites: raw smuggled markup is rejected.
    assert_static_offline("<p>x</p><script>alert(1)</script>").expect_err("raw script caught");
    assert_static_offline("<img src=\"x\" onerror=\"alert(1)\">").expect_err("handler caught");
    assert_static_offline("<a href=\"http://evil.example\">x</a>").expect_err("external caught");
}

// --- Canonical JSON ---------------------------------------------------------

#[test]
fn canonical_json_round_trips_with_version_and_provenance() {
    let frame = menu_frame(10, 4, Theme::Dark, Scenario::Empty);
    let json = canonical_json(&frame).expect("serialize");
    assert_eq!(json, common::read_expected("menu-empty-10x4.json"));
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
    let px_hidden = renderer().render_png(&frame).expect("render");
    let px_shown = renderer().render_png(&shown).expect("render");
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
    let bundle = menu_bundle();
    let other = {
        let frame = streams_frame(60, 12, Theme::Dark, false);
        capture_all(&mut renderer(), &frame, "streams").expect("capture")
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
    let again = menu_bundle();
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
    fn collect(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("approvals dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                collect(&path, root, out);
            } else {
                out.push(
                    path.strip_prefix(root)
                        .expect("under tests/")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    let mut actual = Vec::new();
    collect(&root.join("fixtures/expected"), &root, &mut actual);
    collect(&root.join("visual/approved"), &root, &mut actual);
    actual.sort_unstable();
    paths.sort_unstable();
    assert_eq!(paths, actual, "manifest covers exactly the approvals dirs");
}

// --- Pipes ------------------------------------------------------------------

#[test]
fn pipe_projection_accounts_invalid_utf8_and_truncation() {
    let raw = common::read_data("invalid-utf8.bin");
    let full = pipe_projection(&raw, 1024).expect("project");
    assert_eq!(full.text, common::read_expected("pipe-invalid-utf8.txt"));
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
    assert!(cut.text != full.text);
    // Empty input is clean, not an error.
    let empty = pipe_projection(&common::read_data("empty.txt"), 1024).expect("project");
    assert!(!empty.lossy());
    assert_eq!(empty.text, "");
}
