use super::*;
use image::codecs::png::{CompressionType, FilterType};
use tuiscotti::VENDORED_FACES;
use tuiscotti::snapshot::Status;

// ----------------- C06: missing/corrupt reference artifacts must fail
#[test]
fn c06_missing_approved_png_must_fail_not_regenerate_in_memory() {
    // Classic store gap (src/snapshot.rs `check_with`: missing approved PNG
    // is rendered to MEMORY and the gate can still return Matched).
    let (_dir, st) = tmp_classic("c06");
    let frame = frame_with("frozen pixels");
    let _ = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    let matched = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(matched.status, Status::Matched, "setup: must match first");

    std::fs::remove_file(st.root().join("approved").join("home.png")).unwrap();
    let gap = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(
        gap.status,
        Status::MissingApproval,
        "C06 gap: missing approved PNG must fail closed, got {:?} \
         (approved_png_regenerated={})",
        gap.status,
        gap.approved_png_regenerated
    );
    assert!(
        !gap.approved_png_regenerated,
        "C06 gap: frozen visual mode must not regenerate expected images in memory"
    );

    // Guard: the grouped store already fails closed here — keep it that way.
    let (_gdir, gst) = tmp_grouped("c06g");
    gst.check("s", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    gst.accept("s").unwrap();
    std::fs::remove_file(gst.approved_root().join("s.png")).unwrap();
    let grouped = gst
        .check("s", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(
        grouped.status(),
        Status::MissingApproval,
        "guard: grouped store must stay fail-closed on missing approved PNG"
    );
}

// ----------------------- C05: report/test verdict unification
#[test]
fn c05_report_verdict_must_equal_test_verdict_on_same_inputs() {
    // Gap: `GroupedStore::report_with` IGNORES pixel_threshold
    // (`_pixel_threshold`, src/grouped.rs) and byte-compares compressed PNGs
    // (`disk_status`), while `check` compares DECODED pixels. Re-encoding the
    // same pixels with different bytes splits the two verdicts.
    let (_dir, gst) = tmp_grouped("c05");
    let name = "verdict/parity";
    let frame = frame_with("same verdict everywhere");
    gst.check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    gst.accept(name).unwrap();
    let checked = gst
        .check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(checked.status(), Status::Matched, "setup: check must match");

    // Same decoded pixels, different compressed bytes, written as the actual.
    let actual_png = checked.actual.png.clone();
    let approved_bytes = std::fs::read(gst.approved_root().join(format!("{name}.png"))).unwrap();
    let decoded = decode_rgb(&approved_bytes);
    let reencoded = encode_rgb(&decoded, CompressionType::Best, FilterType::NoFilter);
    assert_ne!(
        approved_bytes, reencoded,
        "setup: re-encode must change bytes while keeping pixels"
    );
    assert_eq!(
        decode_rgb(&reencoded).as_raw(),
        decoded.as_raw(),
        "setup: re-encode must keep decoded pixels"
    );
    std::fs::write(&actual_png, &reencoded).unwrap();

    let mut renderer = tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES).unwrap();
    let report = gst.report_with(&mut renderer, 1.0, "c05").unwrap();
    let reported = report
        .outcomes
        .iter()
        .find(|o| o.name == name)
        .expect("report must cover the scenario");
    assert_eq!(
        reported.status,
        checked.status(),
        "C05 gap: report says {:?} but check said {:?} for identical decoded \
         pixels (report byte-compares PNGs, check compares decoded pixels)",
        reported.status,
        checked.status()
    );
}

// ------------------------------- C07: assertions hard to ignore
#[test]
fn c07_dropped_mismatch_outcome_must_not_silently_pass() {
    let (_dir, st) = tmp_classic("c07");
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
    let mismatch = st
        .check(
            "home",
            &frame_with("after"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    assert_eq!(mismatch.status, Status::CellsDiffer, "setup: must mismatch");

    // `CompareOutcome`/`Status` are #[must_use] (src/snapshot.rs), so a
    // caller that forgets `ensure_matched()` gets an `unused_must_use`
    // warning; `drop` here documents the previously silent scenario.
    drop(mismatch);

    // Runtime half: `ensure_matched` fails loudly on mismatch.
    let fresh = st
        .check(
            "home",
            &frame_with("after"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .unwrap();
    assert!(
        fresh.ensure_matched().is_err(),
        "ensure_matched must error on mismatch"
    );
}
