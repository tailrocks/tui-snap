use super::*;
use tuiscotti::export::*;

#[test]
fn cast_deterministic_and_pinned() {
    let frames = vec![
        ("hello \"quoted\"\nline2\t✓".to_string(), 0.0),
        ("second frame \x1b[31mred\x1b[0m".to_string(), 0.5),
        ("third".to_string(), 1.25),
    ];
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let pa = cast_v2(&frames, 80, 24, a.path()).unwrap();
    let pb = cast_v2(&frames, 80, 24, b.path()).unwrap();
    assert_eq!(pa.file_name().unwrap(), "session.cast");
    let ba = std::fs::read(&pa).unwrap();
    let bb = std::fs::read(&pb).unwrap();
    assert_eq!(ba, bb, "same input must give byte-identical cast");
    let text = String::from_utf8(ba).unwrap();
    let mut lines = text.lines();
    assert_eq!(
        lines.next().unwrap(),
        r#"{"version":2,"width":80,"height":24,"timestamp":0,"title":"tuisnap","env":{"TERM":"tuisnap"}}"#
    );
    assert_eq!(
        lines.next().unwrap(),
        "[0.000000,\"o\",\"hello \\\"quoted\\\"\\nline2\\t✓\"]"
    );
    assert_eq!(
        lines.next().unwrap(),
        "[0.500000,\"o\",\"second frame \\u001b[31mred\\u001b[0m\"]"
    );
    assert_eq!(lines.next().unwrap(), "[1.750000,\"o\",\"third\"]");
    assert_eq!(lines.next(), None);
}

#[test]
fn cast_rejects_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let ok = vec![("x".to_string(), 0.0)];
    assert!(cast_v2(&ok, 0, 24, dir.path()).is_err());
    assert!(cast_v2(&ok, 80, 0, dir.path()).is_err());
    assert!(cast_v2(&[("x".to_string(), -1.0)], 80, 24, dir.path()).is_err());
    assert!(cast_v2(&[("x".to_string(), f64::NAN)], 80, 24, dir.path()).is_err());
}

#[test]
fn cast_empty_frames_is_header_only() {
    let dir = tempfile::tempdir().unwrap();
    let p = cast_v2(&[], 80, 24, dir.path()).unwrap();
    let text = std::fs::read_to_string(p).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.starts_with(r#"{"version":2,"#));
}

// ---------------------------------------------------------------------------
// GIF (A06)
// ---------------------------------------------------------------------------
#[test]
fn gif_deterministic_and_decodable() {
    let frames = vec![
        png_solid(8, 8, [255, 0, 0, 255]),
        png_solid(8, 8, [0, 0, 255, 255]),
    ];
    let delays = vec![100, 200];
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.gif");
    let b = dir.path().join("b.gif");
    gif(&frames, &delays, &a).unwrap();
    gif(&frames, &delays, &b).unwrap();
    let ba = std::fs::read(&a).unwrap();
    let bb = std::fs::read(&b).unwrap();
    assert_eq!(ba, bb, "same PNGs must give byte-identical GIF");
    assert!(ba.starts_with(b"GIF89a"));
    // Qualify: the bytes decode to the two frames we sent.
    use image::AnimationDecoder as _;
    let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&ba)).unwrap();
    let got: Vec<_> = dec.into_frames().collect::<Result<_, _>>().unwrap();
    assert_eq!(got.len(), 2);
}

#[test]
fn gif_rejects_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let good = png_solid(4, 4, [1, 2, 3, 255]);
    let other = png_solid(5, 4, [1, 2, 3, 255]);
    let out = dir.path().join("x.gif");
    assert!(gif(&[], &[], &out).is_err());
    assert!(gif(std::slice::from_ref(&good), &[10, 10], &out).is_err());
    assert!(gif(&[good.clone(), other], &[10, 10], &out).is_err());
    assert!(gif(&[b"not a png".to_vec()], &[10], &out).is_err());
    let bad_speed = GifPolicy {
        speed: 99,
        ..Default::default()
    };
    assert!(gif_with(&[good], &[10], &out, &bad_speed).is_err());
}

// ---------------------------------------------------------------------------
// APNG (A06)
// ---------------------------------------------------------------------------
#[test]
fn apng_deterministic_and_roundtrips_pixels() {
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let frames = vec![png_solid(6, 4, red), png_solid(6, 4, blue)];
    let delays = vec![50, 150];
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.png");
    let b = dir.path().join("b.png");
    apng(&frames, &delays, &a).unwrap();
    apng(&frames, &delays, &b).unwrap();
    let ba = std::fs::read(&a).unwrap();
    let bb = std::fs::read(&b).unwrap();
    assert_eq!(ba, bb, "same PNGs must give byte-identical APNG");
    assert!(ba.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]));
    for marker in [b"acTL".as_slice(), b"fcTL", b"fdAT", b"IDAT", b"IEND"] {
        assert!(
            ba.windows(marker.len()).any(|w| w == marker),
            "APNG must contain {marker:?}"
        );
    }
    // Qualify: image's own APNG decoder reads both frames back losslessly.
    use image::AnimationDecoder as _;
    let dec = image::codecs::png::PngDecoder::new(std::io::Cursor::new(&ba)).unwrap();
    assert!(dec.is_apng().unwrap());
    let got: Vec<_> = dec
        .apng()
        .unwrap()
        .into_frames()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(got.len(), 2);
    assert!(got[0].buffer().pixels().all(|p| p.0 == red));
    assert!(got[1].buffer().pixels().all(|p| p.0 == blue));
}

#[test]
fn apng_rejects_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let good = png_solid(4, 4, [1, 2, 3, 255]);
    let other = png_solid(4, 5, [1, 2, 3, 255]);
    let out = dir.path().join("x.png");
    assert!(apng(&[], &[], &out).is_err());
    assert!(apng(std::slice::from_ref(&good), &[10, 10], &out).is_err());
    assert!(apng(&[good.clone(), other], &[10, 10], &out).is_err());
    let bad_den = ApngPolicy {
        delay_den: 0,
        ..Default::default()
    };
    assert!(apng_with(&[good], &[10], &out, &bad_den).is_err());
}

// ---------------------------------------------------------------------------
// MP4 (A06): missing-encoder path always; real encode iff ffmpeg exists
// ---------------------------------------------------------------------------
#[test]
fn mp4_policy_validated_without_encoder() {
    let dir = tempfile::tempdir().unwrap();
    let frames = vec![png_solid(4, 4, [9, 9, 9, 255])];
    let bad = Mp4Policy {
        crf: 99,
        ..Default::default()
    };
    let err = mp4_with(&frames, &[100], &dir.path().join("x.mp4"), &bad).unwrap_err();
    assert!(!err.is_encoder_missing());
    let mut bad = Mp4Policy::default();
    bad.preset.clear();
    assert!(mp4_with(&frames, &[100], &dir.path().join("x.mp4"), &bad).is_err());
}

#[test]
fn mp4_missing_encoder_or_real_encode() {
    match ffmpeg_version() {
        Err(e) if e.is_encoder_missing() => {
            let msg = e.to_string();
            assert!(msg.contains("ffmpeg"), "must name the tool: {msg}");
            assert!(msg.contains("ffmpeg.org"), "must name the install: {msg}");
            // And mp4 surfaces the same explicit error.
            let dir = tempfile::tempdir().unwrap();
            let frames = vec![png_solid(4, 4, [9, 9, 9, 255])];
            let err = mp4(&frames, &[100], &dir.path().join("x.mp4")).unwrap_err();
            assert!(err.is_encoder_missing());
        }
        Err(e) => panic!("unexpected probe failure: {e}"),
        Ok(version) => {
            assert!(!version.is_empty());
            let dir = tempfile::tempdir().unwrap();
            let frames = vec![
                png_solid(16, 16, [255, 0, 0, 255]),
                png_solid(16, 16, [0, 255, 0, 255]),
            ];
            let out = dir.path().join("x.mp4");
            let sidecar = mp4(&frames, &[100, 100], &out).unwrap();
            assert_eq!(sidecar.ffmpeg_version, version);
            let video = std::fs::read(&sidecar.mp4).unwrap();
            assert!(!video.is_empty());
            let record = std::fs::read_to_string(&sidecar.sidecar).unwrap();
            assert!(record.contains("ffmpeg"));
            assert!(record.contains("\"deterministic\": false"));
            assert!(
                !dir.path().join("x.mp4frames").exists(),
                "staging removed on success"
            );
        }
    }
}
