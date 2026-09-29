//! Evidence exports + graphics inspection (backlog A06, A07).

use tuiscotti::export::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn png_solid(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
    use image::ImageEncoder as _;
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba(px));
    let mut buf = Vec::new();
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .unwrap();
    buf
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn kitty(params: &str, payload_b64: &str) -> Vec<u8> {
    format!("\x1b_G{params};{payload_b64}\x1b\\").into_bytes()
}

fn sixel(params: &str, data: &str) -> Vec<u8> {
    format!("\x1bP{params}q{data}\x1b\\").into_bytes()
}

// ---------------------------------------------------------------------------
// Cast (A06)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Sixel inspection + decode (A07)
// ---------------------------------------------------------------------------

#[test]
fn sixel_parse_and_exact_decode() {
    let mut stream = b"hello ".to_vec();
    stream.extend(sixel("0;0;7", "\"1;1;2;6#0;2;100;0;0~~"));
    stream.extend(b" world");
    let scan = scan_graphics_default(&stream);
    assert!(scan.is_clean(), "diagnostics: {:?}", scan.diagnostics);
    assert_eq!(scan.payloads.len(), 1);
    let p = &scan.payloads[0];
    assert_eq!(p.kind, GraphicsKind::Sixel);
    assert_eq!(p.param("Ph"), Some("2"));
    assert_eq!(p.param("Pv"), Some("6"));
    assert_eq!(p.placement.image_px, Some((2, 6)));
    assert_eq!(p.placement.z, None);
    assert_eq!(p.placement.col, None);
    let img = p.decode_bounded(&GraphicsPolicy::default()).unwrap();
    assert_eq!((img.width, img.height), (2, 6));
    let (pixels, _rest) = img.rgba.as_chunks::<4>();
    assert!(pixels.iter().all(|px| *px == [255, 0, 0, 255]));
}

#[test]
fn sixel_hls_repeat_and_newline() {
    // HLS red (h=0,l=50,s=100), two columns, newline, two more columns.
    let stream = sixel("0;1;7", "\"1;1;2;12#0;1;0;50;100!2~-!2~");
    let scan = scan_graphics_default(&stream);
    assert!(scan.is_clean(), "diagnostics: {:?}", scan.diagnostics);
    let img = scan.payloads[0]
        .decode_bounded(&GraphicsPolicy::default())
        .unwrap();
    assert_eq!((img.width, img.height), (2, 12));
    let px = |x: u32, y: u32| {
        let o = (y as usize * 2 + x as usize) * 4;
        [
            img.rgba[o],
            img.rgba[o + 1],
            img.rgba[o + 2],
            img.rgba[o + 3],
        ]
    };
    assert_eq!(px(0, 0), [255, 0, 0, 255]);
    assert_eq!(px(1, 5), [255, 0, 0, 255]);
    assert_eq!(px(0, 6), [255, 0, 0, 255]);
    assert_eq!(px(1, 11), [255, 0, 0, 255]);
}

#[test]
fn sixel_strict_decode_errors() {
    let policy = GraphicsPolicy::default();
    // Plot with no color defined.
    let scan = scan_graphics_default(&sixel("", "~"));
    assert_eq!(
        scan.payloads[0].decode_bounded(&policy),
        Err(GraphicsDecodeError::UndefinedColor(0))
    );
    // Unexpected byte (space is not sixel).
    let scan = scan_graphics_default(&sixel("", "#0;2;100;100;100  ~"));
    assert!(matches!(
        scan.payloads[0].decode_bounded(&policy),
        Err(GraphicsDecodeError::InvalidData(_))
    ));
    // Empty image.
    let scan = scan_graphics_default(&sixel("", "#0;2;0;0;0"));
    assert!(matches!(
        scan.payloads[0].decode_bounded(&policy),
        Err(GraphicsDecodeError::InvalidData(_))
    ));
}

// ---------------------------------------------------------------------------
// Kitty inspection + decode (A07)
// ---------------------------------------------------------------------------

#[test]
fn kitty_single_decode_and_placement() {
    let raw: Vec<u8> = vec![255, 0, 0, 255, 0, 255, 0, 255]; // 2x1 RGBA
    let stream = kitty("a=T,f=32,s=2,v=1,z=-1,X=3,Y=4,c=5,r=6", &b64(&raw));
    let scan = scan_graphics_default(&stream);
    assert!(scan.is_clean(), "diagnostics: {:?}", scan.diagnostics);
    assert_eq!(scan.payloads.len(), 1);
    let p = &scan.payloads[0];
    assert_eq!(p.kind, GraphicsKind::Kitty);
    assert_eq!(p.action(), Some("T"));
    assert_eq!(p.data, raw);
    assert_eq!(p.placement.z, Some(-1));
    assert_eq!((p.placement.dx_px, p.placement.dy_px), (3, 4));
    assert_eq!(p.placement.image_px, Some((2, 1)));
    assert_eq!(p.placement.display_cells, Some((5, 6)));
    let img = p.decode_bounded(&GraphicsPolicy::default()).unwrap();
    assert_eq!((img.width, img.height), (2, 1));
    assert_eq!(img.rgba, raw);
}

#[test]
fn kitty_chunked_reassembly_equals_single() {
    let raw: Vec<u8> = (0..48).collect(); // 4x3 RGBA
    let full = b64(&raw);
    let (part1, part2) = full.split_at(32); // multiple of 4
    let mut chunked = kitty("a=T,f=32,s=4,v=3,m=1", part1);
    chunked.extend(kitty("m=0", part2));
    let single = kitty("a=T,f=32,s=4,v=3", &full);
    let a = scan_graphics_default(&chunked);
    let b = scan_graphics_default(&single);
    assert!(a.is_clean(), "diagnostics: {:?}", a.diagnostics);
    assert!(b.is_clean(), "diagnostics: {:?}", b.diagnostics);
    assert!(a.payloads[0].placement_equality(&b.payloads[0]));
    assert_eq!(
        a.payloads[0]
            .decode_bounded(&GraphicsPolicy::default())
            .unwrap(),
        b.payloads[0]
            .decode_bounded(&GraphicsPolicy::default())
            .unwrap()
    );
}

#[test]
fn kitty_png_and_rgb24_roundtrip() {
    // f=100 PNG.
    let png = png_solid(3, 2, [10, 20, 30, 40]);
    let scan = scan_graphics_default(&kitty("a=T,f=100", &b64(&png)));
    assert!(scan.is_clean(), "diagnostics: {:?}", scan.diagnostics);
    let img = scan.payloads[0]
        .decode_bounded(&GraphicsPolicy::default())
        .unwrap();
    assert_eq!((img.width, img.height), (3, 2));
    let (pixels, _rest) = img.rgba.as_chunks::<4>();
    assert!(pixels.iter().all(|p| *p == [10, 20, 30, 40]));
    // f=24 RGB gains opaque alpha.
    let scan = scan_graphics_default(&kitty("a=T,f=24,s=1,v=1", &b64(&[7, 8, 9])));
    let img = scan.payloads[0]
        .decode_bounded(&GraphicsPolicy::default())
        .unwrap();
    assert_eq!(img.rgba, vec![7, 8, 9, 255]);
    // f=24 without dims.
    let scan = scan_graphics_default(&kitty("a=T,f=24", &b64(&[7, 8, 9])));
    assert_eq!(
        scan.payloads[0].decode_bounded(&GraphicsPolicy::default()),
        Err(GraphicsDecodeError::MissingDims)
    );
    // Non-direct medium refused.
    let scan = scan_graphics_default(&kitty("a=T,f=100,t=f", &b64(b"/tmp/x")));
    assert!(matches!(
        scan.payloads[0].decode_bounded(&GraphicsPolicy::default()),
        Err(GraphicsDecodeError::UnsupportedMedium(_))
    ));
}

#[test]
fn kitty_transmit_then_display_by_id() {
    let raw: Vec<u8> = vec![1, 2, 3, 4];
    let mut stream = kitty("a=t,f=32,s=1,v=1,i=7", &b64(&raw));
    stream.extend(kitty("a=p,i=7", ""));
    let scan = scan_graphics_default(&stream);
    assert!(scan.is_clean(), "diagnostics: {:?}", scan.diagnostics);
    assert_eq!(scan.payloads.len(), 2);
    assert_eq!(scan.payloads[1].references, Some(7));
    assert_eq!(scan.payloads[1].data, raw);
    // Dangling reference: loud diagnostic + decode error.
    let scan = scan_graphics_default(&kitty("a=p,i=99", ""));
    assert_eq!(scan.payloads.len(), 1);
    assert!(scan
        .diagnostics
        .iter()
        .any(|d| d.kind == GraphicsDiagKind::UnknownReference));
    assert_eq!(
        scan.payloads[0].decode_bounded(&GraphicsPolicy::default()),
        Err(GraphicsDecodeError::UnknownReference(99))
    );
}

#[test]
fn placement_equality_semantics() {
    let raw: Vec<u8> = vec![5, 6, 7, 8];
    let at = |prefix: &[u8], params: &str| {
        let mut s = prefix.to_vec();
        s.extend(kitty(params, &b64(&raw)));
        scan_graphics_default(&s).payloads.pop().unwrap()
    };
    let a = at(b"", "a=T,f=32,s=1,v=1,X=3");
    let b = at(b"different length prefix text", "a=T,f=32,s=1,v=1,X=3");
    assert_ne!(a.stream_offset, b.stream_offset);
    assert!(a.placement_equality(&b), "offsets must not affect equality");
    let moved = at(b"", "a=T,f=32,s=1,v=1,X=4");
    assert!(!a.placement_equality(&moved), "moved placement must differ");
    let other_bytes = at(b"", "a=T,f=32,s=1,v=1,X=3");
    let mut changed = other_bytes.clone();
    changed.data = vec![9, 9, 9, 9];
    assert!(!other_bytes.placement_equality(&changed));
}

// ---------------------------------------------------------------------------
// Bounds, truncation, unsupported, malformed (A07)
// ---------------------------------------------------------------------------

#[test]
fn graphics_truncation_is_explicit_and_never_decodes() {
    let policy = GraphicsPolicy {
        max_payload_bytes: 4,
        ..GraphicsPolicy::default()
    };
    let raw: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let scan = scan_graphics(&kitty("a=T,f=32,s=2,v=1", &b64(&raw)), &policy);
    assert_eq!(scan.payloads.len(), 1);
    assert!(scan.payloads[0].truncated);
    assert!(scan
        .diagnostics
        .iter()
        .any(|d| d.kind == GraphicsDiagKind::Truncated));
    assert_eq!(
        scan.payloads[0].decode_bounded(&policy),
        Err(GraphicsDecodeError::Truncated)
    );
    // Oversize dims rejected before allocation.
    let scan = scan_graphics_default(&kitty("a=T,f=32,s=5000,v=5000", &b64(&[0; 8])));
    assert!(matches!(
        scan.payloads[0].decode_bounded(&GraphicsPolicy::default()),
        Err(GraphicsDecodeError::TooLarge { .. })
    ));
}

#[test]
fn graphics_unsupported_sequences_diagnosed_never_silent() {
    let mut stream = Vec::new();
    stream.extend(b"\x1bP1$r0;0q\x1b\\"); // DCS with final 'r' (DECRQM)
    stream.extend(b"\x1b_Xfoo;YmFy\x1b\\"); // APC, not kitty 'G'
    stream.extend(b"\x1b]1337;File=inline=1:AAAA\x07"); // iTerm2
    stream.extend(b"\x1bPqabc"); // unterminated DCS
    let scan = scan_graphics_default(&stream);
    assert!(scan.payloads.is_empty());
    let kinds: Vec<_> = scan.diagnostics.iter().map(|d| d.kind).collect();
    assert_eq!(
        kinds,
        vec![
            GraphicsDiagKind::Unsupported,
            GraphicsDiagKind::Unsupported,
            GraphicsDiagKind::Unsupported,
            GraphicsDiagKind::Malformed,
        ]
    );
    // Non-graphics OSC (title) stays out of scope: no payload, no diagnostic.
    let scan = scan_graphics_default(b"\x1b]0;title\x07plain");
    assert!(scan.payloads.is_empty());
    assert!(scan.is_clean());
}

#[test]
fn graphics_malformed_kitty_is_loud_but_lenient() {
    // Invalid base64: payload still produced (empty), diagnostic recorded.
    let scan = scan_graphics_default(&kitty("a=T,f=32,s=1,v=1", "***"));
    assert_eq!(scan.payloads.len(), 1);
    assert!(scan.payloads[0].data.is_empty());
    assert!(scan
        .diagnostics
        .iter()
        .any(|d| d.kind == GraphicsDiagKind::Malformed));
    // Key without '='.
    let scan = scan_graphics_default(&kitty("a=T,zzz,f=32,s=1,v=1", &b64(&[1, 2, 3, 4])));
    assert_eq!(scan.payloads.len(), 1);
    assert!(scan
        .diagnostics
        .iter()
        .any(|d| d.kind == GraphicsDiagKind::Malformed));
    // Command without ';'.
    let scan = scan_graphics_default(b"\x1b_Ga=T,f=32\x1b\\");
    assert!(scan.payloads.is_empty());
    assert!(scan
        .diagnostics
        .iter()
        .any(|d| d.kind == GraphicsDiagKind::Malformed));
    // Abandoned m=1 chain.
    let scan = scan_graphics_default(&kitty("a=T,f=32,s=1,v=1,m=1", &b64(&[1, 2])));
    assert!(scan.payloads.is_empty());
    assert!(scan
        .diagnostics
        .iter()
        .any(|d| d.kind == GraphicsDiagKind::Malformed));
}

#[test]
fn graphics_payload_count_bound() {
    let policy = GraphicsPolicy {
        max_payloads: 1,
        ..GraphicsPolicy::default()
    };
    let mut stream = sixel("", "\"1;1;1;6#0;2;100;0;0~");
    stream.extend(sixel("", "\"1;1;1;6#0;2;0;100;0~"));
    let scan = scan_graphics(&stream, &policy);
    assert_eq!(scan.payloads.len(), 1);
    assert!(scan
        .diagnostics
        .iter()
        .any(|d| d.kind == GraphicsDiagKind::Truncated));
}

// ---------------------------------------------------------------------------
// Policy pins
// ---------------------------------------------------------------------------

#[test]
fn export_policies_pin_defaults() {
    let p = ExportPolicies::default();
    assert_eq!(p.cast.timestamp, 0);
    assert_eq!(p.gif.speed, 1);
    assert!(p.gif.repeat_infinite);
    assert_eq!((p.apng.plays, p.apng.delay_den), (0, 1000));
    assert_eq!(p.mp4.crf, 23);
    assert_eq!(p.mp4.preset, "medium");
    assert_eq!(p.mp4.pix_fmt, "yuv420p");
    assert_eq!(p.graphics.max_payload_bytes, 1 << 20);
    assert_eq!(p.graphics.max_dim, 4096);
}
