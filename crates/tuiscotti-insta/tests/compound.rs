//! G6 compound approval tests: same-sample canonical+PNG binding.
//!
//! - Canonical-identical/render-different repro: one [`Screen`] rendered
//!   under two profiles shares its canonical text but yields different PNG
//!   bytes, and the decoded-pixel comparison fails (pixel equality is never
//!   inferred from cells).
//! - Partial acceptance: a canonical approval without its PNG partner (or
//!   with a mismatched generation) fails the strict [`check_consistent`]
//!   gate instead of passing half-blind.
//! - Evidence precedes failure: [`assert_screenshot!`] writes all four
//!   artifacts before the Insta assertions can fail.
//! - Render identity: snapshot descriptions record the profile, renderer
//!   version, and alpha policy the PNG verdict depends on.

use std::fs;
use std::path::Path;

use tuiscotti_core::screen::{Screen, ScreenError};
use tuiscotti_insta::assert::{
    AttemptIdentity, EvidenceId, Location, Policy, check_consistent, frame_from_screen,
    generation_id, png_generation, png_tag_generation, render_sample, resolve_snapshot_identity,
    resolve_snapshot_identity_in, sample_binding, sanitize_segment,
};
use tuiscotti_insta::insta_proto::insta_string;
use tuiscotti_render::diff::{AlphaPolicy, compare_png_with_alpha};
use tuiscotti_render::profile::{Profile, VENDORED_FACES};
use tuiscotti_render::render::Renderer;

fn styled_screen() -> Result<Screen, ScreenError> {
    use tuiscotti_core::frame::{Cell, Color, Cursor, CursorStyle, Mods};
    let cells = vec![
        Cell {
            x: 0,
            y: 0,
            symbol: "A".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Indexed(1),
            bg: Color::Default,
            mods: Mods {
                bold: true,
                ..Mods::default()
            },
            underline_color: Color::Default,
        },
        Cell {
            x: 1,
            y: 0,
            symbol: "b".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Default,
            bg: Color::Default,
            mods: Mods::default(),
            underline_color: Color::Default,
        },
    ];
    Screen::validate(
        2,
        1,
        0,
        0,
        cells,
        Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: CursorStyle::Block,
            blinking: false,
        },
    )
}

#[test]
fn canonical_identical_render_different_repro() {
    let screen = styled_screen().expect("valid test screen");
    let canonical = insta_string(&screen);

    // Same sample through the pinned pipeline.
    let sample = render_sample(&screen).expect("render sample");

    // Same screen through a DIFFERENT profile (half the raster scale).
    let frame = frame_from_screen(&screen);
    let other = Profile {
        scale: 1,
        ..Profile::default_profile()
    };
    let mut renderer =
        Renderer::new(&other, &VENDORED_FACES).expect("renderer with vendored faces");
    let alt_image = renderer.render(&frame).expect("render frame");

    // Canonical text is identical (same screen) ...
    assert_eq!(canonical, insta_string(&screen));
    // ... but the pixels differ, and the decoded-pixel comparison — the same
    // function the PNG comparator delegates to — fails loudly.
    assert_ne!(sample.png, alt_image.png);
    let verdict = compare_png_with_alpha(&sample.png, &alt_image.png, AlphaPolicy::StraightRgba)
        .expect("compare pngs");
    assert!(
        !verdict.pixels_equal,
        "different renders must not compare equal"
    );
}

#[test]
fn same_sample_renders_deterministically() {
    let screen = styled_screen().expect("valid test screen");
    let first = render_sample(&screen).expect("render sample");
    let second = render_sample(&screen).expect("render sample");
    assert_eq!(first.canonical, second.canonical);
    assert_eq!(first.png, second.png);
    assert_eq!(first.ansi, second.ansi);
    assert_eq!(first.txt, second.txt);
    assert_eq!(first.html, second.html);
}

fn write_text_snap(dir: &Path, name: &str, generation: &str, body: &str) -> std::io::Result<()> {
    let content = format!(
        "---\nsource: tests/compound.rs\ndescription: tuiscotti generation {generation}\nexpression: canonical\n---\n{body}"
    );
    fs::write(dir.join(format!("{name}.snap")), content)
}

fn write_binary_snap(
    dir: &Path,
    name: &str,
    generation: &str,
    sidecar: &[u8],
) -> std::io::Result<()> {
    let meta = format!(
        "---\nsource: tests/compound.rs\ndescription: tuiscotti generation {generation}\nexpression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
    );
    fs::write(dir.join(format!("{name}.snap")), meta)?;
    fs::write(dir.join(format!("{name}.snap.png")), sidecar)
}

#[test]
fn partial_acceptance_fails_the_strict_gate() {
    let screen = styled_screen().expect("valid test screen");
    let canonical = insta_string(&screen);
    let generation = generation_id(&canonical);
    let sample = render_sample(&screen).expect("render sample");
    let tagged = png_tag_generation(&sample.png, &generation);

    // Complete compound baseline: passes.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "shot", &generation, &canonical).expect("write text snap");
    write_binary_snap(tmp.path(), "shot-img", &generation, &tagged).expect("write binary snap");
    check_consistent(tmp.path(), "shot", "shot-img").expect("consistent compound baseline passes");

    // Canonical accepted, PNG never approved: strict gate fails (no
    // half-blind pass).
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "shot", &generation, &canonical).expect("write text snap");
    let err = check_consistent(tmp.path(), "shot", "shot-img")
        .expect_err("missing PNG sidecar must fail");
    assert!(err.to_string().contains("missing"), "{err}");

    // Canonical re-approved alone after a change (mixed generations): strict
    // gate names the mismatch instead of comparing across samples.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "shot", "aaa", &canonical).expect("write text snap");
    write_binary_snap(tmp.path(), "shot-img", "bbb", &tagged).expect("write binary snap");
    let err =
        check_consistent(tmp.path(), "shot", "shot-img").expect_err("mixed generations must fail");
    assert!(err.to_string().contains("mixed compound baseline"), "{err}");
}

#[test]
fn render_identity_is_recorded_in_descriptions() {
    let settings = tuiscotti_insta::assert::snapshot_settings(
        Path::new("snaps"),
        Location {
            file: "tests/compound.rs",
            line: 1,
        },
        "abc123",
    );
    let description = settings.description().unwrap_or_default();
    assert!(
        description.contains("tuiscotti generation abc123"),
        "{description}"
    );
    assert!(description.contains("tuiscotti-default"), "{description}");
    assert!(
        description.contains(&format!(
            "rv{}",
            tuiscotti_render::profile::RENDERER_VERSION
        )),
        "{description}"
    );
    assert!(description.contains("straight-rgba"), "{description}");
    assert!(description.contains("tests/compound.rs"), "{description}");
}

/// Whether Insta would bless in place (then the failure-ordering test has no
/// failure to order). Mirrors `tuiscotti/tests/common` logic.
fn insta_updates_in_place() -> bool {
    matches!(
        std::env::var("INSTA_UPDATE").ok().as_deref(),
        Some("always" | "1" | "unseen" | "force")
    )
}

/// Whether Insta writes `.snap.new` pendings (and fails) on mismatch.
/// Mirrors `tuiscotti/tests/common` + insta 1.48 resolution.
fn insta_writes_new_files() -> bool {
    match std::env::var("INSTA_UPDATE").ok().as_deref() {
        Some("new") => true,
        Some("auto" | "") | None => !is_ci(),
        Some(_) => false,
    }
}

fn is_ci() -> bool {
    match std::env::var("CI").ok().as_deref() {
        Some("false" | "0" | "") => false,
        None => std::env::var("TF_BUILD").is_ok(),
        Some(_) => true,
    }
}

/// Recursively collect files under `dir` as `(relative, absolute)` pairs.
fn collect_files(dir: &Path) -> Result<Vec<(String, std::path::PathBuf)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries: Vec<std::path::PathBuf> = fs::read_dir(&d)
            .map_err(|e| format!("read {}: {e}", d.display()))?
            .map(|e| e.map(|e| e.path()).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path
                    .strip_prefix(dir)
                    .map_err(|e| format!("strip {}: {e}", path.display()))?
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, path));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The single published bundle directory under `evidence` (exactly one
/// `complete.json` must exist).
fn single_bundle(evidence: &Path) -> Result<std::path::PathBuf, String> {
    let files = collect_files(evidence)?;
    let completes: Vec<_> = files
        .iter()
        .filter(|(rel, _)| rel.ends_with("/complete.json"))
        .collect();
    assert_eq!(
        completes.len(),
        1,
        "exactly one complete bundle expected, files: {:?}",
        files.iter().map(|(r, _)| r).collect::<Vec<_>>()
    );
    completes[0]
        .1
        .parent()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| "bundle parent".to_string())
}

/// Assert the bundle at `bundle` carries the sample rendered from `screen`
/// under `scenario`/`png_stem`, with a manifest binding matching the tagged
/// image. Returns the manifest binding for approval crafting.
fn assert_bundle_matches_sample(
    bundle: &Path,
    screen: &Screen,
    scenario: &str,
    png_stem: &str,
) -> Result<String, String> {
    for name in [
        "canonical.txt",
        "image.png",
        "sample.ansi",
        "sample.txt",
        "sample.html",
        "manifest.json",
        "complete.json",
    ] {
        assert!(bundle.join(name).is_file(), "missing bundle file {name}");
    }
    let sample = render_sample(screen).map_err(|e| e.to_string())?;
    assert_eq!(
        fs::read_to_string(bundle.join("canonical.txt")).map_err(|e| e.to_string())?,
        sample.canonical
    );
    assert_eq!(
        fs::read_to_string(bundle.join("sample.ansi")).map_err(|e| e.to_string())?,
        sample.ansi
    );
    assert_eq!(
        fs::read_to_string(bundle.join("sample.txt")).map_err(|e| e.to_string())?,
        sample.txt
    );
    assert_eq!(
        fs::read_to_string(bundle.join("sample.html")).map_err(|e| e.to_string())?,
        sample.html
    );
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(bundle.join("manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(manifest["schema"], "tuiscotti-evidence-bundle/1");
    let binding = manifest["binding"]
        .as_str()
        .ok_or_else(|| "manifest binding".to_string())?
        .to_string();
    assert!(binding.starts_with("v2-"), "{binding}");
    assert_eq!(
        manifest["generation"].as_str(),
        Some(generation_id(&sample.canonical).as_str())
    );
    let image = fs::read(bundle.join("image.png")).map_err(|e| e.to_string())?;
    assert_eq!(png_generation(&image).as_deref(), Some(binding.as_str()));
    assert_eq!(
        manifest["snapshot"]["canonical"].as_str(),
        Some(scenario)
    );
    assert_eq!(manifest["snapshot"]["png"].as_str(), Some(png_stem));
    Ok(binding)
}

fn panic_message(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "<non-string panic>".to_string()
    }
}

#[test]
fn evidence_bundle_is_published_before_failure() {
    if insta_updates_in_place() {
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let snaps = tmp.path().join("snaps");
    let evidence = tmp.path().join("evidence");
    fs::create_dir(&snaps).expect("create snaps dir");
    fs::create_dir(&evidence).expect("create evidence dir");
    let policy = Policy::EvolvingIn {
        snapshots: snaps.clone(),
        evidence: evidence.clone(),
    };
    // No approvals exist: the assertion MUST fail in every non-blessing mode.
    let screen = styled_screen().expect("valid test screen");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuiscotti_insta::assert_screenshot!("g6_evidence_first", &screen, &policy);
    }));
    assert!(outcome.is_err(), "unapproved snapshot must fail");
    // ... but the FULL candidate bundle was already published: canonical +
    // tagged image + all renders + manifest + completion marker, partitioned
    // by package/test/scenario x run/attempt.
    let bundle = single_bundle(&evidence).expect("single bundle");
    let rel = bundle
        .strip_prefix(&evidence)
        .expect("strip prefix")
        .to_string_lossy()
        .replace('\\', "/");
    let segs: Vec<&str> = rel.split('/').collect();
    assert_eq!(segs.len(), 5, "package/test/scenario/run/attempt: {rel}");
    assert_eq!(segs[0], "tuiscotti-insta", "package segment: {rel}");
    assert!(
        segs[1].contains("evidence_bundle_is_published_before_failure"),
        "test segment names the test: {rel}"
    );
    assert_eq!(segs[2], "g6_evidence_first", "scenario segment: {rel}");
    assert!(segs[3].starts_with("run-"), "run segment: {rel}");
    assert!(
        segs[4].starts_with("attempt-"),
        "attempt segment: {rel}"
    );
    assert_bundle_matches_sample(&bundle, &screen, "g6_evidence_first", "g6_evidence_first-img")
        .expect("bundle matches sample");
    // Atomic publication: no temp leftovers anywhere under the root.
    for (rel, _) in collect_files(&evidence).expect("collect files") {
        assert!(
            !rel.contains(".tmp-"),
            "no temp leftovers may survive: {rel}"
        );
    }
}

#[test]
fn first_run_publishes_both_pendings_in_one_cycle() {
    if insta_updates_in_place() || !insta_writes_new_files() {
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let snaps = tmp.path().join("snaps");
    let evidence = tmp.path().join("evidence");
    fs::create_dir(&snaps).expect("create snaps dir");
    fs::create_dir(&evidence).expect("create evidence dir");
    let policy = Policy::EvolvingIn {
        snapshots: snaps.clone(),
        evidence: evidence.clone(),
    };
    let screen = styled_screen().expect("valid test screen");
    // ONE call, no approvals: both pendings must exist afterwards — the
    // canonical failure must not suppress the PNG assertion.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuiscotti_insta::assert_screenshot!("g6_both_pendings", &screen, &policy);
    }));
    assert!(outcome.is_err(), "unapproved snapshot must fail");
    assert!(
        snaps.join("g6_both_pendings.snap.new").is_file(),
        "canonical pending missing"
    );
    assert!(
        snaps.join("g6_both_pendings-img.snap.new").is_file(),
        "png pending missing after a single run"
    );
    // The aggregated failure names both artifacts AND the candidate bundle.
    let msg = panic_message(&*outcome.expect_err("outcome is an error"));
    assert!(msg.contains("canonical snapshot failed"), "{msg}");
    assert!(msg.contains("png snapshot failed"), "{msg}");
    assert!(msg.contains("candidate bundle:"), "{msg}");
    // Approving the published bundle passes on rerun: no second repair cycle.
    let bundle = single_bundle(&evidence).expect("single bundle");
    let binding = assert_bundle_matches_sample(
        &bundle,
        &screen,
        "g6_both_pendings",
        "g6_both_pendings-img",
    )
    .expect("bundle matches sample");
    let canonical = fs::read_to_string(bundle.join("canonical.txt")).expect("read canonical");
    let image = fs::read(bundle.join("image.png")).expect("read image");
    write_text_snap(&snaps, "g6_both_pendings", &binding, &canonical).expect("write text snap");
    write_binary_snap(&snaps, "g6_both_pendings-img", &binding, &image)
        .expect("write binary snap");
    tuiscotti_insta::assert_screenshot!("g6_both_pendings", &screen, &policy);
}

#[test]
fn suffixed_snapshots_resolve_and_gate_end_to_end() {
    if insta_updates_in_place() {
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let snaps = tmp.path().join("snaps");
    let evidence = tmp.path().join("evidence");
    fs::create_dir(&snaps).expect("create snaps dir");
    fs::create_dir(&evidence).expect("create evidence dir");
    let policy = Policy::EvolvingIn {
        snapshots: snaps.clone(),
        evidence: evidence.clone(),
    };
    let screen = styled_screen().expect("valid test screen");
    let mut settings = insta::Settings::new();
    settings.set_snapshot_suffix("dark");
    settings.bind(|| {
        // First run under the suffix fails AND partitions evidence by variant...
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tuiscotti_insta::assert_screenshot!("g6_variant", &screen, &policy);
        }));
        assert!(outcome.is_err(), "unapproved snapshot must fail");
        // ...then the published bundle approves the SUFFIXED identity.
        let bundle = single_bundle(&evidence).expect("single bundle");
        assert!(
            bundle.to_string_lossy().contains("g6_variant@dark"),
            "evidence variant partition: {}",
            bundle.display()
        );
        let binding = assert_bundle_matches_sample(
            &bundle,
            &screen,
            "g6_variant@dark",
            "g6_variant-img@dark",
        )
        .expect("bundle matches sample");
        let canonical = fs::read_to_string(bundle.join("canonical.txt")).expect("read canonical");
        let image = fs::read(bundle.join("image.png")).expect("read image");
        write_text_snap(&snaps, "g6_variant@dark", &binding, &canonical)
            .expect("write text snap");
        write_binary_snap(&snaps, "g6_variant-img@dark", &binding, &image)
            .expect("write binary snap");
        // Rerun under the same suffix passes: the gate read the suffixed files.
        tuiscotti_insta::assert_screenshot!("g6_variant", &screen, &policy);
    });
    // The unsuffixed identity is a DIFFERENT snapshot: no approvals exist for it.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuiscotti_insta::assert_screenshot!("g6_variant", &screen, &policy);
    }));
    assert!(outcome.is_err(), "unsuffixed identity must stay unapproved");
}

// ---------------------------------------------------------------------------
// F10: resolved identity, bindings, and evidence partitioning (pure units).
// ---------------------------------------------------------------------------

#[test]
fn snapshot_identity_is_caller_relative_and_suffixed() {
    let loc = Location {
        file: "tests/compound.rs",
        line: 1,
    };
    // Default: <manifest>/<caller-parent>/snapshots, absolute.
    let id = resolve_snapshot_identity("/pkg-a", loc, "shot", None);
    assert_eq!(id.dir, Path::new("/pkg-a/tests/snapshots"));
    assert_eq!(id.canonical, "shot");
    assert_eq!(id.png_base, "shot-img");
    // Same name in another package or module file: a different identity.
    let other_pkg = resolve_snapshot_identity("/pkg-b", loc, "shot", None);
    assert_ne!(id.dir, other_pkg.dir);
    let other_mod = resolve_snapshot_identity(
        "/pkg-a",
        Location {
            file: "tests/sub/other.rs",
            line: 1,
        },
        "shot",
        None,
    );
    assert_ne!(id.dir, other_mod.dir);
    // Same directory, different file: Insta's native default shares the dir
    // (prepend is off), so same-name snapshots there are one snapshot.
    let same_dir = resolve_snapshot_identity(
        "/pkg-a",
        Location {
            file: "tests/other.rs",
            line: 1,
        },
        "shot",
        None,
    );
    assert_eq!(id.dir, same_dir.dir);
    // Absolute override wins verbatim; relative override joins the caller dir.
    let abs = resolve_snapshot_identity("/pkg-a", loc, "shot", Some(Path::new("/snaps")));
    assert_eq!(abs.dir, Path::new("/snaps"));
    let rel = resolve_snapshot_identity("/pkg-a", loc, "shot", Some(Path::new("custom")));
    assert_eq!(rel.dir, Path::new("/pkg-a/tests/custom"));
    // Explicit-dir form honors the same rules.
    let explicit = resolve_snapshot_identity_in("/pkg-a", loc, Path::new("/e"), "shot");
    assert_eq!(explicit.dir, Path::new("/e"));
    // Active suffix applies to BOTH stems, exactly like Insta.
    let mut settings = insta::Settings::new();
    settings.set_snapshot_suffix("v2");
    settings.bind(|| {
        let suffixed = resolve_snapshot_identity("/pkg-a", loc, "shot", None);
        assert_eq!(suffixed.canonical, "shot@v2");
        assert_eq!(suffixed.png_base, "shot-img@v2");
    });
    // No suffix leaks out of the bind scope.
    let plain = resolve_snapshot_identity("/pkg-a", loc, "shot", None);
    assert_eq!(plain.canonical, "shot");
}

#[test]
fn sample_binding_covers_canonical_render_and_payload() {
    let screen = styled_screen().expect("valid test screen");
    let sample = render_sample(&screen).expect("render sample");
    let base = sample_binding(&sample.canonical, "tuiscotti-default/rv1/straight-rgba", &sample.png);
    assert!(base.starts_with("v2-"), "{base}");
    // Deterministic.
    assert_eq!(
        base,
        sample_binding(&sample.canonical, "tuiscotti-default/rv1/straight-rgba", &sample.png)
    );
    // Canonical change moves it.
    assert_ne!(
        base,
        sample_binding("other", "tuiscotti-default/rv1/straight-rgba", &sample.png)
    );
    // Profile/renderer-only change moves it (same canonical text).
    assert_ne!(
        base,
        sample_binding(&sample.canonical, "custom/rv1/straight-rgba", &sample.png)
    );
    assert_ne!(
        base,
        sample_binding(&sample.canonical, "tuiscotti-default/rv2/straight-rgba", &sample.png)
    );
    // Payload-only change moves it.
    let mut png = sample.png.clone();
    let last = png.len() - 1;
    png[last] ^= 0xFF;
    assert_ne!(
        base,
        sample_binding(&sample.canonical, "tuiscotti-default/rv1/straight-rgba", &png)
    );
}

#[test]
fn missing_or_partial_bindings_fail_the_strict_gate() {
    let screen = styled_screen().expect("valid test screen");
    let canonical = insta_string(&screen);
    let binding = sample_binding(&canonical, "prof/rv1/straight-rgba", b"png-bytes");
    let sample = render_sample(&screen).expect("render sample");
    let tagged = png_tag_generation(&sample.png, &binding);
    let untagged = sample.png.clone();

    // Nothing at all: missing.
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = check_consistent(tmp.path(), "m", "m-img").expect_err("empty dir must fail");
    assert!(err.to_string().contains("missing"), "{err}");

    // Canonical only: partial.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "m", &binding, &canonical).expect("write text snap");
    let err = check_consistent(tmp.path(), "m", "m-img").expect_err("canonical-only must fail");
    assert!(err.to_string().contains("missing"), "{err}");

    // PNG sidecar without a tag: partial.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "m", &binding, &canonical).expect("write text snap");
    write_binary_snap(tmp.path(), "m-img", &binding, &untagged).expect("write binary snap");
    let err = check_consistent(tmp.path(), "m", "m-img").expect_err("untagged sidecar must fail");
    assert!(err.to_string().contains("tEXt"), "{err}");

    // PNG metadata without a binding: partial.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "m", &binding, &canonical).expect("write text snap");
    fs::write(
        tmp.path().join("m-img.snap"),
        "---\nsource: t\nexpression: png\n---\n",
    )
    .expect("write png meta");
    fs::write(tmp.path().join("m-img.snap.png"), &tagged).expect("write sidecar");
    let err = check_consistent(tmp.path(), "m", "m-img").expect_err("unbound png meta must fail");
    assert!(err.to_string().contains("missing"), "{err}");

    // Complete and agreeing: passes.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_text_snap(tmp.path(), "m", &binding, &canonical).expect("write text snap");
    write_binary_snap(tmp.path(), "m-img", &binding, &tagged).expect("write binary snap");
    check_consistent(tmp.path(), "m", "m-img").expect("complete compound passes");
}

#[test]
fn attempt_identity_parses_nextest_shape() {
    use std::collections::HashMap;
    let mut env = HashMap::new();
    env.insert("NEXTEST_RUN_ID".to_string(), "run-1".to_string());
    env.insert("NEXTEST_ATTEMPT".to_string(), "2".to_string());
    env.insert("NEXTEST_ATTEMPT_ID".to_string(), "run-1:bin$test".to_string());
    env.insert("NEXTEST_STRESS_CURRENT".to_string(), "3".to_string());
    env.insert("TUISCOTTI_SHARD".to_string(), "0/4".to_string());
    let id = AttemptIdentity::from_map(&env);
    assert_eq!(id.run, "run-1");
    assert_eq!(id.attempt, 2);
    assert_eq!(id.attempt_uid.as_deref(), Some("run-1:bin$test"));
    assert_eq!(id.stress_iter, Some(3));
    assert_eq!(id.shard.as_deref(), Some("0/4"));
    assert_eq!(id.run_dir(), "run-run-1");
    assert_eq!(id.attempt_dir(), "attempt-2-stress3-shard-0_4");

    // Garbage attempt is lossy-zero; "none" stress is None.
    let mut env = HashMap::new();
    env.insert("NEXTEST_ATTEMPT".to_string(), "bogus".to_string());
    env.insert("NEXTEST_STRESS_CURRENT".to_string(), "none".to_string());
    let id = AttemptIdentity::from_map(&env);
    assert_eq!(id.attempt, 0);
    assert_eq!(id.stress_iter, None);

    // Absent env: process-unique local run, attempt 0.
    let id = AttemptIdentity::from_map(&HashMap::new());
    assert!(id.run.starts_with("local-"), "{}", id.run);
    assert_eq!(id.attempt, 0);
    assert_eq!(id.shard, None);
}

#[test]
fn evidence_identity_partitions_and_never_escapes() {
    use std::collections::HashMap;
    let attempt = AttemptIdentity::from_map(&HashMap::from([
        ("NEXTEST_RUN_ID".to_string(), "r".to_string()),
        ("NEXTEST_ATTEMPT".to_string(), "1".to_string()),
    ]));
    let root = Path::new("/root");
    let id = EvidenceId {
        package: "my-pkg".to_string(),
        test: "a::b".to_string(),
        scenario: "shot".to_string(),
        variant: Some("dark".to_string()),
        attempt,
    };
    assert_eq!(
        id.bundle_dir(root),
        Path::new("/root/my-pkg/a__b/shot@dark/run-r/attempt-1")
    );
    // Hostile segments are sanitized, never escaped.
    assert_eq!(sanitize_segment("../../etc"), "_.._etc");
    assert_eq!(sanitize_segment(""), "unknown");
    assert_eq!(sanitize_segment("..."), "unknown");
    let evil = EvidenceId {
        package: "../../p".to_string(),
        test: "t/t".to_string(),
        scenario: "shot".to_string(),
        variant: None,
        attempt: AttemptIdentity::from_map(&HashMap::new()),
    };
    let dir = evil.bundle_dir(root);
    assert!(dir.starts_with(root), "{}", dir.display());
    for comp in dir.strip_prefix(root).expect("strip root").components() {
        let s = comp.as_os_str().to_string_lossy();
        assert!(!s.contains('/') && !s.contains('\\'), "no separators: {s}");
        assert!(s != ".." && s != ".", "no dot segments: {s}");
    }
}

