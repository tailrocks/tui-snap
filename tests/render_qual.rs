//! Rendering qualification (backlog V01, V03–V10; V02 frozen).
//!
//! Independent of capture paths: every grid here is HAND-AUTHORED (no
//! Ratatui, no PTY), so expectations cannot share the renderer's
//! assumptions. Source widths in the test data control layout; fallback
//! faces must never shift the grid (V04).

use tuisnap::frame::{Frame, FRAME_VERSION};
use tuisnap::profile::{
    font_sha256, BlinkPhase, CursorPolicy, MissingGlyphPolicy, PalettePolicy, RenderProfile,
    RENDERER_VERSION, VENDORED_FACES, VENDORED_FALLBACK_FACES, VENDORED_FONT_BOLD,
    VENDORED_FONT_BOLD_ITALIC, VENDORED_FONT_BOLD_ITALIC_SHA256, VENDORED_FONT_BOLD_SHA256,
    VENDORED_FONT_ITALIC, VENDORED_FONT_ITALIC_SHA256, VENDORED_FONT_SHA256,
};
use tuisnap::render::{
    check_contract_bytes, escape_html, escape_html_attr, escape_json_for_script, frame_from_screen,
    redact_frame, redact_screen, render_cache_disabled, render_frame_strict, render_screen,
    render_screen_png, screen_content_hash, BundleManifest, RenderCache, Renderer,
};
use tuisnap::{Cell, Color, Cursor, Mods, Provenance, Screen, VENDORED_FONT};

// ---------------------------------------------------------------------------
// Hand-authored grid helpers (independent of capture/model adapters).
// ---------------------------------------------------------------------------

fn prov() -> Provenance {
    Provenance {
        tool: "tuisnap".into(),
        tool_version: "test".into(),
        profile: "tuisnap-default".into(),
        source: "test".into(),
        argv: vec![],
        created_unix: 0,
    }
}

fn cell(x: u16, y: u16, symbol: &str, width: u8) -> Cell {
    Cell {
        x,
        y,
        symbol: symbol.to_string(),
        width,
        continuation: false,
        fg: Color::Default,
        bg: Color::Default,
        mods: Mods::default(),
        underline_color: Color::Default,
    }
}

fn cont(x: u16, y: u16) -> Cell {
    Cell {
        x,
        y,
        symbol: String::new(),
        width: 0,
        continuation: true,
        fg: Color::Default,
        bg: Color::Default,
        mods: Mods::default(),
        underline_color: Color::Default,
    }
}

/// Blank frame with `leads` overlaid by (x, y). Caller supplies wide-cell
/// continuations explicitly — nothing is inferred.
fn frame_from_leads(cols: u16, rows: u16, leads: Vec<Cell>) -> Frame {
    let mut f = Frame::blank(cols, rows, prov());
    for c in leads {
        f.set(c);
    }
    f.validate().unwrap();
    f
}

fn screen_from_leads(cols: u16, rows: u16, leads: Vec<Cell>) -> Screen {
    let mut cells = Vec::with_capacity(cols as usize * rows as usize);
    for y in 0..rows {
        for x in 0..cols {
            cells.push(Cell::blank(x, y));
        }
    }
    for c in leads {
        let i = c.y as usize * cols as usize + c.x as usize;
        cells[i] = c;
    }
    Screen::validate(cols, rows, 0, 0, cells, Cursor::default()).unwrap()
}

fn strict_placeholder(fallbacks: Vec<tuisnap::FallbackFace<'_>>) -> RenderProfile<'_> {
    RenderProfile::strict(
        "qual".to_string(),
        VENDORED_FACES,
        [
            VENDORED_FONT_SHA256,
            VENDORED_FONT_BOLD_SHA256,
            VENDORED_FONT_ITALIC_SHA256,
            VENDORED_FONT_BOLD_ITALIC_SHA256,
        ],
        fallbacks,
        16.0,
        10,
        21,
        12,
        2,
        PalettePolicy::xterm(),
        CursorPolicy::Show,
        BlinkPhase::On,
        MissingGlyphPolicy::Placeholder,
        RENDERER_VERSION,
    )
    .unwrap()
}

fn decode(png: &[u8]) -> image::RgbImage {
    image::load_from_memory(png).unwrap().to_rgb8()
}

// ---------------------------------------------------------------------------
// V01: one engine for screens and frames.
// ---------------------------------------------------------------------------

#[test]
fn screen_and_frame_share_one_engine() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let mods = Mods {
        bold: true,
        underline: true,
        ..Mods::default()
    };
    let mut lead = cell(0, 0, "東", 2);
    lead.mods = mods;
    lead.fg = Color::Indexed(2);
    let frame = frame_from_leads(6, 3, vec![lead, cont(1, 0), cell(2, 0, "★", 1)]);
    let screen = Screen::from_frame(&frame).unwrap();
    let via_screen = render_screen(&screen, &rp).unwrap();
    let via_frame = render_frame_strict(&frame, &rp).unwrap();
    assert_eq!(via_screen.png, via_frame.png);
    assert_eq!(via_screen.fidelity.to_json(), via_frame.fidelity.to_json());
    assert_eq!(render_screen_png(&screen, &rp).unwrap(), via_screen.png);
}

#[test]
fn screen_adaptation_is_lossless_and_origin_free() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let mut lead = cell(1, 1, "京", 2);
    lead.mods.italic = true;
    lead.bg = Color::Indexed(4);
    let screen = screen_from_leads(6, 3, vec![lead.clone(), cont(2, 1)]);
    let frame = frame_from_screen(&screen, "qual");
    assert_eq!(frame.version, FRAME_VERSION);
    assert_eq!((frame.cols, frame.rows), (6, 3));
    assert_eq!(frame.cells, screen.cells().to_vec());
    assert_eq!(frame.cursor, *screen.cursor());
    frame.validate().unwrap();
    // Same grid at a nonzero origin renders identical pixels: origin is
    // positional metadata, not render input.
    let moved = Screen::validate(6, 3, 5, 7, screen.cells().to_vec(), *screen.cursor()).unwrap();
    assert_eq!(
        render_screen(&screen, &rp).unwrap().png,
        render_screen(&moved, &rp).unwrap().png
    );
    assert_eq!(lead.symbol, "京");
}

// ---------------------------------------------------------------------------
// Strict profile: pins, hashes, face-swap detection.
// ---------------------------------------------------------------------------

#[test]
fn vendored_pins_match_vendored_bytes() {
    assert_eq!(font_sha256(VENDORED_FONT), VENDORED_FONT_SHA256);
    assert_eq!(font_sha256(VENDORED_FONT_BOLD), VENDORED_FONT_BOLD_SHA256);
    assert_eq!(
        font_sha256(VENDORED_FONT_ITALIC),
        VENDORED_FONT_ITALIC_SHA256
    );
    assert_eq!(
        font_sha256(VENDORED_FONT_BOLD_ITALIC),
        VENDORED_FONT_BOLD_ITALIC_SHA256
    );
    let rp = RenderProfile::vendored();
    assert_eq!(
        rp.face_hashes(),
        &[
            VENDORED_FONT_SHA256.to_string(),
            VENDORED_FONT_BOLD_SHA256.to_string(),
            VENDORED_FONT_ITALIC_SHA256.to_string(),
            VENDORED_FONT_BOLD_ITALIC_SHA256.to_string(),
        ]
    );
    assert_eq!(rp.fallback_order().len(), VENDORED_FALLBACK_FACES.len());
    assert_eq!(rp.renderer_version(), RENDERER_VERSION);
    assert_eq!(rp.missing(), MissingGlyphPolicy::Strict);
    // Profile hash is stable and covers every pin.
    assert_eq!(rp.hash(), rp.hash());
    assert_eq!(rp.hash().len(), 64);
}

#[test]
fn strict_constructor_validates_every_pin() {
    let rp = RenderProfile::vendored();
    assert_eq!(rp.hash(), RenderProfile::vendored().hash());
    // Reordered fallback chain = different identity.
    let mut rev = VENDORED_FALLBACK_FACES.to_vec();
    rev.reverse();
    let reordered = strict_placeholder(rev).hash();
    assert_ne!(
        reordered,
        strict_placeholder(VENDORED_FALLBACK_FACES.to_vec()).hash()
    );
    // Other phase / policy / cursor = different identity (no shared keys).
    assert_ne!(
        rp.with_phase(BlinkPhase::Off).hash(),
        rp.with_phase(BlinkPhase::On).hash()
    );
    assert_ne!(
        rp.with_missing(MissingGlyphPolicy::Placeholder).hash(),
        rp.hash()
    );
    assert_ne!(rp.with_cursor(CursorPolicy::Hide).hash(), rp.hash());
}

#[test]
fn strict_constructor_rejects_bad_pins_and_geometry() {
    let good = [
        VENDORED_FONT_SHA256,
        VENDORED_FONT_BOLD_SHA256,
        VENDORED_FONT_ITALIC_SHA256,
        VENDORED_FONT_BOLD_ITALIC_SHA256,
    ];
    let build = |hashes: [&str; 4]| {
        RenderProfile::strict(
            "x".to_string(),
            VENDORED_FACES,
            hashes,
            VENDORED_FALLBACK_FACES.to_vec(),
            16.0,
            10,
            21,
            12,
            2,
            PalettePolicy::xterm(),
            CursorPolicy::Show,
            BlinkPhase::On,
            MissingGlyphPolicy::Strict,
            RENDERER_VERSION,
        )
    };
    // Swapped pin: no substitution, an error naming the face.
    let mut bad = good;
    bad[1] = VENDORED_FONT_SHA256;
    let err = build(bad).unwrap_err();
    assert!(err.to_string().contains("bold"), "{err}");
    // Wrong renderer version.
    let err = RenderProfile::strict(
        "x".to_string(),
        VENDORED_FACES,
        good,
        VENDORED_FALLBACK_FACES.to_vec(),
        16.0,
        10,
        21,
        12,
        2,
        PalettePolicy::xterm(),
        CursorPolicy::Show,
        BlinkPhase::On,
        MissingGlyphPolicy::Strict,
        RENDERER_VERSION + 1,
    )
    .unwrap_err();
    assert!(err.to_string().contains("renderer version"), "{err}");
    // Degenerate geometry.
    let err = RenderProfile::strict(
        "x".to_string(),
        VENDORED_FACES,
        good,
        vec![],
        16.0,
        0,
        21,
        12,
        2,
        PalettePolicy::xterm(),
        CursorPolicy::Show,
        BlinkPhase::On,
        MissingGlyphPolicy::Strict,
        RENDERER_VERSION,
    )
    .unwrap_err();
    assert!(err.to_string().contains("geometry"), "{err}");
}

#[test]
fn face_swap_detected_at_construction_and_at_render() {
    // Mutate one byte of a COPY of the regular face: strict construction
    // must fail (hash verified, never substituted).
    let mut swapped = VENDORED_FONT.to_vec();
    let mid = swapped.len() / 2;
    swapped[mid] = swapped[mid].wrapping_add(1);
    let faces = tuisnap::FontFaces {
        regular: &swapped,
        bold: VENDORED_FONT_BOLD,
        italic: VENDORED_FONT_ITALIC,
        bold_italic: VENDORED_FONT_BOLD_ITALIC,
    };
    let err = RenderProfile::strict(
        "x".to_string(),
        faces,
        [
            VENDORED_FONT_SHA256,
            VENDORED_FONT_BOLD_SHA256,
            VENDORED_FONT_ITALIC_SHA256,
            VENDORED_FONT_BOLD_ITALIC_SHA256,
        ],
        vec![],
        16.0,
        10,
        21,
        12,
        2,
        PalettePolicy::xterm(),
        CursorPolicy::Show,
        BlinkPhase::On,
        MissingGlyphPolicy::Placeholder,
        RENDERER_VERSION,
    )
    .unwrap_err();
    assert!(err.to_string().contains("regular"), "{err}");

    // Swap AFTER construction (borrowed bytes mutated): render refuses.
    // UnsafeCell models hostile/shared-mutability under the borrow; the
    // renderer must re-verify, not trust construction-time pins.
    let cell = std::cell::UnsafeCell::new(VENDORED_FONT.to_vec());
    let bytes: &[u8] = unsafe { &*cell.get() };
    let pin = font_sha256(bytes);
    let rp = RenderProfile::strict(
        "x".to_string(),
        tuisnap::FontFaces {
            regular: bytes,
            bold: VENDORED_FONT_BOLD,
            italic: VENDORED_FONT_ITALIC,
            bold_italic: VENDORED_FONT_BOLD_ITALIC,
        },
        [
            &pin,
            VENDORED_FONT_BOLD_SHA256,
            VENDORED_FONT_ITALIC_SHA256,
            VENDORED_FONT_BOLD_ITALIC_SHA256,
        ],
        vec![],
        16.0,
        10,
        21,
        12,
        2,
        PalettePolicy::xterm(),
        CursorPolicy::Show,
        BlinkPhase::On,
        MissingGlyphPolicy::Placeholder,
        RENDERER_VERSION,
    )
    .unwrap();
    unsafe {
        let slot = &mut (&mut (*cell.get()))[mid];
        *slot = slot.wrapping_add(1);
    }
    let err = Renderer::for_render_profile(&rp)
        .err()
        .expect("face swap must refuse render");
    assert!(
        err.to_string().contains("sha256 mismatch at render"),
        "{err}"
    );
}

// ---------------------------------------------------------------------------
// V03/V04: grapheme qualification corpus — hand-authored expected grids.
// Each case states its expected (symbol, width, continuation) cells; the
// test asserts the adapted grid matches EXACTLY and the render keeps the
// source geometry (dims from source widths, never from fallback metrics).
// ---------------------------------------------------------------------------

/// (case name, lead symbol, expected width, covered-by-pinned-chain?)
const CORPUS: &[(&str, &str, u8, bool)] = &[
    ("combining", "e\u{301}", 1, true), // e + U+0301 overlays at one origin
    ("cjk", "東", 2, true),             // vendored CJK fallback face
    ("nerd", "\u{f015}", 1, true),      // U+F015 primary Nerd coverage
    ("box", "╔", 1, true),
    ("box-h", "═", 1, true),
    ("braille", "⠋", 1, true),
    ("block-full", "█", 1, true),
    ("block-shade", "▓", 1, true),
    ("symbol-star", "★", 1, true),  // vendored Symbols2 fallback face
    ("coffee", "☕", 2, true),      // wide, vendored fallback face
    ("emoji-crab", "🦀", 2, false), // POLICY: wide; uncovered → strict fails
];

#[test]
fn grapheme_corpus_grids_match_hand_authored_expectations() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    for (name, symbol, width, covered) in CORPUS {
        let mut leads = vec![cell(0, 0, symbol, *width)];
        if *width == 2 {
            leads.push(cont(1, 0));
        }
        let screen = screen_from_leads(6, 2, leads);
        // Independent grid expectation: exact widths/continuations.
        let frame = frame_from_screen(&screen, "qual");
        let lead = frame.get(0, 0).unwrap();
        assert_eq!(lead.symbol, *symbol, "{name}");
        assert_eq!(lead.width, *width, "{name}");
        assert!(!lead.continuation, "{name}");
        if *width == 2 {
            let c = frame.get(1, 0).unwrap();
            assert!(
                c.continuation && c.width == 0 && c.symbol.is_empty(),
                "{name}"
            );
        }
        // Source widths control layout: image dims derive from the grid.
        let rendered = render_screen(&screen, &rp).unwrap();
        let img = decode(&rendered.png);
        let (ew, eh) = rp.image_size(6, 2);
        assert_eq!((img.width(), img.height()), (ew, eh), "{name}");
        // Coverage expectation is per-case, not renderer-derived.
        if *covered {
            assert!(
                rendered.fidelity.missing.is_empty(),
                "{name}: {:?}",
                rendered.fidelity.missing
            );
        } else {
            assert_eq!(rendered.fidelity.missing.len(), 1, "{name}");
            assert!(!rendered.fidelity.to_json().contains("faithful"), "{name}");
        }
    }
}

#[test]
fn fallback_never_shifts_the_grid() {
    // Same hand-authored frame, full chain vs NO fallback chain: geometry
    // (dims + every pixel outside the served span) must be identical —
    // fallback only fills ink inside the source-width span (V04).
    let full = strict_placeholder(VENDORED_FALLBACK_FACES.to_vec());
    let bare = strict_placeholder(vec![]);
    let screen = screen_from_leads(
        8,
        2,
        vec![cell(0, 0, "東", 2), cont(1, 0), cell(3, 0, "A", 1)],
    );
    let a = render_screen(&screen, &full).unwrap();
    let b = render_screen(&screen, &bare).unwrap();
    assert!(!a.fidelity.fallback_glyphs.is_empty());
    assert_eq!(b.fidelity.missing.len(), 1);
    let ia = decode(&a.png);
    let ib = decode(&b.png);
    assert_eq!((ia.width(), ia.height()), (ib.width(), ib.height()));
    // Served span: row 0, cells 0..2 → pixel rect.
    let (cw, ch, pad, u) = (10u32, 21u32, 12u32, 2u32);
    let (x0, x1) = (pad * u, (pad + 2 * cw) * u);
    let (y0, y1) = (pad * u, (pad + ch) * u);
    let mut inside_diffs = 0;
    for y in 0..ia.height() {
        for x in 0..ia.width() {
            let same = ia.get_pixel(x, y) == ib.get_pixel(x, y);
            let inside = x >= x0 && x < x1 && y >= y0 && y < y1;
            if inside {
                if !same {
                    inside_diffs += 1;
                }
            } else {
                assert!(same, "fallback moved pixels outside its span at ({x},{y})");
            }
        }
    }
    assert!(
        inside_diffs > 20,
        "glyph vs tofu must differ inside the span"
    );
}

#[test]
fn clipped_styles_draw_across_trailing_spaces() {
    // Hand-authored: underline+strike on blank cells must ink pixels (real
    // terminals decorate whitespace); image geometry is unchanged.
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let deco = Mods {
        underline: true,
        strikethrough: true,
        ..Mods::default()
    };
    let mut blanks: Vec<Cell> = (0..4).map(|x| cell(x, 0, " ", 1)).collect();
    for c in &mut blanks {
        c.mods = deco;
    }
    let decorated = screen_from_leads(8, 2, blanks);
    let plain = screen_from_leads(8, 2, vec![]);
    let a = decode(&render_screen(&decorated, &rp).unwrap().png);
    let b = decode(&render_screen(&plain, &rp).unwrap().png);
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let changed = a.pixels().zip(b.pixels()).filter(|(x, y)| x != y).count();
    assert!(
        changed > 50,
        "decorations across blanks changed {changed} px"
    );
}

#[test]
fn styled_space_paints_exact_palette_color() {
    // Independent pixel expectation: an Indexed(1) space centers on
    // xterm red (205,49,49) — no glyph ink involved.
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let mut c = cell(2, 1, " ", 1);
    c.bg = Color::Indexed(1);
    let screen = screen_from_leads(6, 3, vec![c]);
    let img = decode(&render_screen(&screen, &rp).unwrap().png);
    let (cw, ch, pad, u) = (10u32, 21u32, 12u32, 2u32);
    let (px, py) = ((pad + 2 * cw) * u + 10, (pad + ch) * u + 21);
    assert_eq!(img.get_pixel(px, py), &image::Rgb([205, 49, 49]));
}

// ---------------------------------------------------------------------------
// V05: strict missing-glyph policy.
// ---------------------------------------------------------------------------

#[test]
fn strict_missing_glyph_fails_with_exact_diagnostics() {
    let rp = RenderProfile::vendored(); // Strict by default.
    let screen = screen_from_leads(8, 2, vec![cell(3, 0, "🦀", 2), cont(4, 0)]);
    let err = render_screen(&screen, &rp)
        .err()
        .expect("strict must fail on uncovered glyph");
    let msg = err.to_string();
    assert!(msg.contains("strict missing-glyph"), "{msg}");
    assert!(msg.contains("U+1F980"), "{msg}");
    assert!(msg.contains("(3,0)"), "{msg}");
    // Explicit Placeholder renders + reports, never claims faithful.
    let rp = rp.with_missing(MissingGlyphPolicy::Placeholder);
    let r = render_screen(&screen, &rp).unwrap();
    assert!(r.fidelity.approximate);
    assert_eq!(r.fidelity.missing.len(), 1);
    assert_eq!(
        r.fidelity.missing[0].codepoints,
        vec!["U+1F980".to_string()]
    );
    assert!(!r.fidelity.to_json().contains("faithful"));
}

// ---------------------------------------------------------------------------
// V07: blink intent preserved, stills sample a declared phase.
// ---------------------------------------------------------------------------

#[test]
fn blink_intent_preserved_and_phases_sampled() {
    let mut c = cell(1, 0, "X", 1);
    c.mods.blink = true;
    let screen = screen_from_leads(6, 2, vec![c]);
    // Intent survives adaptation to canonical state.
    let frame = frame_from_screen(&screen, "qual");
    assert!(frame.get(1, 0).unwrap().mods.blink);
    assert!(frame.to_json().contains("\"blink\":true"));

    let on = RenderProfile::vendored()
        .with_missing(MissingGlyphPolicy::Placeholder)
        .with_phase(BlinkPhase::On);
    let off = on.with_phase(BlinkPhase::Off);
    let a = render_screen(&screen, &on).unwrap().png;
    let b = render_screen(&screen, &off).unwrap().png;
    assert_ne!(a, b, "declared phases must sample different stills");
    // Off-phase cell region is pure background (ink + decorations gone).
    let img = decode(&b);
    let (cw, ch, pad, u) = (10u32, 21u32, 12u32, 2u32);
    let (x0, y0) = ((pad + cw) * u, pad * u);
    for y in y0..y0 + ch * u {
        for x in x0..x0 + cw * u {
            assert_eq!(img.get_pixel(x, y), &image::Rgb([0, 0, 0]), "({x},{y})");
        }
    }
    // SVG samples the same declared phase.
    let svg_on = tuisnap::render::render_svg_phased(&frame, &on.to_profile(), BlinkPhase::On);
    let svg_off = tuisnap::render::render_svg_phased(&frame, &off.to_profile(), BlinkPhase::Off);
    assert!(svg_on.contains('X'));
    assert!(!svg_off.contains('X'));
}

// ---------------------------------------------------------------------------
// V08: content-addressed cache.
// ---------------------------------------------------------------------------

static CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn cache_png() -> Vec<u8> {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    render_screen(&screen_from_leads(4, 2, vec![cell(0, 0, "Q", 1)]), &rp)
        .unwrap()
        .png
}

#[test]
fn cache_roundtrip_and_key_sensitivity() {
    let _g = CACHE_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let approved = tempfile::tempdir().unwrap();
    let mut cache = RenderCache::open(dir.path(), &[approved.path()]).unwrap();
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let screen = screen_from_leads(4, 2, vec![cell(0, 0, "Q", 1)]);
    let key = RenderCache::key_for(&screen, &rp);
    assert_eq!(key.len(), 64);
    assert!(cache.get(&key).is_none());
    let png = cache_png();
    cache.put(&key, &png).unwrap();
    assert_eq!(cache.stores(), 1);
    assert_eq!(cache.get(&key).unwrap(), png);
    assert_eq!(cache.hits(), 1);
    // Key moves with screen, profile, phase, and fallback order.
    let other = screen_from_leads(4, 2, vec![cell(0, 0, "R", 1)]);
    assert_ne!(RenderCache::key_for(&other, &rp), key);
    assert_ne!(
        RenderCache::key_for(&screen, &rp.with_phase(BlinkPhase::Off)),
        key
    );
    let mut rev = VENDORED_FALLBACK_FACES.to_vec();
    rev.reverse();
    assert_ne!(RenderCache::key_for(&screen, &strict_placeholder(rev)), key);
    assert_eq!(screen_content_hash(&screen), screen_content_hash(&screen));
    assert_ne!(screen_content_hash(&screen), screen_content_hash(&other));
}

#[test]
fn corrupt_and_incompatible_entries_rejected_and_counted() {
    let _g = CACHE_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut cache = RenderCache::open(dir.path(), &[]).unwrap();
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let screen = screen_from_leads(4, 2, vec![cell(0, 0, "Q", 1)]);
    let key = RenderCache::key_for(&screen, &rp);
    let entry = dir.path().join(format!("{key}.cache"));
    // Garbage bytes.
    std::fs::write(&entry, b"definitely not a cache entry").unwrap();
    assert!(cache.get(&key).is_none());
    assert_eq!(cache.rejected(), 1);
    assert!(!entry.exists(), "corrupt entry must be removed");
    // Wrong renderer version up front + real PNG behind.
    let mut bad = 0xFFFFu32.to_le_bytes().to_vec();
    bad.extend_from_slice(&cache_png());
    std::fs::write(&entry, &bad).unwrap();
    assert!(cache.get(&key).is_none());
    assert_eq!(cache.rejected(), 2);
    assert!(!entry.exists());
    assert_eq!(cache.hits(), 0);
}

#[test]
fn approved_roots_are_never_cache_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let err = RenderCache::open(dir.path(), &[dir.path()])
        .err()
        .expect("approved root must be refused");
    assert!(err.to_string().contains("never a render cache"), "{err}");
}

#[test]
fn no_cache_mode_disables_reads_and_writes_but_not_renders() {
    let _g = CACHE_LOCK.lock().unwrap();
    std::env::set_var("RENDER_NO_CACHE", "1");
    assert!(render_cache_disabled());
    let dir = tempfile::tempdir().unwrap();
    let mut cache = RenderCache::open(dir.path(), &[]).unwrap();
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let screen = screen_from_leads(4, 2, vec![cell(0, 0, "Q", 1)]);
    let key = RenderCache::key_for(&screen, &rp);
    cache.put(&key, &cache_png()).unwrap();
    assert_eq!(cache.stores(), 0, "put must be dropped in no-cache mode");
    assert!(cache.get(&key).is_none());
    assert!(dir.path().read_dir().unwrap().next().is_none());
    // Qualification renders still work with the escape set.
    assert!(!render_screen(&screen, &rp).unwrap().png.is_empty());
    std::env::remove_var("RENDER_NO_CACHE");
    assert!(!render_cache_disabled());
}

// ---------------------------------------------------------------------------
// V06: safe export — escaping, concealment vs redaction.
// ---------------------------------------------------------------------------

#[test]
fn export_escapes_untrusted_content() {
    assert_eq!(escape_html("<a>&\""), "&lt;a&gt;&amp;\"");
    assert_eq!(escape_html_attr("x\" onload=\""), "x&quot; onload=&quot;");
    let evil = "{\"s\":\"</script><img src=x onerror=y>\"}";
    let safe = escape_json_for_script(evil);
    assert!(!safe.contains("</script>"), "{safe}");
    assert!(safe.contains("\\u003c/script>"), "{safe}");
    // Still valid JSON, re-parses to the identical value.
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&safe).unwrap(),
        serde_json::from_str::<serde_json::Value>(evil).unwrap()
    );
    // End to end: hostile title cannot break out of the HTML document.
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let frame = frame_from_leads(4, 1, vec![cell(0, 0, "<", 1)]);
    let html = Renderer::for_render_profile(&rp)
        .unwrap()
        .render_html(&frame, "x\" onload=\"y")
        .unwrap();
    assert!(!html.contains("alt=\"x\" onload=\""), "{html}");
    let body = html.split("<body>").nth(1).unwrap();
    assert!(!body.split("<script").next().unwrap().contains("</script>"));
}

#[test]
fn concealment_hides_pixels_not_canonical_data() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let mut hiddens: Vec<Cell> = "SECRET"
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            let mut c = cell(i as u16, 0, &ch.to_string(), 1);
            c.mods.hidden = true;
            c
        })
        .collect();
    let screen = screen_from_leads(8, 2, hiddens.clone());
    for c in &mut hiddens {
        c.mods.hidden = false;
        c.symbol = " ".to_string();
    }
    let blanks = screen_from_leads(8, 2, hiddens);
    // Pixels: concealed SECRET == blank spaces.
    assert_eq!(
        render_screen(&screen, &rp).unwrap().png,
        render_screen(&blanks, &rp).unwrap().png
    );
    // Visible HTML/SVG carry no trace of the concealed text.
    let frame = frame_from_screen(&screen, "qual");
    let svg = tuisnap::render::render_svg(&frame, &rp.to_profile());
    assert!(!svg.contains("SECRET"), "{svg}");
    let html = Renderer::for_render_profile(&rp)
        .unwrap()
        .render_html(&frame, "t")
        .unwrap();
    let visible = html.split("<script").next().unwrap();
    assert!(!visible.contains("SECRET"), "{visible}");
    // Canonical data DOES retain it: concealment is not redaction.
    // (Per-cell JSON holds single symbols, so check each concealed cell.)
    let json = frame.to_json();
    for ch in "SECRET".chars() {
        assert!(json.contains(&format!("\"symbol\":\"{ch}\"")), "{ch}");
    }
    assert!(json.contains("\"hidden\":true"));
    assert!(tuisnap::render::ansi_dump(&frame).contains("SECRET"));
}

#[test]
fn redaction_destroys_content_and_validates() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let mut c = cell(0, 0, "S", 1);
    c.mods.hidden = true;
    let frame = frame_from_leads(6, 2, vec![c, cell(1, 0, "東", 2), cont(2, 0)]);
    let red = redact_frame(&frame);
    red.validate().unwrap();
    assert!(!red.to_json().contains('S'));
    assert!(!red.to_json().contains('東'));
    // Geometry, colors, cursor survive; content and mods do not.
    assert_eq!((red.cols, red.rows), (frame.cols, frame.rows));
    assert_eq!(red.get(1, 0).unwrap().width, 2);
    assert!(red.get(2, 0).unwrap().continuation);
    assert_eq!(red.get(0, 0).unwrap().symbol, "█");
    assert_eq!(red.get(1, 0).unwrap().symbol, "██");
    assert_eq!(red.get(0, 0).unwrap().mods, Mods::default());
    assert_ne!(
        render_frame_strict(&frame, &rp).unwrap().png,
        render_frame_strict(&red, &rp).unwrap().png
    );
    // Screen variant preserves the origin.
    let screen = Screen::from_frame(&frame).unwrap();
    let red_screen = redact_screen(&screen).unwrap();
    assert_eq!(red_screen.origin(), screen.origin());
    assert_eq!(red_screen.cols(), screen.cols());
    let red_frame = frame_from_screen(&red_screen, "qual");
    assert!(!red_frame.to_json().contains('東'));
}

// ---------------------------------------------------------------------------
// V10: opt-in byte contracts.
// ---------------------------------------------------------------------------

#[test]
fn contract_bytes_opt_in_and_exact() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let frame = frame_from_leads(6, 2, vec![cell(0, 0, "h", 1), cell(1, 0, "i", 1)]);
    let mut r = Renderer::for_render_profile(&rp).unwrap();
    let a = r.render_artifacts(&frame, "t").unwrap().contract();
    let b = r.render_artifacts(&frame, "t").unwrap().contract();
    check_contract_bytes(&a, &b).unwrap();
    // One changed cell breaks the contract with field + byte offset.
    let mutated = frame_from_leads(6, 2, vec![cell(0, 0, "H", 1), cell(1, 0, "i", 1)]);
    let c = r.render_artifacts(&mutated, "t").unwrap().contract();
    let err = check_contract_bytes(&c, &a).unwrap_err();
    assert!(err.to_string().contains("ansi"), "{err}");
    assert!(err.to_string().contains("byte"), "{err}");
    // Contracts serialize for storage.
    let round: tuisnap::render::ContractBytes =
        serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
    assert_eq!(round, a);
}

// ---------------------------------------------------------------------------
// V09: portable offline bundle.
// ---------------------------------------------------------------------------

#[test]
fn bundle_is_offline_and_self_describing() {
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let frame = frame_from_leads(6, 2, vec![cell(0, 0, "A", 1)]);
    let mut r = Renderer::for_render_profile(&rp).unwrap();
    let artifacts = r.render_artifacts(&frame, "bundle").unwrap();
    let manifest = BundleManifest::for_render(&rp, &artifacts.fidelity);
    let dir = tempfile::tempdir().unwrap();
    let paths = artifacts.write_bundle(dir.path(), &manifest).unwrap();
    assert_eq!(paths.len(), 6);
    for name in [
        "screen.ansi",
        "screen.txt",
        "screen.png",
        "screen.html",
        "fidelity.json",
        "manifest.json",
    ] {
        assert!(dir.path().join(name).is_file(), "{name}");
    }
    let html = std::fs::read_to_string(dir.path().join("screen.html")).unwrap();
    assert!(html.contains("data:image/png;base64,"), "PNG embedded");
    assert!(!html.contains("src=\"http"), "no external refs");
    assert!(!html.contains("href=\"http"), "no external refs");
    let m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(m["renderer_version"], RENDERER_VERSION);
    assert_eq!(m["profile_hash"], serde_json::Value::String(rp.hash()));
    assert_eq!(m["face_hashes"].as_array().unwrap().len(), 4);
    assert_eq!(m["fallback_faces"].as_array().unwrap().len(), 3);
    assert_eq!(m["approximate"], serde_json::Value::Bool(false));
}
