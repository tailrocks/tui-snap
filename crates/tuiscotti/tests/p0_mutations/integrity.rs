use super::*;
use image::codecs::png::{CompressionType, FilterType};
use tuiscotti::VENDORED_FACES;
use tuiscotti::snapshot::Status;

// ------ C08: transactional candidate generation / consistent approval
#[test]
fn c08_interrupted_candidate_must_report_incomplete_not_match() {
    // Case A (classic store): report RE-RENDERS from the actual frame
    // (`Store::report_with` → `check_with`), silently healing an interrupted
    // candidate (frame present, PNG write lost) back to Matched.
    let (_dir, st) = tmp_classic("c08");
    let frame = frame_with("atomic candidate");
    let _ = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    st.accept("home").unwrap();
    let matched = st
        .check("home", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(matched.status, Status::Matched, "setup: must match first");

    // Simulate interruption: actual frame survived, actual PNG did not.
    let actual_png = matched.actual_png.clone();
    std::fs::remove_file(&actual_png).unwrap();
    let sidecar = actual_png.with_extension("png.fidelity.json");
    let _ = std::fs::remove_file(&sidecar);
    assert!(!actual_png.exists(), "setup: actual PNG must be gone");

    let mut renderer = tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES).unwrap();
    let report = st.report_with(&mut renderer, 1.0, "c08").unwrap();
    let reported = &report.outcomes[0];
    assert!(
        !reported.status.matched(),
        "C08 gap (classic): interrupted candidate (actual frame without PNG) \
         reports {:?}; an absent completion must be incomplete, never pass",
        reported.status
    );

    // Case B (grouped store): a missing actual PNG is misattributed as a
    // pixel difference (`disk_status` byte-compares), not flagged as
    // incomplete/missing evidence.
    let (_gdir, gst) = tmp_grouped("c08g");
    gst.check("s", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    gst.accept("s").unwrap();
    let gmatched = gst
        .check("s", &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    assert_eq!(
        gmatched.status(),
        Status::Matched,
        "setup: must match first"
    );
    std::fs::remove_file(&gmatched.actual.png).unwrap();
    let mut grenderer = tuiscotti::render::Renderer::new(&profile(), &VENDORED_FACES).unwrap();
    let greport = gst.report_with(&mut grenderer, 1.0, "c08g").unwrap();
    let greported = &greport.outcomes[0];
    assert_eq!(
        greported.status,
        Status::MissingApproval,
        "C08 gap (grouped): interrupted candidate (actual frame without PNG) \
         reports {:?} instead of missing/incomplete evidence",
        greported.status
    );
}

// ------ C02: actual evidence from candidate, never approved
#[test]
fn c02_actual_evidence_comes_from_candidate_never_approved() {
    // Gap (src/grouped.rs tiered fast path): when the ansi/txt cell gates
    // passed, check COPIED approved PNG/HTML bytes into actual/ instead of
    // rendering the candidate — fabricating html_match=true and pixel
    // score 1.0 without rendering, and masking approved-side tamper.
    let (_dir, gst) = tmp_grouped("c02");
    let name = "c02/evidence";
    let frame = frame_with("candidate evidence");
    gst.check(name, &frame, &profile(), &VENDORED_FACES, 1.0)
        .unwrap();
    gst.accept(name).unwrap();

    // Sabotage approved RENDER bytes only (cells unchanged): re-encode the
    // approved PNG (same decoded pixels, different bytes) and perturb the
    // approved HTML. Any approved→actual byte copy is then detectable.
    let approved_png_path = gst.approved_root().join(format!("{name}.png"));
    let approved_html_path = gst.approved_root().join(format!("{name}.html"));
    let approved_png_before = std::fs::read(&approved_png_path).unwrap();
    let decoded_rgba = image::load_from_memory(&approved_png_before)
        .unwrap()
        .to_rgba8();
    let reencoded = encode_rgba(&decoded_rgba, CompressionType::Best, FilterType::NoFilter);
    assert_ne!(
        approved_png_before, reencoded,
        "setup: re-encode must change bytes while keeping pixels"
    );
    assert_eq!(
        image::load_from_memory(&reencoded)
            .unwrap()
            .to_rgba8()
            .as_raw(),
        decoded_rgba.as_raw(),
        "setup: re-encode must keep decoded pixels"
    );
    std::fs::write(&approved_png_path, &reencoded).unwrap();
    let approved_html_before = std::fs::read(&approved_html_path).unwrap();
    let mut tampered_html = approved_html_before.clone();
    tampered_html.extend_from_slice(b"\n<!-- c02 tamper -->\n");
    std::fs::write(&approved_html_path, &tampered_html).unwrap();

    // Fresh render of the CANDIDATE frame, straight from the renderer.
    let mut renderer = profile().renderer(&VENDORED_FACES).unwrap();
    let fresh = renderer.render_artifacts(&frame, name).unwrap();

    // The one check path must write fresh candidate renders as evidence:
    // there is no skip-render path that could copy approved bytes (C02).
    let mut r = profile().renderer(&VENDORED_FACES).unwrap();
    let outcome = gst.check_with(&mut r, name, &frame, 1.0).unwrap();
    assert_eq!(outcome.ansi_match, Some(true), "setup: cells unchanged");
    assert_eq!(outcome.txt_match, Some(true), "setup: cells unchanged");
    let actual_png = std::fs::read(&outcome.actual.png).unwrap();
    let actual_html = std::fs::read(&outcome.actual.html).unwrap();
    assert_eq!(
        actual_png, fresh.png,
        "C02 gap: actual PNG must be a fresh render of the candidate frame, \
         not approved bytes"
    );
    assert_eq!(
        actual_html,
        fresh.html.as_bytes(),
        "C02 gap: actual HTML must be a fresh render of the candidate frame, \
         not approved bytes"
    );
    assert_ne!(
        actual_png, reencoded,
        "C02 gap: actual PNG copies sabotaged approved bytes"
    );
    assert_ne!(
        actual_html, tampered_html,
        "C02 gap: actual HTML copies sabotaged approved bytes"
    );
    // The render-level gate now sees the tamper (PNG pixels still match,
    // so only HTML falls).
    assert_eq!(
        outcome.status(),
        Status::PixelsDiffer,
        "tampered approved HTML must fail the render-level gate"
    );
    assert_eq!(outcome.html_match, Some(false));
}
