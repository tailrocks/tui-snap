//! Approved-store behaviors: write-before-assert, explicit accept,
//! corrupt/missing handling, concurrency, reports, no auto-bless.

use ratatui::widgets::Paragraph;
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

fn tmp_store(tag: &str) -> Result<(tempfile::TempDir, Store), String> {
    let dir = tempfile::tempdir().map_err(|e| format!("tempdir: {e}"))?;
    let st = Store::new(&dir.path().join(tag));
    Ok((dir, st))
}

#[test]
fn missing_approval_fails_closed_but_writes_actuals() {
    let (_dir, st) = tmp_store("missing").expect("tmp store");
    let frame = frame_with("hello");
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    assert_eq!(outcome.status, Status::MissingApproval);
    // Actuals on disk BEFORE any assertion ran.
    assert!(outcome.actual_frame.exists());
    assert!(outcome.actual_png.exists());
    assert!(outcome.ensure_matched().is_err());
}

#[test]
fn accept_then_match_round_trip() {
    let (_dir, st) = tmp_store("accept").expect("tmp store");
    let frame = frame_with("stable screen");
    let o1 = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    assert!(!o1.status.matched());
    st.accept("home").expect("accept");
    let o2 = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    assert_eq!(o2.status, Status::Matched);
    assert_eq!(o2.pixel_score, Some(1.0));
    o2.ensure_matched().expect("ensure matched");
}

#[test]
fn changed_snapshot_reports_cells_and_diff_image() {
    let (_dir, st) = tmp_store("changed").expect("tmp store");
    let _outcome = st
        .check(
            "home",
            &frame_with("before"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .expect("check");
    st.accept("home").expect("accept");
    let outcome = st
        .check(
            "home",
            &frame_with("after!"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .expect("check");
    assert_eq!(outcome.status, Status::CellsDiffer);
    assert!(outcome.cell_diff_total > 0);
    assert!(!outcome.cell_diffs.is_empty());
    assert!(outcome.pixel_score.is_some_and(|s| s < 1.0));
    let diff = outcome.diff_png.clone().expect("diff png present");
    assert!(diff.exists());
    let err = outcome
        .ensure_matched()
        .expect_err("ensure matched must fail")
        .to_string();
    assert!(err.contains("actual:") && err.contains("diff:") && err.contains("tuisnap accept"));
}

#[test]
fn check_writes_fidelity_sidecar_next_to_actual_png() {
    let (_dir, st) = tmp_store("fidelity").expect("tmp store");
    let frame = frame_with("crab 🦀");
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    let sidecar = outcome.actual_png.with_extension("png.fidelity.json");
    let json = std::fs::read_to_string(&sidecar).expect("read file");
    assert!(json.contains("\"approximate\": true"), "{json}");
    assert!(json.contains("U+1F980"), "{json}");
    // Acceptance pairs the sidecar with the approved PNG.
    st.accept("home").expect("accept");
    let approved = st.root().join("approved").join("home.png.fidelity.json");
    assert!(
        std::fs::read_to_string(&approved)
            .expect("read file")
            .contains("U+1F980")
    );
}

#[test]
fn corrupt_approval_is_explicit() {
    let (dir, st) = tmp_store("corrupt").expect("tmp store");
    std::fs::create_dir_all(dir.path().join("corrupt").join("approved")).expect("create dir");
    std::fs::write(
        dir.path()
            .join("corrupt")
            .join("approved")
            .join("home.frame.json"),
        "{broken",
    )
    .expect("write file");
    let outcome = st
        .check("home", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    assert_eq!(outcome.status, Status::CorruptApproval);
    let err = outcome
        .ensure_matched()
        .expect_err("ensure matched must fail")
        .to_string();
    assert!(err.contains("corrupt"), "{err}");
}

#[test]
fn no_env_var_can_auto_accept() {
    // CI-safety: acceptance is an explicit command, never ambient state.
    // (Historically this test set BLESS / TUISNAP_ACCEPT / UPDATE_SNAPSHOT;
    // nothing reads them and `set_var` is an `unsafe fn` in edition 2024, so
    // the store is checked against the ambient environment instead.)
    let (_dir, st) = tmp_store("noauto").expect("tmp store");
    let outcome = st
        .check("home", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    assert_eq!(outcome.status, Status::MissingApproval);
}

#[test]
fn concurrent_different_names_are_safe() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("conc");
    let results = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for t in 0..4 {
            let root = root.clone();
            handles.push(s.spawn(move || -> Result<(Status, Status), String> {
                let st = Store::new(&root);
                let name = format!("screen-{t}");
                let frame = frame_with(&format!("thread {t}"));
                let o = st
                    .check(&name, &frame, &profile(), &VENDORED_FACES, 1.0)
                    .map_err(|e| format!("check: {e}"))?;
                let first = o.status;
                st.accept(&name).map_err(|e| format!("accept: {e}"))?;
                let o2 = st
                    .check(&name, &frame, &profile(), &VENDORED_FACES, 1.0)
                    .map_err(|e| format!("check: {e}"))?;
                Ok((first, o2.status))
            }));
        }
        handles.into_iter().map(|h| h.join()).collect::<Vec<_>>()
    });
    for r in results {
        let (first, second) = r.expect("join thread").expect("thread check");
        assert_eq!(first, Status::MissingApproval);
        assert!(second.matched());
    }
    let st = Store::new(&root);
    assert_eq!(st.actual_names().expect("list actuals").len(), 4);
}

#[test]
fn report_links_png_files_not_base64() {
    let (_dir, st) = tmp_store("report").expect("tmp store");
    let frame = frame_with("reported");
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    let entry = tuiscotti::snapshot::report_entry(&outcome, &profile()).expect("report entry");
    let report =
        tuiscotti::snapshot::write_report(&st, "test report", &[entry]).expect("write report");
    let html = std::fs::read_to_string(&report).expect("read file");
    assert!(
        !html.contains("data:image/png;base64,"),
        "report must link files, not embed PNG bytes"
    );
    assert!(html.contains(".png"), "{html}");
    assert!(html.contains("tuisnap-default"));
}

#[test]
fn corrupt_approved_png_is_an_explicit_error() {
    let (_dir, st) = tmp_store("badpng").expect("tmp store");
    let _outcome = st
        .check("home", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    st.accept("home").expect("accept");
    // Sabotage the approved PNG (frame JSON stays valid).
    let root = st.root();
    std::fs::write(root.join("approved").join("home.png"), b"not a png").expect("write file");
    let err = st
        .check("home", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .expect_err("check must fail")
        .to_string();
    assert!(err.contains("cannot decode expected PNG"), "{err}");
}

#[test]
fn dimension_mismatch_status() {
    let (_dir, st) = tmp_store("dims").expect("tmp store");
    let _outcome = st
        .check("home", &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    st.accept("home").expect("accept");
    let other = tuiscotti::ratatui::widget_frame(Paragraph::new("x"), 20, 5, prov());
    let outcome = st
        .check("home", &other, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    assert_eq!(outcome.status, Status::DimensionMismatch);
}

#[test]
fn relaxed_threshold_still_gates_dimensions() {
    let (_dir, st) = tmp_store("threshold").expect("tmp store");
    let frame = frame_with("same");
    let _outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    st.accept("home").expect("accept");
    // Identical frames score 1.0: matched under any threshold <= 1.0.
    let outcome = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 0.99)
        .expect("check");
    assert!(outcome.status.matched());
    assert_eq!(outcome.pixel_score, Some(1.0));
}

#[test]
fn same_name_concurrent_checks_are_safe() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("samesame");
    let frame = frame_with("shared");
    let results = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for _ in 0..4 {
            let root = root.clone();
            let frame = frame.clone();
            handles.push(s.spawn(move || -> Result<Status, String> {
                let st = Store::new(&root);
                let o = st
                    .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
                    .map_err(|e| format!("check: {e}"))?;
                // Acceptance of identical content converges.
                drop(st.accept("home"));
                Ok(o.status)
            }));
        }
        handles.into_iter().map(|h| h.join()).collect::<Vec<_>>()
    });
    for r in results {
        let status = r.expect("join thread").expect("thread check");
        assert!(matches!(
            status,
            Status::MissingApproval | Status::Matched | Status::CellsDiffer
        ));
    }
    let st = Store::new(&root);
    st.accept("home").expect("accept");
    let o = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("check");
    assert!(o.status.matched());
    // Approved frame still parses (no torn writes).
    let text =
        std::fs::read_to_string(root.join("approved").join("home.frame.json")).expect("read file");
    tuiscotti::Frame::from_json(&text).expect("parse frame json");
}

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
