use super::*;
use ratatui::widgets::Paragraph;
use tuiscotti::VENDORED_FACES;
use tuiscotti::snapshot::{Status, Store, write_atomic};

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
        handles
            .into_iter()
            .map(std::thread::ScopedJoinHandle::join)
            .collect::<Vec<_>>()
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
    assert!(html.contains("tuiscotti-default"));
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

#[cfg(unix)]
#[test]
fn symlink_plant_at_target_cannot_redirect_snapshot_writes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("actual").join("home.frame.json");
    std::fs::create_dir_all(target.parent().expect("target parent")).expect("mkdir");
    let victim = dir.path().join("victim");
    std::fs::write(&victim, b"sentinel").expect("write victim");
    std::os::unix::fs::symlink(&victim, &target).expect("plant symlink");
    write_atomic(&target, b"snapshot-bytes").expect("write over a planted link succeeds");
    // The plant was replaced, never followed: victim intact, live name real.
    assert_eq!(std::fs::read(&victim).expect("read victim"), b"sentinel");
    assert!(
        !std::fs::symlink_metadata(&target)
            .expect("symlink_metadata succeeds")
            .file_type()
            .is_symlink(),
        "live target must not be a symlink"
    );
    assert_eq!(
        std::fs::read(&target).expect("read target"),
        b"snapshot-bytes"
    );
}

#[cfg(unix)]
#[test]
fn symlink_plants_at_tmp_names_fail_closed_without_following() {
    // Tmp names carry pid + a process-wide counter + thread id. The counter
    // value at this call is unknown (shared with every parallel test in the
    // binary), so plants cover a counter range on THIS thread with wide
    // margin over this binary's write volume: a hit fails closed (exclusive
    // create) and a miss writes normally — the victim is unreachable either
    // way.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("actual")).expect("mkdir");
    let victim = dir.path().join("victim");
    std::fs::write(&victim, b"sentinel").expect("write victim");
    let target = dir.path().join("actual").join("home.frame.json");
    let pid = std::process::id();
    let tid = format!("{:?}", std::thread::current().id());
    for n in 0..1024u64 {
        let tmp = target.with_extension(format!("tmp.{pid}.{n}.{tid}"));
        std::os::unix::fs::symlink(&victim, &tmp).expect("plant symlink");
    }
    // Hit (a plant matched): Err, nothing written. Miss: Ok, bytes landed.
    // Both are correct; the victim surviving is the property under test.
    match write_atomic(&target, b"snapshot-bytes") {
        Ok(()) => assert_eq!(
            std::fs::read(&target).expect("read target"),
            b"snapshot-bytes"
        ),
        Err(e) => assert!(!target.exists(), "failed write publishes nothing: {e}"),
    }
    assert_eq!(std::fs::read(&victim).expect("read victim"), b"sentinel");
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
        handles
            .into_iter()
            .map(std::thread::ScopedJoinHandle::join)
            .collect::<Vec<_>>()
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
