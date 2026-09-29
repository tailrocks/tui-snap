use super::*;
use tuiscotti::VENDORED_FACES;
use tuiscotti::snapshot::Status;

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
