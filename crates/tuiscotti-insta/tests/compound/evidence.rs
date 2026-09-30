//! Evidence publication: ordering, pendings, suffixed identities.

use std::fs;

use super::helpers::{
    assert_bundle_matches_sample, collect_files, insta_updates_in_place, insta_writes_new_files,
    panic_message, single_bundle, styled_screen, write_binary_snap, write_text_snap,
};
use tuiscotti_insta::assert::Policy;

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
    assert!(segs[4].starts_with("attempt-"), "attempt segment: {rel}");
    assert_bundle_matches_sample(
        &bundle,
        &screen,
        "g6_evidence_first",
        "g6_evidence_first-img",
    )
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
    let binding =
        assert_bundle_matches_sample(&bundle, &screen, "g6_both_pendings", "g6_both_pendings-img")
            .expect("bundle matches sample");
    let canonical = fs::read_to_string(bundle.join("canonical.txt")).expect("read canonical");
    let image = fs::read(bundle.join("image.png")).expect("read image");
    write_text_snap(&snaps, "g6_both_pendings", &binding, &canonical).expect("write text snap");
    write_binary_snap(&snaps, "g6_both_pendings-img", &binding, &image).expect("write binary snap");
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
        write_text_snap(&snaps, "g6_variant@dark", &binding, &canonical).expect("write text snap");
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
