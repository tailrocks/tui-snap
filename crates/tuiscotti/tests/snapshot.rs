//! Approved-store behaviors: write-before-assert, explicit accept,
//! corrupt/missing handling, concurrency, reports, no auto-bless.

use ratatui::widgets::Paragraph;
use std::path::PathBuf;
use tuiscotti::snapshot::{Status, Store};
use tuiscotti::{Profile, Provenance, VENDORED_FACES};

fn prov() -> Provenance {
    Provenance {
        tool: "tuisnap".into(),
        tool_version: "test".into(),
        profile: "tuisnap-default".into(),
        source: "test".into(),
        argv: vec![],
        created_unix: 0,
    }
}

fn profile() -> Profile {
    Profile::default_profile()
}

fn frame_with(text: &str) -> tuiscotti::Frame {
    tuiscotti::ratatui::widget_frame(Paragraph::new(text), 30, 6, prov())
}

fn tmp_store(tag: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let st = Store::new(&dir.path().join(tag));
    (dir, st)
}

#[test]
fn missing_approval_fails_closed_but_writes_actuals() {
    let (_dir, st) = tmp_store("missing");
    let frame = frame_with("hello");
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(outcome.status, Status::MissingApproval);
    // Actuals on disk BEFORE any assertion ran.
    assert!(outcome.actual_frame.exists());
    assert!(outcome.actual_png.exists());
    assert!(outcome.ensure_matched().is_err());
}

#[test]
fn accept_then_match_round_trip() {
    let (_dir, st) = tmp_store("accept");
    let frame = frame_with("stable screen");
    let o1 = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert!(!o1.status.matched());
    st.accept("home").unwrap();
    let o2 = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(o2.status, Status::Matched);
    assert_eq!(o2.pixel_score, Some(1.0));
    o2.ensure_matched().unwrap();
}

#[test]
fn changed_snapshot_reports_cells_and_diff_image() {
    let (_dir, st) = tmp_store("changed");
    let _ = st
        .check(
            "home",
            &frame_with("before"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    st.accept("home").unwrap();
    let outcome = st
        .check(
            "home",
            &frame_with("after!"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    assert_eq!(outcome.status, Status::CellsDiffer);
    assert!(outcome.cell_diff_total > 0);
    assert!(!outcome.cell_diffs.is_empty());
    assert!(outcome.pixel_score.is_some_and(|s| s < 1.0));
    let diff = outcome.diff_png.clone().unwrap();
    assert!(diff.exists());
    let err = outcome.ensure_matched().unwrap_err().to_string();
    assert!(err.contains("actual:") && err.contains("diff:") && err.contains("tuisnap accept"));
}

#[test]
fn check_writes_fidelity_sidecar_next_to_actual_png() {
    let (_dir, st) = tmp_store("fidelity");
    let frame = frame_with("crab 🦀");
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    let sidecar = outcome.actual_png.with_extension("png.fidelity.json");
    let json = std::fs::read_to_string(&sidecar).unwrap();
    assert!(json.contains("\"approximate\": true"), "{json}");
    assert!(json.contains("U+1F980"), "{json}");
    // Acceptance pairs the sidecar with the approved PNG.
    st.accept("home").unwrap();
    let approved = st.root().join("approved").join("home.png.fidelity.json");
    assert!(std::fs::read_to_string(&approved)
        .unwrap()
        .contains("U+1F980"));
}

#[test]
fn corrupt_approval_is_explicit() {
    let (dir, st) = tmp_store("corrupt");
    std::fs::create_dir_all(dir.path().join("corrupt").join("approved")).unwrap();
    std::fs::write(
        dir.path()
            .join("corrupt")
            .join("approved")
            .join("home.frame.json"),
        "{broken",
    )
    .unwrap();
    let outcome = st
        .check("home", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(outcome.status, Status::CorruptApproval);
    let err = outcome.ensure_matched().unwrap_err().to_string();
    assert!(err.contains("corrupt"), "{err}");
}

#[test]
fn no_env_var_can_auto_accept() {
    // CI-safety: acceptance is an explicit command, never ambient state.
    // (Historically this test set BLESS / TUISNAP_ACCEPT / UPDATE_SNAPSHOT;
    // nothing reads them and `set_var` is an `unsafe fn` in edition 2024, so
    // the store is checked against the ambient environment instead.)
    let (_dir, st) = tmp_store("noauto");
    let outcome = st
        .check("home", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(outcome.status, Status::MissingApproval);
}

#[test]
fn concurrent_different_names_are_safe() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("conc");
    std::thread::scope(|s| {
        for t in 0..4 {
            let root = root.clone();
            s.spawn(move || {
                let st = Store::new(&root);
                let name = format!("screen-{t}");
                let frame = frame_with(&format!("thread {t}"));
                let o = st
                    .check(&name, &frame, &profile(), &VENDORED_FACES, 1.0)
                    .unwrap();
                assert_eq!(o.status, Status::MissingApproval);
                st.accept(&name).unwrap();
                let o2 = st
                    .check(&name, &frame, &profile(), &VENDORED_FACES, 1.0)
                    .unwrap();
                assert!(o2.status.matched());
            });
        }
    });
    let st = Store::new(&root);
    assert_eq!(st.actual_names().unwrap().len(), 4);
}

#[test]
fn report_links_png_files_not_base64() {
    let (_dir, st) = tmp_store("report");
    let frame = frame_with("reported");
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    let entry = tuiscotti::snapshot::report_entry(&outcome, &profile()).unwrap();
    let report = tuiscotti::snapshot::write_report(&st, "test report", &[entry]).unwrap();
    let html = std::fs::read_to_string(&report).unwrap();
    assert!(
        !html.contains("data:image/png;base64,"),
        "report must link files, not embed PNG bytes"
    );
    assert!(html.contains(".png"), "{html}");
    assert!(html.contains("tuisnap-default"));
    let _ = PathBuf::from("x");
}

#[test]
fn corrupt_approved_png_is_an_explicit_error() {
    let (_dir, st) = tmp_store("badpng");
    let _ = st
        .check("home", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    // Sabotage the approved PNG (frame JSON stays valid).
    let root = st.root();
    std::fs::write(root.join("approved").join("home.png"), b"not a png").unwrap();
    let err = st
        .check("home", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot decode expected PNG"), "{err}");
}

#[test]
fn dimension_mismatch_status() {
    let (_dir, st) = tmp_store("dims");
    let _ = st
        .check("home", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    let other = tuiscotti::ratatui::widget_frame(Paragraph::new("x"), 20, 5, prov());
    let outcome = st
        .check("home", &other, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(outcome.status, Status::DimensionMismatch);
}

#[test]
fn relaxed_threshold_still_gates_dimensions() {
    let (_dir, st) = tmp_store("threshold");
    let frame = frame_with("same");
    let _ = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    // Identical frames score 1.0: matched under any threshold <= 1.0.
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 0.99)
        .unwrap();
    assert!(outcome.status.matched());
    assert_eq!(outcome.pixel_score, Some(1.0));
}

#[test]
fn same_name_concurrent_checks_are_safe() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("samesame");
    let frame = frame_with("shared");
    std::thread::scope(|s| {
        for _ in 0..4 {
            let root = root.clone();
            let frame = frame.clone();
            s.spawn(move || {
                let st = Store::new(&root);
                let o = st
                    .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
                    .unwrap();
                assert!(matches!(
                    o.status,
                    Status::MissingApproval | Status::Matched | Status::CellsDiffer
                ));
                // Acceptance of identical content converges.
                let _ = st.accept("home");
            });
        }
    });
    let st = Store::new(&root);
    st.accept("home").unwrap();
    let o = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert!(o.status.matched());
    // Approved frame still parses (no torn writes).
    let text = std::fs::read_to_string(root.join("approved").join("home.frame.json")).unwrap();
    tuiscotti::Frame::from_json(&text).unwrap();
}

#[test]
fn missing_approved_png_fails_closed_with_missing_approval_panel() {
    // C06: expected bytes come from disk or the check fails. A missing
    // approved PNG is MissingApproval — never regenerated in memory —
    // and the report shows the "missing approval" panel, not a
    // re-rendered expectation.
    let (_dir, st) = tmp_store("expectedpanel");
    let frame = frame_with("frozen expected");
    let _ = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    std::fs::remove_file(st.root().join("approved").join("home.png")).unwrap();
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert!(!outcome.approved_png_regenerated);
    assert_eq!(
        outcome.status,
        Status::MissingApproval,
        "missing approved PNG must fail closed"
    );
    assert!(outcome.expected_png.is_none());
    assert!(outcome.expected_png_bytes.is_none());
    assert!(outcome.ensure_matched().is_err());
    let entry = st.report_entry(&outcome, &profile()).unwrap();
    let report = tuiscotti::snapshot::write_report(&st, "t", &[entry]).unwrap();
    let html = std::fs::read_to_string(report).unwrap();
    assert!(html.contains("missing approval"), "{html}");
    assert!(html.contains("missing-approval"), "{html}");
}

#[test]
fn check_seals_candidate_manifest_and_verify_accepts_it() {
    // C08: every check seals actual/<name>.manifest.json with per-artifact
    // hashes, the profile id, and complete:true; verify_candidate reports
    // NotChecked for the intact trio.
    let (_dir, st) = tmp_store("manifest");
    let frame = frame_with("sealed candidate");
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    let manifest_path = st.root().join("actual").join("home.manifest.json");
    let text = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(manifest["name"], "home");
    assert_eq!(manifest["complete"], true);
    assert_eq!(manifest["profile"], profile().name);
    for key in ["frame_sha256", "png_sha256", "fidelity_sha256"] {
        let hex = manifest[key].as_str().unwrap();
        assert_eq!(hex.len(), 64, "{key} must be hex sha256");
    }
    assert_eq!(st.verify_candidate("home"), Status::NotChecked);
    assert!(outcome.actual_png.exists());
}

#[test]
fn interrupted_candidate_reports_capture_incomplete_not_match() {
    // C08: actual frame without its PNG (interrupted write) reports
    // CaptureIncomplete from both verify_candidate and report_with —
    // the report must not silently re-render the gap back to Matched.
    let (_dir, st) = tmp_store("interrupted");
    let frame = frame_with("atomic candidate");
    let _ = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    let matched = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(matched.status, Status::Matched);
    assert_eq!(st.verify_candidate("home"), Status::NotChecked);
    std::fs::remove_file(&matched.actual_png).unwrap();
    let sidecar = matched.actual_png.with_extension("png.fidelity.json");
    let _ = std::fs::remove_file(&sidecar);
    assert_eq!(st.verify_candidate("home"), Status::CaptureIncomplete);
    let mut renderer = profile().renderer(&VENDORED_FACES).unwrap();
    let report = st.report_with(&mut renderer, 1.0, "interrupted").unwrap();
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].status, Status::CaptureIncomplete);
    assert!(!report.outcomes[0].status.matched());
    assert!(report.outcomes[0].ensure_matched().is_err());
}

#[test]
fn store_report_reverifies_actuals_and_rewrites_report_html() {
    let (_dir, st) = tmp_store("storereport");
    let _ = st
        .check(
            "home",
            &frame_with("alpha"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    st.accept("home").unwrap();
    let _ = st
        .check(
            "other",
            &frame_with("beta"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "suite report")
        .unwrap();
    assert_eq!(report.outcomes.len(), 2);
    assert_eq!(report.failed(), 1, "`other` has no approval");
    assert!(report.path.exists());
    let html = std::fs::read_to_string(&report.path).unwrap();
    assert!(html.contains("suite report"), "{html}");
    assert!(html.contains("home — matched"), "{html}");
    assert!(html.contains("other — missing-approval"), "{html}");
    // Unmatched outcomes do not error; the caller asserts on them.
    assert!(report
        .outcomes
        .iter()
        .any(|o| o.name == "other" && o.status == Status::MissingApproval));
}

#[test]
fn store_report_on_empty_store_writes_empty_index() {
    let (_dir, st) = tmp_store("emptyreport");
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "empty")
        .unwrap();
    assert_eq!(report.failed(), 0);
    assert!(report.path.exists());
}

#[test]
fn check_with_reuses_one_renderer_across_checks() {
    let (_dir, st) = tmp_store("checkwith");
    let profile = profile();
    let mut renderer = profile.renderer(&VENDORED_FACES).unwrap();
    let frame = frame_with("cached check");
    let o1 = st.check_with(&mut renderer, "home", &frame, 1.0).unwrap();
    assert_eq!(o1.status, Status::MissingApproval);
    st.accept("home").unwrap();
    let o2 = st.check_with(&mut renderer, "home", &frame, 1.0).unwrap();
    assert_eq!(o2.status, Status::Matched);
    assert_eq!(o2.pixel_score, Some(1.0));
    o2.ensure_matched().unwrap();
}

#[test]
fn script_embed_round_trips_hostile_symbols() {
    let (_dir, st) = tmp_store("script");
    let mut frame = frame_with("ok");
    // A markup-significant symbol: the report must escape it in <script>
    // embeds (no literal `</script>` may reach the HTML) and re-import
    // must restore it losslessly.
    frame.cells[0].symbol = "<".to_string();
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    let entry = tuiscotti::snapshot::report_entry(&outcome, &profile()).unwrap();
    let report = tuiscotti::snapshot::write_report(&st, "t", &[entry]).unwrap();
    let html = std::fs::read_to_string(&report).unwrap();
    // Index does not inline frame JSON, so a cell `<` cannot break HTML.
    assert!(!html.contains("<script type=\"application/json\""));
    assert_eq!(html.matches("</script>").count(), 0);
    let back = tuiscotti::Frame::from_json(&std::fs::read_to_string(&outcome.actual_frame).unwrap())
        .unwrap();
    assert_eq!(back.digest(), frame.digest());
}

#[test]
fn unsafe_names_rejected_before_any_write() {
    let (dir, st) = tmp_store("badnames");
    let frame = frame_with("x");
    for bad in [
        "../../evil",
        "/abs/evil",
        "a/../evil",
        "",
        "a\\evil",
        "a//evil",
        ".",
        "a/./evil",
    ] {
        let err = st
            .check(bad, &frame, &profile(), &VENDORED_FACES, 1.0)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid snapshot name"), "{bad:?}: {err}");
        let err = st.accept(bad).unwrap_err().to_string();
        assert!(err.contains("invalid snapshot name"), "{bad:?}: {err}");
    }
    // Rejected before any write: no store dirs, no escape from the root.
    assert!(!st.root().join("actual").exists());
    assert!(!st.root().join("approved").exists());
    assert!(!dir.path().join("evil").exists());
}
