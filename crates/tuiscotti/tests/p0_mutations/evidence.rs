use super::*;
use image::codecs::png::{CompressionType, FilterType};
use tuiscotti::VENDORED_FACES;
use tuiscotti::snapshot::Status;

// ----------------- C06: missing/corrupt reference artifacts must fail
#[test]
fn c06_missing_approved_png_must_fail_not_regenerate_in_memory() {
    // Classic store gap (src/snapshot.rs `check_with`: missing approved PNG
    // is rendered to MEMORY and the gate can still return Matched).
    let (_dir, st) = tmp_classic("c06").expect("tmp_classic succeeds");
    let frame = frame_with("frozen pixels");
    drop(
        st.check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
            .expect("st .check(\"home\", &frame, &profile(), &VENDORED_FACES, 1.0) succeeds"),
    );
    st.accept("home").expect("st.accept(\"home\") succeeds");
    let matched = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("st .check(\"home\", &frame, &profile(), &VENDORED_FACES, 1.0) succeeds");
    assert_eq!(matched.status, Status::Matched, "setup: must match first");

    std::fs::remove_file(st.root().join("approved").join("home.png"))
        .expect("std::fs::remove_file(st.root().join(\"approved\").join(\"home.png\")) succeeds");
    let gap = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("st .check(\"home\", &frame, &profile(), &VENDORED_FACES, 1.0) succeeds");
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
    let (_gdir, gst) = tmp_grouped("c06g").expect("tmp_grouped succeeds");
    gst.check("s", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("gst.check(\"s\", &frame, &profile(), &VENDORED_FACES, 1.0) succeeds");
    gst.accept("s").expect("gst.accept(\"s\") succeeds");
    std::fs::remove_file(gst.approved_root().join("s.png"))
        .expect("std::fs::remove_file(gst.approved_root().join(\"s.png\")) succeeds");
    let grouped = gst
        .check("s", &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("gst .check(\"s\", &frame, &profile(), &VENDORED_FACES, 1.0) succeeds");
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
    let (_dir, gst) = tmp_grouped("c05").expect("tmp_grouped succeeds");
    let name = "verdict/parity";
    let frame = frame_with("same verdict everywhere");
    gst.check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("gst.check(name, &frame, &profile(), &VENDORED_FACES, 1.0) succeeds");
    gst.accept(name).expect("gst.accept(name) succeeds");
    let checked = gst
        .check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .expect("gst .check(name, &frame, &profile(), &VENDORED_FACES, 1.0) succeeds");
    assert_eq!(checked.status(), Status::Matched, "setup: check must match");

    // Same decoded pixels, different compressed bytes, written as the actual.
    let actual_png = checked.actual.png.clone();
    let approved_bytes = std::fs::read(gst.approved_root().join(format!("{name}.png")))
        .expect("std::fs::read(gst.approved_root().join(format!(\"{name}.png\"))) succeeds");
    let decoded = decode_rgb(&approved_bytes).expect("decode_rgb succeeds");
    let reencoded = encode_rgb(&decoded, CompressionType::Best, FilterType::NoFilter)
        .expect("encode_rgb succeeds");
    assert_ne!(
        approved_bytes, reencoded,
        "setup: re-encode must change bytes while keeping pixels"
    );
    assert_eq!(
        decode_rgb(&reencoded)
            .expect("decode_rgb succeeds")
            .as_raw(),
        decoded.as_raw(),
        "setup: re-encode must keep decoded pixels"
    );
    std::fs::write(&actual_png, &reencoded)
        .expect("std::fs::write(&actual_png, &reencoded) succeeds");

    let mut renderer = tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES)
        .expect("tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES) succeeds");
    let report = gst
        .report_with(&mut renderer, 1.0, "c05")
        .expect("gst.report_with(&mut renderer, 1.0, \"c05\") succeeds");
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
    let (_dir, st) = tmp_classic("c07").expect("tmp_classic succeeds");
    drop(st        .check(
            "home",
            &frame_with("before"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .expect("st .check( \"home\", &frame_with(\"before\"), &profile(), &VENDORED_FACES, 1.0, ) succeeds")
    );
    st.accept("home").expect("st.accept(\"home\") succeeds");
    let mismatch = st
        .check(
            "home",
            &frame_with("after"),
            &profile(),
            &VENDORED_FACES,
            1.0,
        )
        .expect("st .check( \"home\", &frame_with(\"after\"), &profile(), &VENDORED_FACES, 1.0, ) succeeds");
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
        .expect("st .check( \"home\", &frame_with(\"after\"), &profile(), &VENDORED_FACES, 1.0, ) succeeds");
    assert!(
        fresh.ensure_matched().is_err(),
        "ensure_matched must error on mismatch"
    );
}
