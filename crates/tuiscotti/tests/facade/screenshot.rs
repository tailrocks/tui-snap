use super::*;
use std::fs;
use tuiscotti::assert::{check_consistent, generation_id, png_tag_generation, render_sample};
use tuiscotti::insta_proto::insta_string;

#[test]
fn snapshot_macro_passes_on_identical_rerun() {
    let ws = workspace();
    let screen = fixture();
    let canonical = insta_string(&screen);
    let generation = generation_id(&canonical);
    // Second same-process call auto-suffixes to `fac_rerun-2` (no public opt-out).
    write_text_snap(&ws.snaps, "fac_rerun", &generation, &canonical);
    write_text_snap(&ws.snaps, "fac_rerun-2", &generation, &canonical);
    tuiscotti::assert_snapshot!("fac_rerun", &screen, &ws.policy());
    tuiscotti::assert_snapshot!("fac_rerun", &screen, &ws.policy());
}

// ---------------------------------------------------------------------------
// I02: assert_screenshot!
// ---------------------------------------------------------------------------
#[test]
fn screenshot_passes_when_consistent() {
    let ws = workspace();
    let screen = fixture();
    let sample = render_sample(&screen).unwrap();
    let generation = generation_id(&sample.canonical);
    write_text_snap(&ws.snaps, "fac_shotok", &generation, &sample.canonical);
    write_binary_snap(
        &ws.snaps,
        "fac_shotok-img",
        &generation,
        &png_tag_generation(&sample.png, &generation),
    );
    tuiscotti::assert_screenshot!("fac_shotok", &screen, &ws.policy());
}

#[test]
fn screenshot_evidence_present_before_failure() {
    let ws = workspace();
    let screen = fixture();
    // No approvals for fac_evidence: the macro must fail.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuiscotti::assert_screenshot!("fac_evidence", &screen, &ws.policy());
    }));
    assert!(result.is_err(), "unapproved screenshot must fail");
    // ...but candidate evidence from the same sample is on disk first.
    let sample = render_sample(&screen).unwrap();
    let generation = generation_id(&sample.canonical);
    assert_eq!(
        fs::read(ws.evidence.join("fac_evidence.png")).unwrap(),
        png_tag_generation(&sample.png, &generation)
    );
    assert_eq!(
        fs::read_to_string(ws.evidence.join("fac_evidence.ansi")).unwrap(),
        sample.ansi
    );
    assert_eq!(
        fs::read_to_string(ws.evidence.join("fac_evidence.txt")).unwrap(),
        sample.txt
    );
    assert_eq!(
        fs::read_to_string(ws.evidence.join("fac_evidence.html")).unwrap(),
        sample.html
    );
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
    let ws = workspace();
    let screen = fixture();
    let sample = render_sample(&screen).unwrap();
    let generation = generation_id(&sample.canonical);
    // Canonical approved at gen-A; PNG pixels identical but bound to gen-B.
    write_text_snap(&ws.snaps, "fac_mixed", &generation, &sample.canonical);
    write_binary_snap(
        &ws.snaps,
        "fac_mixed-img",
        "gen-b",
        &png_tag_generation(&sample.png, "gen-b"),
    );
    // Strict gate agrees directly.
    check_consistent(&ws.snaps, "fac_mixed", "fac_mixed-img")
        .expect_err("mixed baseline must be inconsistent");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuiscotti::assert_screenshot!("fac_mixed", &screen, &ws.policy());
    }));
    let msg = panic_message(result.unwrap_err());
    assert!(msg.contains("mixed compound baseline"), "{msg}");
}
