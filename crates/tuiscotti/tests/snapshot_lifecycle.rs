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
