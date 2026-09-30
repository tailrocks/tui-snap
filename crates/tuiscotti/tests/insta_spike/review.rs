use super::fixtures::*;
use super::harness::*;
use super::*;
use std::fs;
use tuiscotti::diff::AlphaPolicy;
use tuiscotti::screen::canonical_string;

#[test]
fn compound_canonical_plus_png_green() {
    let (_tmp, dir) = fresh_dir("green").expect("fresh_dir succeeds");
    write_text_snap(
        &dir,
        "shot",
        GEN1,
        &canonical_string(&screen_gen1().expect("screen_gen1 succeeds")),
    )
    .expect("write_text_snap succeeds");
    write_binary_snap(
        &dir,
        "shot_img",
        GEN1,
        &png_gen1().expect("png_gen1 succeeds"),
    )
    .expect("write_binary_snap succeeds");

    run_canonical(
        &dir,
        "shot",
        &screen_gen1().expect("screen_gen1 succeeds"),
        GEN1,
    )
    .expect("canonical must pass");
    run_png(
        &dir,
        "shot_img",
        png_gen1().expect("png_gen1 succeeds"),
        GEN1,
        AlphaPolicy::StraightRgba,
    )
    .expect("png must pass");
    check_consistent(&dir, "shot", "shot_img").expect("generations must agree");

    // Same pixels, different compressed bytes: passes through the macro path
    // (a byte comparator would fail here).
    copy_approved(&dir, "shot_img", "shot_reenc").expect("copy_approved succeeds");
    let reenc = png_insert_text(
        &encode_png(
            &rgba_image(&pixels_gen1().expect("pixels_gen1 succeeds"), 4, 4)
                .expect("rgba_image succeeds"),
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::NoFilter,
        )
        .expect("encode_png succeeds"),
        PNG_GEN_KEYWORD,
        GEN1,
    )
    .expect("png_insert_text succeeds");
    let approved = fs::read(dir.join("shot_reenc.snap.png")).expect("fs::read succeeds");
    assert_ne!(approved, reenc, "setup: encodings must differ");
    run_png(&dir, "shot_reenc", reenc, GEN1, AlphaPolicy::StraightRgba)
        .expect("re-encoded pixels must pass");
}

#[test]
fn reject_one_artifact_breaks_compound() {
    if !require_pending_mode() {
        return;
    }
    let (_tmp, dir) = fresh_dir("reject").expect("fresh_dir succeeds");
    write_text_snap(
        &dir,
        "rj_c",
        GEN1,
        &canonical_string(&screen_gen1().expect("screen_gen1 succeeds")),
    )
    .expect("write_text_snap succeeds");
    write_binary_snap(&dir, "rj_p", GEN1, &png_gen1().expect("png_gen1 succeeds"))
        .expect("write_binary_snap succeeds");
    let approved_c = fs::read(dir.join("rj_c.snap")).expect("fs::read succeeds");
    let approved_p = fs::read(dir.join("rj_p.snap")).expect("fs::read succeeds");
    let approved_png = fs::read(dir.join("rj_p.snap.png")).expect("fs::read succeeds");

    // New generation fails against gen1 approvals; pendings are written.
    let c1 = run_canonical(
        &dir,
        "rj_c",
        &screen_gen2().expect("screen_gen2 succeeds"),
        GEN2,
    );
    let p1 = run_png(
        &dir,
        "rj_p",
        png_gen2().expect("png_gen2 succeeds"),
        GEN2,
        AlphaPolicy::StraightRgba,
    );
    assert!(c1.is_err() && p1.is_err(), "gen2 must fail vs gen1");
    assert!(dir.join("rj_c.snap.new").exists() && dir.join("rj_p.snap.new").exists());
    // INSTA_UPDATE=no never blesses: approvals byte-identical.
    assert_eq!(
        fs::read(dir.join("rj_c.snap")).expect("fs::read succeeds"),
        approved_c
    );
    assert_eq!(
        fs::read(dir.join("rj_p.snap")).expect("fs::read succeeds"),
        approved_p
    );
    assert_eq!(
        fs::read(dir.join("rj_p.snap.png")).expect("fs::read succeeds"),
        approved_png
    );

    // Review: accept canonical, reject PNG.
    accept_sim(&dir, "rj_c").expect("accept_sim succeeds");
    reject_sim(&dir, "rj_p");
    assert!(!dir.join("rj_p.snap.new").exists());

    // Mixed baseline: canonical gen-002, PNG gen-001.
    let err = check_consistent(&dir, "rj_c", "rj_p").expect_err("mixed baseline is an error");
    assert!(err.contains("mixed compound baseline"), "{err}");

    // Re-run (fresh names over copied approved state): canonical passes,
    // PNG fails, consistency still red.
    copy_approved(&dir, "rj_c", "rj_c2").expect("copy_approved succeeds");
    copy_approved(&dir, "rj_p", "rj_p2").expect("copy_approved succeeds");
    run_canonical(
        &dir,
        "rj_c2",
        &screen_gen2().expect("screen_gen2 succeeds"),
        GEN2,
    )
    .expect("accepted canonical passes");
    assert!(
        run_png(
            &dir,
            "rj_p2",
            png_gen2().expect("png_gen2 succeeds"),
            GEN2,
            AlphaPolicy::StraightRgba
        )
        .is_err()
    );
    check_consistent(&dir, "rj_c2", "rj_p2").expect_err("mixed baseline must stay red");
}

#[test]
fn partial_accept_breaks_compound() {
    if !require_pending_mode() {
        return;
    }
    let (_tmp, dir) = fresh_dir("partial").expect("fresh_dir succeeds");
    write_text_snap(
        &dir,
        "pa_c",
        GEN1,
        &canonical_string(&screen_gen1().expect("screen_gen1 succeeds")),
    )
    .expect("write_text_snap succeeds");
    write_binary_snap(&dir, "pa_p", GEN1, &png_gen1().expect("png_gen1 succeeds"))
        .expect("write_binary_snap succeeds");

    assert!(
        run_canonical(
            &dir,
            "pa_c",
            &screen_gen2().expect("screen_gen2 succeeds"),
            GEN2
        )
        .is_err()
    );
    assert!(
        run_png(
            &dir,
            "pa_p",
            png_gen2().expect("png_gen2 succeeds"),
            GEN2,
            AlphaPolicy::StraightRgba
        )
        .is_err()
    );

    // Review accepts canonical only; PNG pending left in place.
    accept_sim(&dir, "pa_c").expect("accept_sim succeeds");
    assert!(
        dir.join("pa_p.snap.new").exists(),
        "png review still pending"
    );

    check_consistent(&dir, "pa_c", "pa_p").expect_err("partial accept must be red");

    copy_approved(&dir, "pa_c", "pa_c2").expect("copy_approved succeeds");
    copy_approved(&dir, "pa_p", "pa_p2").expect("copy_approved succeeds");
    run_canonical(
        &dir,
        "pa_c2",
        &screen_gen2().expect("screen_gen2 succeeds"),
        GEN2,
    )
    .expect("accepted canonical passes");
    assert!(
        run_png(
            &dir,
            "pa_p2",
            png_gen2().expect("png_gen2 succeeds"),
            GEN2,
            AlphaPolicy::StraightRgba
        )
        .is_err()
    );
    check_consistent(&dir, "pa_c2", "pa_p2").expect_err("partial accept must stay red");
}

#[test]
fn interrupted_write_breaks_compound() {
    if !require_pending_mode() {
        return;
    }
    let (_tmp, dir) = fresh_dir("interrupted").expect("fresh_dir succeeds");
    write_text_snap(
        &dir,
        "iw_c",
        GEN1,
        &canonical_string(&screen_gen1().expect("screen_gen1 succeeds")),
    )
    .expect("write_text_snap succeeds");
    write_binary_snap(&dir, "iw_p", GEN1, &png_gen1().expect("png_gen1 succeeds"))
        .expect("write_binary_snap succeeds");

    assert!(
        run_canonical(
            &dir,
            "iw_c",
            &screen_gen2().expect("screen_gen2 succeeds"),
            GEN2
        )
        .is_err()
    );
    assert!(
        run_png(
            &dir,
            "iw_p",
            png_gen2().expect("png_gen2 succeeds"),
            GEN2,
            AlphaPolicy::StraightRgba
        )
        .is_err()
    );

    // Crash mid-review: PNG sidecar pending lost (torn write), metadata left.
    fs::remove_file(dir.join("iw_p.snap.new.png")).expect("fs::remove_file succeeds");
    assert!(dir.join("iw_p.snap.new").exists());
    // Accept of a torn binary pending is refused: no metadata-without-pixels.
    accept_sim(&dir, "iw_p").expect_err("torn pending must refuse accept");
    // Canonical accepted; crash "resolved" by deleting the torn PNG pending.
    accept_sim(&dir, "iw_c").expect("accept_sim succeeds");
    reject_sim(&dir, "iw_p");

    // Looks clean (no pendings) but mixed: consistency gate catches it.
    assert!(!dir.join("iw_c.snap.new").exists() && !dir.join("iw_p.snap.new").exists());
    check_consistent(&dir, "iw_c", "iw_p").expect_err("post-crash mix must be red");

    // Re-run: canonical passes, PNG fails and its pending regenerates —
    // nothing was silently lost or blessed.
    copy_approved(&dir, "iw_c", "iw_c2").expect("copy_approved succeeds");
    copy_approved(&dir, "iw_p", "iw_p2").expect("copy_approved succeeds");
    run_canonical(
        &dir,
        "iw_c2",
        &screen_gen2().expect("screen_gen2 succeeds"),
        GEN2,
    )
    .expect("accepted canonical passes");
    assert!(
        run_png(
            &dir,
            "iw_p2",
            png_gen2().expect("png_gen2 succeeds"),
            GEN2,
            AlphaPolicy::StraightRgba
        )
        .is_err()
    );
    assert!(
        dir.join("iw_p2.snap.new").exists(),
        "pending must regenerate"
    );
    check_consistent(&dir, "iw_c2", "iw_p2").expect_err("post-crash mix must stay red");
}
