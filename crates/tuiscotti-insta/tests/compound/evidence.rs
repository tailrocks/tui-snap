//! Evidence publication: ordering, pendings, suffixed identities.

use std::fs;
use std::path::PathBuf;

use super::helpers::{
    assert_bundle_matches_sample, collect_files, insta_updates_in_place, insta_writes_new_files,
    panic_message, single_bundle, styled_screen, write_binary_snap, write_text_snap,
};
use tuiscotti_insta::assert::{
    GEN_DESC_PREFIX, Location, Policy, png_generation, resolve_snapshot_identity,
    snapshot_dir_override,
};

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

/// Removes default-placement pendings (and the dir, when left empty) even
/// when the test panics mid-way: caller `snapshots/` keeps no residue.
struct PendingGuard {
    dir: PathBuf,
    files: Vec<PathBuf>,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        for f in &self.files {
            let _gone = fs::remove_file(f);
        }
        // Only removes the dir when OUR run left it empty.
        let _dir = fs::remove_dir(&self.dir);
    }
}

#[test]
fn default_placement_lands_in_caller_snapshots() {
    if insta_updates_in_place() || !insta_writes_new_files() {
        return;
    }
    // An explicit snapshot-dir override would redirect placement: untestable.
    if snapshot_dir_override().is_some() {
        return;
    }
    let name = "f_default_placement";
    let loc = Location {
        file: file!(),
        line: line!(),
    };
    let expected = resolve_snapshot_identity(env!("CARGO_MANIFEST_DIR"), loc, name, None);
    let pendings = vec![
        expected.dir.join(format!("{name}.snap")),
        expected.dir.join(format!("{name}.snap.new")),
    ];
    // First-run state (also clears residue from an aborted run).
    for p in &pendings {
        let _gone = fs::remove_file(p);
    }
    let _guard = PendingGuard {
        dir: expected.dir.clone(),
        files: pendings.clone(),
    };
    let screen = styled_screen().expect("valid test screen");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuiscotti_insta::assert_snapshot!(name, &screen);
    }));
    assert!(outcome.is_err(), "unapproved snapshot must fail");
    assert!(
        expected.dir.join(format!("{name}.snap.new")).is_file(),
        "None-override pending must land in caller snapshots/: {}",
        expected.dir.display()
    );
}

#[test]
fn insta_source_names_caller_on_real_pendings() {
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
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuiscotti_insta::assert_screenshot!("g_source_names_caller", &screen, &policy);
    }));
    assert!(outcome.is_err(), "unapproved snapshot must fail");
    // Both real pendings name THIS caller file in `source:`.
    let caller = file!().rsplit('/').next().expect("caller file name");
    for pending in [
        "g_source_names_caller.snap.new",
        "g_source_names_caller-img.snap.new",
    ] {
        let text = fs::read_to_string(snaps.join(pending)).expect("read pending");
        let source = text
            .lines()
            .find_map(|l| l.strip_prefix("source:"))
            .expect("source line");
        assert!(source.contains(caller), "source names caller: {source}");
    }
}

/// Binding token from a pending `.snap.new` description header.
fn pending_binding(pending: &str) -> Option<String> {
    let mut lines = pending.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(v) = line.trim().strip_prefix("description:") {
            let v = v.trim().trim_matches('"');
            let binding = v.strip_prefix(GEN_DESC_PREFIX)?;
            return binding.split_whitespace().next().map(str::to_string);
        }
    }
    None
}

#[test]
fn first_run_accept_by_rename_passes_on_rerun() {
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
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuiscotti_insta::assert_screenshot!("k_accept_by_rename", &screen, &policy);
    }));
    assert!(outcome.is_err(), "unapproved snapshot must fail");
    // Parse the REAL pending headers + PNG tag (no synthesized approvals).
    let canonical_new = fs::read_to_string(snaps.join("k_accept_by_rename.snap.new"))
        .expect("read canonical pending");
    let png_new = fs::read_to_string(snaps.join("k_accept_by_rename-img.snap.new"))
        .expect("read png pending");
    let sidecar_new = fs::read(snaps.join("k_accept_by_rename-img.snap.new.png"))
        .expect("read png sidecar pending");
    let c = pending_binding(&canonical_new).expect("canonical pending binding");
    let p = pending_binding(&png_new).expect("png pending binding");
    let t = png_generation(&sidecar_new).expect("sidecar tag");
    assert!(c.starts_with("v2-"), "{c}");
    assert_eq!(p, c, "png header must bind the same sample");
    assert_eq!(t, c, "sidecar tag must bind the same sample");
    // Accept exactly like `cargo insta accept`: rename pendings into place.
    fs::rename(
        snaps.join("k_accept_by_rename.snap.new"),
        snaps.join("k_accept_by_rename.snap"),
    )
    .expect("accept canonical");
    fs::rename(
        snaps.join("k_accept_by_rename-img.snap.new"),
        snaps.join("k_accept_by_rename-img.snap"),
    )
    .expect("accept png meta");
    fs::rename(
        snaps.join("k_accept_by_rename-img.snap.new.png"),
        snaps.join("k_accept_by_rename-img.snap.png"),
    )
    .expect("accept png sidecar");
    // Rerun passes: no second repair cycle.
    tuiscotti_insta::assert_screenshot!("k_accept_by_rename", &screen, &policy);
}
