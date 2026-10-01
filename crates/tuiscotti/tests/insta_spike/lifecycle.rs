use super::fixtures::*;
use super::harness::*;
use super::*;
use std::fs;
use tuiscotti::diff::AlphaPolicy;
use tuiscotti::insta_proto::PngPixelComparator;
use tuiscotti::screen::{canonical_string, canonical_value};

fn assert_send_sync<T: Send + Sync>() {}

/// Rejection cases: anything that must NOT match the gen1 reference.
fn check_mismatch_cases(
    cmp: PngPixelComparator,
    dir: &std::path::Path,
    reference: &insta::Snapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    use insta::Comparator as _;
    // One pixel differs: no match.
    write_binary_snap(dir, "gen2", GEN2, &png_gen2()?)?;
    let gen2 = insta::Snapshot::from_file(&dir.join("gen2.snap"))?;
    assert!(!cmp.matches(reference, &gen2));

    // Corrupt bytes on either side never match.
    write_binary_snap(dir, "corrupt", GEN1, b"not a png")?;
    let corrupt = insta::Snapshot::from_file(&dir.join("corrupt.snap"))?;
    assert!(!cmp.matches(reference, &corrupt));
    assert!(!cmp.matches(&corrupt, reference));

    // Missing sidecar (Binary(None)) never matches, even against itself.
    write_binary_snap(dir, "noside", GEN1, &png_gen1()?)?;
    fs::remove_file(dir.join("noside.snap.png"))?;
    let noside = insta::Snapshot::from_file(&dir.join("noside.snap"))?;
    assert!(!cmp.matches(reference, &noside));
    assert!(!cmp.matches(&noside, &noside));

    // Text snapshots keep stock semantics via DefaultComparator.
    write_text_snap(dir, "t1", GEN1, "hello\n")?;
    write_text_snap(dir, "t2", GEN1, "hello\n")?;
    write_text_snap(dir, "t3", GEN1, "other\n")?;
    let t1 = insta::Snapshot::from_file(&dir.join("t1.snap"))?;
    let t2 = insta::Snapshot::from_file(&dir.join("t2.snap"))?;
    let t3 = insta::Snapshot::from_file(&dir.join("t3.snap"))?;
    assert!(cmp.matches(&t1, &t2));
    assert!(!cmp.matches(&t1, &t3));

    // Text/binary mix never matches.
    assert!(!cmp.matches(reference, &t1));
    assert!(!cmp.matches(&t1, reference));
    Ok(())
}

#[test]
fn projection_deterministic_and_complete() {
    let g1 = screen_gen1().expect("screen_gen1 succeeds");
    assert_eq!(
        canonical_string(&g1),
        canonical_string(&screen_gen1().expect("screen_gen1 succeeds"))
    );
    assert_eq!(
        canonical_value(&g1),
        canonical_value(&screen_gen1().expect("screen_gen1 succeeds"))
    );
    assert_ne!(
        canonical_string(&g1),
        canonical_string(&screen_gen2().expect("screen_gen2 succeeds"))
    );

    let text = canonical_string(&g1);
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

    let v = canonical_value(&g1);
    assert_eq!(v["cols"], serde_json::json!(4));
    assert_eq!(v["rows"], serde_json::json!(2));
    assert_eq!(v["ox"], serde_json::json!(5));
    assert_eq!(v["cursor"]["blinking"], serde_json::json!(true));
    assert_eq!(v["cells"].as_array().expect("cells is array").len(), 8);
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
    let tagged = png_insert_text(
        &png_gen1_no_tag_for_test().expect("png_gen1_no_tag_for_test succeeds"),
        PNG_GEN_KEYWORD,
        GEN1,
    )
    .expect("png_insert_text succeeds");
    let tagged = png_insert_text(&tagged, "k", "v").expect("png_insert_text succeeds");
    assert_eq!(png_find_text(&tagged, "k").as_deref(), Some("v"));
    assert_eq!(
        png_find_text(&tagged, PNG_GEN_KEYWORD).as_deref(),
        Some(GEN1)
    );
    // Comparator is Settings-compatible.
    assert_send_sync::<PngPixelComparator>();
    let c = PngPixelComparator::new(AlphaPolicy::Opaque);
    assert_eq!(c.alpha_policy(), AlphaPolicy::Opaque);
    let _clone: Box<dyn insta::Comparator> =
        <PngPixelComparator as insta::Comparator>::dyn_clone(&c);
}

#[test]
fn comparator_matches_decoded_pixels() {
    use insta::Comparator as _;
    let (_tmp, dir) = fresh_dir("comparator").expect("fresh_dir succeeds");
    let cmp = PngPixelComparator::new(AlphaPolicy::StraightRgba);

    // Reference: gen1 bytes.
    write_binary_snap(&dir, "ref", GEN1, &png_gen1().expect("png_gen1 succeeds"))
        .expect("write_binary_snap succeeds");
    let reference =
        insta::Snapshot::from_file(&dir.join("ref.snap")).expect("Snapshot::from_file succeeds");

    // Identical bytes match.
    write_binary_snap(&dir, "same", GEN1, &png_gen1().expect("png_gen1 succeeds"))
        .expect("write_binary_snap succeeds");
    let same =
        insta::Snapshot::from_file(&dir.join("same.snap")).expect("Snapshot::from_file succeeds");
    assert!(cmp.matches(&reference, &same));

    // Re-encoded identical pixels (different compressed bytes, no tEXt tag at
    // all) match: decoded equality, not byte equality.
    let reenc = encode_png(
        &rgba_image(&pixels_gen1().expect("pixels_gen1 succeeds"), 4, 4)
            .expect("rgba_image succeeds"),
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::NoFilter,
    )
    .expect("encode_png succeeds");
    assert_ne!(
        reenc,
        png_gen1().expect("png_gen1 succeeds"),
        "setup: encodings must differ"
    );
    write_binary_snap(&dir, "reenc", GEN1, &reenc).expect("write_binary_snap succeeds");
    let reenc_snap =
        insta::Snapshot::from_file(&dir.join("reenc.snap")).expect("Snapshot::from_file succeeds");
    assert!(cmp.matches(&reference, &reenc_snap));

    // Same pixels, different tEXt generation tag: pixels still match
    // (ancillary chunks are not pixels).
    let retagged = png_insert_text(
        &png_gen1_no_tag_for_test().expect("png_gen1_no_tag_for_test succeeds"),
        PNG_GEN_KEYWORD,
        "other",
    )
    .expect("png_insert_text succeeds");
    write_binary_snap(&dir, "retagged", "other", &retagged).expect("write_binary_snap succeeds");
    let retagged_snap = insta::Snapshot::from_file(&dir.join("retagged.snap"))
        .expect("Snapshot::from_file succeeds");
    assert!(cmp.matches(&reference, &retagged_snap));

    check_mismatch_cases(cmp, &dir, &reference).expect("mismatch cases hold");

    // Policy is explicit: semi-transparent identical pixels match under
    // StraightRgba but never under Opaque.
    let semi_img = rgba_image(&pixels_semi().expect("pixels_semi succeeds"), 4, 4)
        .expect("rgba_image succeeds");
    let semi_a = png_insert_text(
        &encode_png(
            &semi_img,
            image::codecs::png::CompressionType::Default,
            image::codecs::png::FilterType::Adaptive,
        )
        .expect("encode_png succeeds"),
        PNG_GEN_KEYWORD,
        GEN1,
    )
    .expect("png_insert_text succeeds");
    let semi_b = png_insert_text(
        &encode_png(
            &semi_img,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::NoFilter,
        )
        .expect("encode_png succeeds"),
        PNG_GEN_KEYWORD,
        GEN1,
    )
    .expect("png_insert_text succeeds");
    assert_ne!(semi_a, semi_b, "setup: encodings must differ");
    write_binary_snap(&dir, "semi_a", GEN1, &semi_a).expect("write_binary_snap succeeds");
    write_binary_snap(&dir, "semi_b", GEN1, &semi_b).expect("write_binary_snap succeeds");
    let semi_snap_a =
        insta::Snapshot::from_file(&dir.join("semi_a.snap")).expect("Snapshot::from_file succeeds");
    let semi_snap_b =
        insta::Snapshot::from_file(&dir.join("semi_b.snap")).expect("Snapshot::from_file succeeds");
    assert!(cmp.matches(&semi_snap_a, &semi_snap_b));
    let opaque_cmp = PngPixelComparator::new(AlphaPolicy::Opaque);
    assert!(!opaque_cmp.matches(&semi_snap_a, &semi_snap_b));
}
