use super::*;
use tuiscotti::VENDORED_FACES;
use tuiscotti::snapshot::Status;

#[test]
fn missing_approved_png_fails_closed_with_missing_approval_panel() {
    // C06: expected bytes come from disk or the check fails. A missing
    // approved PNG is MissingApproval — never regenerated in memory —
    // and the report shows the "missing approval" panel, not a
    // re-rendered expectation.
    let (_dir, st) = tmp_store("expectedpanel").expect("tmp store");
    let frame = frame_with("frozen expected");
    let _outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    st.accept("home").expect("accept");
    std::fs::remove_file(st.root().join("approved").join("home.png")).expect("remove file");
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    assert!(!outcome.approved_png_regenerated);
    assert_eq!(
        outcome.status,
        Status::MissingApproval,
        "missing approved PNG must fail closed"
    );
    assert!(outcome.expected_png.is_none());
    assert!(outcome.expected_png_bytes.is_none());
    assert!(outcome.ensure_matched().is_err());
    let entry = st.report_entry(&outcome, &profile()).expect("report entry");
    let report = tuiscotti::snapshot::write_report(&st, "t", &[entry]).expect("write report");
    let html = std::fs::read_to_string(report).expect("read file");
    assert!(html.contains("missing approval"), "{html}");
    assert!(html.contains("missing-approval"), "{html}");
}

#[test]
fn check_seals_candidate_manifest_and_verify_accepts_it() {
    // C08: every check seals actual/<name>.manifest.json with per-artifact
    // hashes, the profile id, and complete:true; verify_candidate reports
    // NotChecked for the intact trio.
    let (_dir, st) = tmp_store("manifest").expect("tmp store");
    let frame = frame_with("sealed candidate");
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    let manifest_path = st.root().join("actual").join("home.manifest.json");
    let text = std::fs::read_to_string(&manifest_path).expect("read file");
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("parse json");
    assert_eq!(manifest["name"], "home");
    assert_eq!(manifest["complete"], true);
    assert_eq!(manifest["profile"], profile().name);
    for key in ["frame_sha256", "png_sha256", "fidelity_sha256"] {
        let hex = manifest[key].as_str().expect("json string");
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
    let (_dir, st) = tmp_store("interrupted").expect("tmp store");
    let frame = frame_with("atomic candidate");
    let _outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    st.accept("home").expect("accept");
    let matched = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    assert_eq!(matched.status, Status::Matched);
    assert_eq!(st.verify_candidate("home"), Status::NotChecked);
    std::fs::remove_file(&matched.actual_png).expect("remove file");
    let sidecar = matched.actual_png.with_extension("png.fidelity.json");
    drop(std::fs::remove_file(&sidecar));
    assert_eq!(st.verify_candidate("home"), Status::CaptureIncomplete);
    let mut renderer = profile().renderer(&VENDORED_FACES).expect("build renderer");
    let report = st
        .report_with(&mut renderer, 1.0, "interrupted")
        .expect("report");
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].status, Status::CaptureIncomplete);
    assert!(!report.outcomes[0].status.matched());
    assert!(report.outcomes[0].ensure_matched().is_err());
}

#[test]
fn store_report_reverifies_actuals_and_rewrites_report_html() {
    let (_dir, st) = tmp_store("storereport").expect("tmp store");
    let _outcome = st
        .check(
            "home",
            &frame_with("alpha"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .expect("check");
    st.accept("home").expect("accept");
    let _outcome = st
        .check(
            "other",
            &frame_with("beta"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .expect("check");
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "suite report")
        .expect("report");
    assert_eq!(report.outcomes.len(), 2);
    assert_eq!(report.failed(), 1, "`other` has no approval");
    assert!(report.path.exists());
    let html = std::fs::read_to_string(&report.path).expect("read file");
    assert!(html.contains("suite report"), "{html}");
    assert!(html.contains("home — matched"), "{html}");
    assert!(html.contains("other — missing-approval"), "{html}");
    // Unmatched outcomes do not error; the caller asserts on them.
    assert!(
        report
            .outcomes
            .iter()
            .any(|o| o.name == "other" && o.status == Status::MissingApproval)
    );
}

#[test]
fn store_report_on_empty_store_writes_empty_index() {
    let (_dir, st) = tmp_store("emptyreport").expect("tmp store");
    let report = st
        .report(&profile(), &VENDORED_FACES, 1.0, "empty")
        .expect("report");
    assert_eq!(report.failed(), 0);
    assert!(report.path.exists());
}

#[test]
fn check_with_reuses_one_renderer_across_checks() {
    let (_dir, st) = tmp_store("checkwith").expect("tmp store");
    let profile = profile();
    let mut renderer = profile.renderer(&VENDORED_FACES).expect("build renderer");
    let frame = frame_with("cached check");
    let o1 = st
        .check_with(&mut renderer, "home", &frame, 1.0)
        .expect("check");
    assert_eq!(o1.status, Status::MissingApproval);
    st.accept("home").expect("accept");
    let o2 = st
        .check_with(&mut renderer, "home", &frame, 1.0)
        .expect("check");
    assert_eq!(o2.status, Status::Matched);
    assert_eq!(o2.pixel_score, Some(1.0));
    o2.ensure_matched().expect("ensure matched");
}

#[test]
fn script_embed_round_trips_hostile_symbols() {
    let (_dir, st) = tmp_store("script").expect("tmp store");
    let mut frame = frame_with("ok");
    // A markup-significant symbol: the report must escape it in <script>
    // embeds (no literal `</script>` may reach the HTML) and re-import
    // must restore it losslessly.
    frame.cells[0].symbol = "<".to_string();
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    let entry = tuiscotti::snapshot::report_entry(&outcome, &profile()).expect("report entry");
    let report = tuiscotti::snapshot::write_report(&st, "t", &[entry]).expect("write report");
    let html = std::fs::read_to_string(&report).expect("read file");
    // Index does not inline frame JSON, so a cell `<` cannot break HTML.
    assert!(!html.contains("<script type=\"application/json\""));
    assert_eq!(html.matches("</script>").count(), 0);
    let back = tuiscotti::Frame::from_json(
        &std::fs::read_to_string(&outcome.actual_frame).expect("read file"),
    )
    .expect("parse frame json");
    assert_eq!(back.digest(), frame.digest());
}

#[test]
fn unsafe_names_rejected_before_any_write() {
    let (dir, st) = tmp_store("badnames").expect("tmp store");
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
            .expect_err("check must fail")
            .to_string();
        assert!(err.contains("invalid snapshot name"), "{bad:?}: {err}");
        let err = st.accept(bad).expect_err("accept must fail").to_string();
        assert!(err.contains("invalid snapshot name"), "{bad:?}: {err}");
    }
    // Rejected before any write: no store dirs, no escape from the root.
    assert!(!st.root().join("actual").exists());
    assert!(!st.root().join("approved").exists());
    assert!(!dir.path().join("evil").exists());
}
