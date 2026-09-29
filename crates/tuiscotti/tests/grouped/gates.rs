use super::*;
use tuiscotti::VENDORED_FACES;
use tuiscotti::grouped::validate_name;
use tuiscotti::snapshot::Status;

#[test]
fn name_validation_rejects_unsafe_names() {
    for bad in [
        "",
        "/absolute/path",
        "a/../b",
        "..",
        "../escape",
        "a//b",
        "/",
        "a/",
        "a\\b",
        "C:\\snapshots\\x",
        "a/./b",
    ] {
        assert!(validate_name(bad).is_err(), "must reject {bad:?}");
        let (_dir, st) = tmp_store("validate");
        let err = st
            .check(bad, &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid snapshot name"), "{err}");
        assert!(st.accept(bad).is_err(), "accept must reject {bad:?}");
    }
    for good in ["home", "a/b/c", "showcase/pages/overview_120x40_truecolor"] {
        validate_name(good).unwrap();
    }
}

#[test]
fn cell_change_fails_ansi_gate_as_cells_differ() {
    let name = "flows/checkout/step1";
    let (_dir, st) = tmp_store("cells");
    st.check(
        name,
        &frame_with("before"),
        &profile(),
        &VENDORED_FACES,
        1.0,
    )
    .unwrap();
    st.accept(name).unwrap();
    let outcome = st
        .check(
            name,
            &frame_with("after!"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    assert_eq!(outcome.status(), Status::CellsDiffer);
    assert_eq!(outcome.ansi_match, Some(false));
    assert_eq!(outcome.txt_match, Some(false));
    assert_eq!(outcome.html_match, Some(false));
    // Pixel mismatch wrote a diff PNG under the DIFF root (not approved/).
    let diff = outcome.outcome.diff_png.clone().unwrap();
    assert!(diff.exists());
    assert!(diff.starts_with(st.diff_root()), "{}", diff.display());
    let err = outcome.ensure_matched().unwrap_err().to_string();
    assert!(err.contains("cells-differ"), "{err}");
    assert!(err.contains("tuisnap accept"), "{err}");
}

#[test]
fn style_only_change_keeps_txt_equal() {
    let name = "flows/checkout/step2";
    let (_dir, st) = tmp_store("style");
    st.check(
        name,
        &frame_with("same text"),
        &profile(),
        &VENDORED_FACES,
        1.0,
    )
    .unwrap();
    st.accept(name).unwrap();
    let mut styled = frame_with("same text");
    styled.cells[0].mods.bold = true;
    let outcome = st
        .check(name, &styled, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(outcome.status(), Status::CellsDiffer);
    assert_eq!(outcome.ansi_match, Some(false), "SGR run changed");
    assert_eq!(outcome.txt_match, Some(true), "plain text unchanged");
}

#[test]
fn png_pixel_gate_honors_threshold() {
    let name = "pages/overview";
    let (_dir, st) = tmp_store("pixels");
    let frame = frame_with("pixel gate");
    st.check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept(name).unwrap();
    // Sabotage ONLY the approved PNG (render of a different screen): the
    // byte gates still pass, isolating the decoded-pixel gate.
    let other_png = {
        let profile = profile();
        let mut renderer = profile.renderer(&VENDORED_FACES).unwrap();
        renderer.render(&frame_with("pixel gate!")).unwrap().png
    };
    std::fs::write(st.approved_root().join(format!("{name}.png")), &other_png).unwrap();

    let mut renderer = profile().renderer(&VENDORED_FACES).unwrap();
    let strict = st.check_with(&mut renderer, name, &frame, 1.0).unwrap();
    assert_eq!(strict.status(), Status::PixelsDiffer);
    assert_eq!(strict.ansi_match, Some(true));
    assert_eq!(strict.html_match, Some(true));
    let score = strict.outcome.pixel_score.unwrap();
    assert!(score < 1.0, "score {score}");
    assert!(strict.outcome.diff_png.as_ref().unwrap().exists());

    // Same comparison passes under a threshold at/below the score.
    let relaxed = st.check_with(&mut renderer, name, &frame, 0.0).unwrap();
    assert_eq!(relaxed.status(), Status::Matched);
    assert_eq!(relaxed.outcome.pixel_score, Some(score));
}

#[test]
fn missing_single_artifact_fails_closed() {
    let name = "a/b";
    let (_dir, st) = tmp_store("partial");
    st.check(name, &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept(name).unwrap();
    std::fs::remove_file(st.approved_root().join(format!("{name}.txt"))).unwrap();
    let outcome = st
        .check(name, &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(outcome.status(), Status::MissingApproval);
    assert!(
        outcome.outcome.note.contains(".txt"),
        "{}",
        outcome.outcome.note
    );
}

#[test]
fn corrupt_approved_png_is_an_explicit_error() {
    let name = "a/b";
    let (_dir, st) = tmp_store("corruptpng");
    st.check(name, &frame_with("x"), &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept(name).unwrap();
    std::fs::write(st.approved_root().join(format!("{name}.png")), b"not a png").unwrap();
    let mut renderer = profile().renderer(&VENDORED_FACES).unwrap();
    let err = st
        .check_with(&mut renderer, name, &frame_with("x"), 1.0)
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot decode expected PNG"), "{err}");
}
