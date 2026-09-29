use super::fixtures::*;
use super::harness::*;
use super::*;
use std::fs;
use tuiscotti::diff::AlphaPolicy;
use tuiscotti::insta_proto::{PngPixelComparator, insta_string, insta_value};

#[test]
fn projection_deterministic_and_complete() {
    let g1 = screen_gen1();
    assert_eq!(insta_string(&g1), insta_string(&screen_gen1()));
    assert_eq!(insta_value(&g1), insta_value(&screen_gen1()));
    assert_ne!(insta_string(&g1), insta_string(&screen_gen2()));

    let text = insta_string(&g1);
    for needle in [
        "geometry cols=4 rows=2 ox=5 oy=7",
        "cursor x=1 y=0 visible=true style=block blinking=true",
        "sym=\"A\" w=1 cont=false fg=index=1",
        "sym=\"中\" w=2 cont=false",
        "sym=\"\" w=0 cont=true",
        "bg=index=4",
        "mods=bold",
        "mods=underline",
        "mods=hidden+blink",
        "mods=reverse",
        "fg=#010203",
    ] {
        assert!(text.contains(needle), "missing {needle} in:\n{text}");
    }
    // Every cell present exactly once, row-major.
    assert_eq!(text.lines().filter(|l| l.starts_with("cell ")).count(), 8);

    let v = insta_value(&g1);
    assert_eq!(v["cols"], serde_json::json!(4));
    assert_eq!(v["rows"], serde_json::json!(2));
    assert_eq!(v["ox"], serde_json::json!(5));
    assert_eq!(v["cursor"]["blinking"], serde_json::json!(true));
    assert_eq!(v["cells"].as_array().unwrap().len(), 8);
    assert_eq!(
        v["cells"][1],
        serde_json::json!({
            "x": 1, "y": 0, "symbol": "中", "width": 2, "continuation": false,
            "fg": "default", "bg": "default",
            "mods": {"hidden": false, "blink": false, "bold": false, "dim": false,
                     "italic": false, "underline": false, "strikethrough": false,
                     "reverse": false},
        })
    );
    // tEXt round-trip + CRC sanity (standard check vector).
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    let tagged = png_insert_text(&png_gen1_no_tag_for_test(), PNG_GEN_KEYWORD, GEN1);
    let tagged = png_insert_text(&tagged, "k", "v");
    assert_eq!(png_find_text(&tagged, "k").as_deref(), Some("v"));
    assert_eq!(
        png_find_text(&tagged, PNG_GEN_KEYWORD).as_deref(),
        Some(GEN1)
    );
    // Comparator is Settings-compatible.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PngPixelComparator>();
    let c = PngPixelComparator::new(AlphaPolicy::Opaque);
    assert_eq!(c.alpha_policy(), AlphaPolicy::Opaque);
    let _clone: Box<dyn insta::Comparator> =
        <PngPixelComparator as insta::Comparator>::dyn_clone(&c);
}

#[test]
fn comparator_matches_decoded_pixels() {
    use insta::Comparator as _;
    let (_tmp, dir) = fresh_dir("comparator");
    let cmp = PngPixelComparator::new(AlphaPolicy::StraightRgba);

    // Reference: gen1 bytes.
    write_binary_snap(&dir, "ref", GEN1, &png_gen1());
    let reference = insta::Snapshot::from_file(&dir.join("ref.snap")).unwrap();

    // Identical bytes match.
    write_binary_snap(&dir, "same", GEN1, &png_gen1());
    let same = insta::Snapshot::from_file(&dir.join("same.snap")).unwrap();
    assert!(cmp.matches(&reference, &same));

    // Re-encoded identical pixels (different compressed bytes, no tEXt tag at
    // all) match: decoded equality, not byte equality.
    let reenc = encode_png(
        &rgba_image(&pixels_gen1(), 4, 4),
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::NoFilter,
    );
    assert_ne!(reenc, png_gen1(), "setup: encodings must differ");
    write_binary_snap(&dir, "reenc", GEN1, &reenc);
    let reenc_snap = insta::Snapshot::from_file(&dir.join("reenc.snap")).unwrap();
    assert!(cmp.matches(&reference, &reenc_snap));

    // Same pixels, different tEXt generation tag: pixels still match
    // (ancillary chunks are not pixels).
    let retagged = png_insert_text(&png_gen1_no_tag_for_test(), PNG_GEN_KEYWORD, "other");
    write_binary_snap(&dir, "retagged", "other", &retagged);
    let retagged_snap = insta::Snapshot::from_file(&dir.join("retagged.snap")).unwrap();
    assert!(cmp.matches(&reference, &retagged_snap));

    // One pixel differs: no match.
    write_binary_snap(&dir, "gen2", GEN2, &png_gen2());
    let gen2 = insta::Snapshot::from_file(&dir.join("gen2.snap")).unwrap();
    assert!(!cmp.matches(&reference, &gen2));

    // Corrupt bytes on either side never match.
    write_binary_snap(&dir, "corrupt", GEN1, b"not a png");
    let corrupt = insta::Snapshot::from_file(&dir.join("corrupt.snap")).unwrap();
    assert!(!cmp.matches(&reference, &corrupt));
    assert!(!cmp.matches(&corrupt, &reference));

    // Missing sidecar (Binary(None)) never matches, even against itself.
    write_binary_snap(&dir, "noside", GEN1, &png_gen1());
    fs::remove_file(dir.join("noside.snap.png")).unwrap();
    let noside = insta::Snapshot::from_file(&dir.join("noside.snap")).unwrap();
    assert!(!cmp.matches(&reference, &noside));
    assert!(!cmp.matches(&noside, &noside));

    // Text snapshots keep stock semantics via DefaultComparator.
    write_text_snap(&dir, "t1", GEN1, "hello\n");
    write_text_snap(&dir, "t2", GEN1, "hello\n");
    write_text_snap(&dir, "t3", GEN1, "other\n");
    let t1 = insta::Snapshot::from_file(&dir.join("t1.snap")).unwrap();
    let t2 = insta::Snapshot::from_file(&dir.join("t2.snap")).unwrap();
    let t3 = insta::Snapshot::from_file(&dir.join("t3.snap")).unwrap();
    assert!(cmp.matches(&t1, &t2));
    assert!(!cmp.matches(&t1, &t3));

    // Text/binary mix never matches.
    assert!(!cmp.matches(&reference, &t1));
    assert!(!cmp.matches(&t1, &reference));

    // Policy is explicit: semi-transparent identical pixels match under
    // StraightRgba but never under Opaque.
    let mut semi = [[0u8; 4]; 16];
    for (i, p) in semi.iter_mut().enumerate() {
        *p = [(i as u8) * 9, 40, 90, 128];
    }
    let semi_img = rgba_image(&semi, 4, 4);
    let semi_a = png_insert_text(
        &encode_png(
            &semi_img,
            image::codecs::png::CompressionType::Default,
            image::codecs::png::FilterType::Adaptive,
        ),
        PNG_GEN_KEYWORD,
        GEN1,
    );
    let semi_b = png_insert_text(
        &encode_png(
            &semi_img,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::NoFilter,
        ),
        PNG_GEN_KEYWORD,
        GEN1,
    );
    assert_ne!(semi_a, semi_b, "setup: encodings must differ");
    write_binary_snap(&dir, "semi_a", GEN1, &semi_a);
    write_binary_snap(&dir, "semi_b", GEN1, &semi_b);
    let semi_snap_a = insta::Snapshot::from_file(&dir.join("semi_a.snap")).unwrap();
    let semi_snap_b = insta::Snapshot::from_file(&dir.join("semi_b.snap")).unwrap();
    assert!(cmp.matches(&semi_snap_a, &semi_snap_b));
    let opaque_cmp = PngPixelComparator::new(AlphaPolicy::Opaque);
    assert!(!opaque_cmp.matches(&semi_snap_a, &semi_snap_b));
}

#[test]
fn compound_canonical_plus_png_green() {
    let (_tmp, dir) = fresh_dir("green");
    write_text_snap(&dir, "shot", GEN1, &insta_string(&screen_gen1()));
    write_binary_snap(&dir, "shot_img", GEN1, &png_gen1());

    run_canonical(&dir, "shot", &screen_gen1(), GEN1).expect("canonical must pass");
    run_png(
        &dir,
        "shot_img",
        png_gen1(),
        GEN1,
        AlphaPolicy::StraightRgba,
    )
    .expect("png must pass");
    check_consistent(&dir, "shot", "shot_img").expect("generations must agree");

    // Same pixels, different compressed bytes: passes through the macro path
    // (a byte comparator would fail here).
    copy_approved(&dir, "shot_img", "shot_reenc");
    let reenc = png_insert_text(
        &encode_png(
            &rgba_image(&pixels_gen1(), 4, 4),
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::NoFilter,
        ),
        PNG_GEN_KEYWORD,
        GEN1,
    );
    let approved = fs::read(dir.join("shot_reenc.snap.png")).unwrap();
    assert_ne!(approved, reenc, "setup: encodings must differ");
    run_png(&dir, "shot_reenc", reenc, GEN1, AlphaPolicy::StraightRgba)
        .expect("re-encoded pixels must pass");
}

#[test]
fn reject_one_artifact_breaks_compound() {
    if !require_pending_mode() {
        return;
    }
    let (_tmp, dir) = fresh_dir("reject");
    write_text_snap(&dir, "rj_c", GEN1, &insta_string(&screen_gen1()));
    write_binary_snap(&dir, "rj_p", GEN1, &png_gen1());
    let approved_c = fs::read(dir.join("rj_c.snap")).unwrap();
    let approved_p = fs::read(dir.join("rj_p.snap")).unwrap();
    let approved_png = fs::read(dir.join("rj_p.snap.png")).unwrap();

    // New generation fails against gen1 approvals; pendings are written.
    let c1 = run_canonical(&dir, "rj_c", &screen_gen2(), GEN2);
    let p1 = run_png(&dir, "rj_p", png_gen2(), GEN2, AlphaPolicy::StraightRgba);
    assert!(c1.is_err() && p1.is_err(), "gen2 must fail vs gen1");
    assert!(dir.join("rj_c.snap.new").exists() && dir.join("rj_p.snap.new").exists());
    // INSTA_UPDATE=no never blesses: approvals byte-identical.
    assert_eq!(fs::read(dir.join("rj_c.snap")).unwrap(), approved_c);
    assert_eq!(fs::read(dir.join("rj_p.snap")).unwrap(), approved_p);
    assert_eq!(fs::read(dir.join("rj_p.snap.png")).unwrap(), approved_png);

    // Review: accept canonical, reject PNG.
    accept_sim(&dir, "rj_c").unwrap();
    reject_sim(&dir, "rj_p");
    assert!(!dir.join("rj_p.snap.new").exists());

    // Mixed baseline: canonical gen-002, PNG gen-001.
    let err = check_consistent(&dir, "rj_c", "rj_p").unwrap_err();
    assert!(err.contains("mixed compound baseline"), "{err}");

    // Re-run (fresh names over copied approved state): canonical passes,
    // PNG fails, consistency still red.
    copy_approved(&dir, "rj_c", "rj_c2");
    copy_approved(&dir, "rj_p", "rj_p2");
    run_canonical(&dir, "rj_c2", &screen_gen2(), GEN2).expect("accepted canonical passes");
    assert!(run_png(&dir, "rj_p2", png_gen2(), GEN2, AlphaPolicy::StraightRgba).is_err());
    check_consistent(&dir, "rj_c2", "rj_p2").expect_err("mixed baseline must stay red");
}

#[test]
fn partial_accept_breaks_compound() {
    if !require_pending_mode() {
        return;
    }
    let (_tmp, dir) = fresh_dir("partial");
    write_text_snap(&dir, "pa_c", GEN1, &insta_string(&screen_gen1()));
    write_binary_snap(&dir, "pa_p", GEN1, &png_gen1());

    assert!(run_canonical(&dir, "pa_c", &screen_gen2(), GEN2).is_err());
    assert!(run_png(&dir, "pa_p", png_gen2(), GEN2, AlphaPolicy::StraightRgba).is_err());

    // Review accepts canonical only; PNG pending left in place.
    accept_sim(&dir, "pa_c").unwrap();
    assert!(
        dir.join("pa_p.snap.new").exists(),
        "png review still pending"
    );

    check_consistent(&dir, "pa_c", "pa_p").expect_err("partial accept must be red");

    copy_approved(&dir, "pa_c", "pa_c2");
    copy_approved(&dir, "pa_p", "pa_p2");
    run_canonical(&dir, "pa_c2", &screen_gen2(), GEN2).expect("accepted canonical passes");
    assert!(run_png(&dir, "pa_p2", png_gen2(), GEN2, AlphaPolicy::StraightRgba).is_err());
    check_consistent(&dir, "pa_c2", "pa_p2").expect_err("partial accept must stay red");
}

#[test]
fn interrupted_write_breaks_compound() {
    if !require_pending_mode() {
        return;
    }
    let (_tmp, dir) = fresh_dir("interrupted");
    write_text_snap(&dir, "iw_c", GEN1, &insta_string(&screen_gen1()));
    write_binary_snap(&dir, "iw_p", GEN1, &png_gen1());

    assert!(run_canonical(&dir, "iw_c", &screen_gen2(), GEN2).is_err());
    assert!(run_png(&dir, "iw_p", png_gen2(), GEN2, AlphaPolicy::StraightRgba).is_err());

    // Crash mid-review: PNG sidecar pending lost (torn write), metadata left.
    fs::remove_file(dir.join("iw_p.snap.new.png")).unwrap();
    assert!(dir.join("iw_p.snap.new").exists());
    // Accept of a torn binary pending is refused: no metadata-without-pixels.
    accept_sim(&dir, "iw_p").expect_err("torn pending must refuse accept");
    // Canonical accepted; crash "resolved" by deleting the torn PNG pending.
    accept_sim(&dir, "iw_c").unwrap();
    reject_sim(&dir, "iw_p");

    // Looks clean (no pendings) but mixed: consistency gate catches it.
    assert!(!dir.join("iw_c.snap.new").exists() && !dir.join("iw_p.snap.new").exists());
    check_consistent(&dir, "iw_c", "iw_p").expect_err("post-crash mix must be red");

    // Re-run: canonical passes, PNG fails and its pending regenerates —
    // nothing was silently lost or blessed.
    copy_approved(&dir, "iw_c", "iw_c2");
    copy_approved(&dir, "iw_p", "iw_p2");
    run_canonical(&dir, "iw_c2", &screen_gen2(), GEN2).expect("accepted canonical passes");
    assert!(run_png(&dir, "iw_p2", png_gen2(), GEN2, AlphaPolicy::StraightRgba).is_err());
    assert!(
        dir.join("iw_p2.snap.new").exists(),
        "pending must regenerate"
    );
    check_consistent(&dir, "iw_c2", "iw_p2").expect_err("post-crash mix must stay red");
}
