use super::*;
use std::fs;
use tuiscotti::assert::{check_consistent, generation_id, png_tag_generation, render_sample};
use tuiscotti::insta_proto::insta_string;

/// Find the published bundle for `scenario` under the evidence root
/// (exactly one `complete.json` beneath the scenario partition).
fn find_bundle(evidence: &Path, scenario: &str) -> Result<PathBuf, String> {
    let mut stack = vec![evidence.to_path_buf()];
    let mut hits = Vec::new();
    while let Some(d) = stack.pop() {
        let entries: Vec<PathBuf> = fs::read_dir(&d)
            .map_err(|e| format!("read {}: {e}", d.display()))?
            .map(|e| e.map(|e| e.path()).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "complete.json")
                && path
                    .strip_prefix(evidence)
                    .is_ok_and(|rel| rel.components().any(|c| c.as_os_str() == scenario))
            {
                hits.push(
                    path.parent()
                        .map(std::path::Path::to_path_buf)
                        .ok_or_else(|| "bundle parent".to_string())?,
                );
            }
        }
    }
    assert_eq!(hits.len(), 1, "exactly one bundle for {scenario}");
    hits.pop().ok_or_else(|| "bundle hit".to_string())
}

#[test]
fn snapshot_macro_passes_on_identical_rerun() {
    let ws = workspace().expect("workspace succeeds");
    let screen = fixture().expect("fixture succeeds");
    let canonical = insta_string(&screen);
    let generation = generation_id(&canonical);
    // Second same-process call auto-suffixes to `fac_rerun-2` (no public opt-out).
    write_text_snap(&ws.snaps, "fac_rerun", &generation, &canonical)
        .expect("write_text_snap succeeds");
    write_text_snap(&ws.snaps, "fac_rerun-2", &generation, &canonical)
        .expect("write_text_snap succeeds");
    tuiscotti::assert_snapshot!("fac_rerun", &screen, &ws.policy());
    tuiscotti::assert_snapshot!("fac_rerun", &screen, &ws.policy());
}

// ---------------------------------------------------------------------------
// I02: assert_screenshot!
// ---------------------------------------------------------------------------
#[test]
fn screenshot_passes_when_consistent() {
    let ws = workspace().expect("workspace succeeds");
    let screen = fixture().expect("fixture succeeds");
    let sample = render_sample(&screen).expect("render_sample(&screen) succeeds");
    let generation = generation_id(&sample.canonical);
    write_text_snap(&ws.snaps, "fac_shotok", &generation, &sample.canonical)
        .expect("write_text_snap succeeds");
    write_binary_snap(
        &ws.snaps,
        "fac_shotok-img",
        &generation,
        &png_tag_generation(&sample.png, &generation),
    )
    .expect("write_binary_snap succeeds");
    tuiscotti::assert_screenshot!("fac_shotok", &screen, &ws.policy());
}

#[test]
fn screenshot_evidence_present_before_failure() {
    let ws = workspace().expect("workspace succeeds");
    let screen = fixture().expect("fixture succeeds");
    // No approvals for fac_evidence: the macro must fail.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuiscotti::assert_screenshot!("fac_evidence", &screen, &ws.policy());
    }));
    assert!(result.is_err(), "unapproved screenshot must fail");
    // ...but the full candidate bundle from the same sample is on disk first.
    let bundle = find_bundle(&ws.evidence, "fac_evidence").expect("find_bundle succeeds");
    let sample = render_sample(&screen).expect("render_sample(&screen) succeeds");
    assert_eq!(
        fs::read_to_string(bundle.join("canonical.txt"))
            .expect("fs::read_to_string(bundle.join(\"canonical.txt\")) succeeds"),
        sample.canonical
    );
    assert_eq!(
        fs::read_to_string(bundle.join("sample.ansi"))
            .expect("fs::read_to_string(bundle.join(\"sample.ansi\")) succeeds"),
        sample.ansi
    );
    assert_eq!(
        fs::read_to_string(bundle.join("sample.txt"))
            .expect("fs::read_to_string(bundle.join(\"sample.txt\")) succeeds"),
        sample.txt
    );
    assert_eq!(
        fs::read_to_string(bundle.join("sample.html"))
            .expect("fs::read_to_string(bundle.join(\"sample.html\")) succeeds"),
        sample.html
    );
    // The tagged image carries the manifest binding for this sample.
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(bundle.join("manifest.json"))
            .expect("fs::read_to_string(bundle.join(\"manifest.json\")) succeeds"),
    )
    .expect("manifest parses");
    let binding = manifest["binding"].as_str().expect("manifest binding");
    assert_eq!(
        fs::read(bundle.join("image.png")).expect("fs::read(bundle.join(\"image.png\")) succeeds"),
        png_tag_generation(&sample.png, binding)
    );
    assert!(bundle.join("complete.json").is_file());
    // Approvals are never blessed (unless the ambient run forces in-place
    // updates), and no pendings are written when the effective mode writes
    // nothing (`INSTA_UPDATE` is ambient-only now; see `common`).
    if !common::insta_updates_in_place() {
        assert!(!ws.snaps.join("fac_evidence.snap").exists());
        assert!(!ws.snaps.join("fac_evidence-img.snap").exists());
    }
    if common::insta_writes_nothing() {
        assert!(!ws.snaps.join("fac_evidence.snap.new").exists());
        assert!(!ws.snaps.join("fac_evidence-img.snap.new").exists());
    }
}

#[test]
fn screenshot_mixed_generation_fails() {
    let ws = workspace().expect("workspace succeeds");
    let screen = fixture().expect("fixture succeeds");
    let sample = render_sample(&screen).expect("render_sample(&screen) succeeds");
    let generation = generation_id(&sample.canonical);
    // Canonical approved at gen-A; PNG pixels identical but bound to gen-B.
    write_text_snap(&ws.snaps, "fac_mixed", &generation, &sample.canonical)
        .expect("write_text_snap succeeds");
    write_binary_snap(
        &ws.snaps,
        "fac_mixed-img",
        "gen-b",
        &png_tag_generation(&sample.png, "gen-b"),
    )
    .expect("write_binary_snap succeeds");
    // Strict gate agrees directly.
    check_consistent(&ws.snaps, "fac_mixed", "fac_mixed-img")
        .expect_err("mixed baseline must be inconsistent");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuiscotti::assert_screenshot!("fac_mixed", &screen, &ws.policy());
    }));
    let msg = panic_message(&*result.expect_err("result is an error"));
    assert!(msg.contains("mixed compound baseline"), "{msg}");
}
