//! Compound-snapshot integration SPIKE (backlog I01–I05; de-risks M2).
//!
//! Question: can public Insta APIs carry a compound canonical-plus-PNG
//! snapshot lifecycle where both artifacts bind to one candidate generation?
//!
//! What public APIs sufficed:
//! - `insta::assert_snapshot!` (canonical text) and
//!   `insta::assert_binary_snapshot!("name.png", bytes)` (PNG sidecar).
//! - `insta::Settings::{set_snapshot_path, set_description,
//!   set_prepend_module_to_snapshot, set_comparator}` + `bind` for hermetic
//!   per-phase scopes (absolute tempdirs; `Path::join` with an absolute path
//!   replaces the default `tests/snapshots` location).
//! - `insta::Comparator` + `insta::DefaultComparator` (text delegation) for
//!   the decoded-pixel comparator. `INSTA_UPDATE` stays ambient (read-only):
//!   `set_var` is an `unsafe fn` in edition 2024 and cannot be used under the
//!   workspace lints. Pending-dependent simulations (accept/reject/interrupted)
//!   need the `new` behavior — failing assertions write `.snap.new` pendings
//!   AND fail without blessing — so they skip unless the effective mode
//!   writes pendings (see `common`); run with `INSTA_UPDATE=new` to force it.
//! - `insta::Snapshot::from_file` for direct comparator unit checks.
//!
//! Missing hook (I05): there is NO public `Snapshot::as_binary()` accessor.
//! Payload extraction needs `insta::internals::SnapshotContents` (public but
//! explicitly internal), and `MetaData::snapshot_kind` (binary extension) is
//! `pub(crate)`, so an external comparator cannot re-check extension equality
//! like `DefaultComparator` does. Upstream ask: `as_binary() -> Option<&[u8]>`.
//! `assert_json_snapshot!` over `insta_value` needs only the `json` feature,
//! but Insta serializes via its own `Content` pretty-printer (not reproducible
//! outside Insta), so the structured projection is pinned by direct asserts,
//! not by an approved JSON file.
//!
//! Review/reject flows tested (via file-level simulation of
//! `cargo insta accept` = rename `.snap.new` → `.snap` incl. sidecar, and
//! `cargo insta reject` = delete `.snap.new` files):
//! - green: canonical + PNG approved at one generation pass together (I01, I02).
//! - reject: accept canonical, reject PNG → mixed baseline fails re-run (I04).
//! - partial accept: accept canonical, leave PNG pending → re-run fails (I04).
//! - interrupted write: torn binary pending (sidecar lost) → accept refused,
//!   post-crash mixed baseline fails re-run, pending regenerates (C08, I04).
//!
//! Each generation is bound by a `generation_id` embedded in BOTH artifacts:
//! the `description` field of each `.snap` file (set via public
//! `Settings::set_description`, visible in review) and a `tEXt` chunk inside
//! the PNG bytes. [`check_consistent`] fails on any mismatch.
//!
//! Simulation artifact: Insta auto-suffixes repeat assertions of one name in
//! a single process (`name-2`), with no public opt-out — the dedup key is
//! `module::name` and does NOT include the test function, so names must also
//! be unique across `#[test]`s in one binary. Re-run phases therefore use
//! FRESH snapshot names over byte-copied approved state. Real M2 re-runs are
//! separate processes and unaffected.
//!
//! Adjacent finding (out of spike scope, needs a P0 owner): `crate::diff`'s
//! identical-bytes fast path returns `pixels_equal: true` without consulting
//! the [`AlphaPolicy`], so byte-identical semi-transparent PNGs pass under
//! `Opaque` although the policy demands failure on any non-255 alpha. The
//! policy test below uses distinctly-encoded identical pixels to exercise the
//! real decoded-pixel path.
//!
//! These tests never touch `tests/visual/approved`: all snapshot dirs are
//! per-test tempdirs.

mod common;

use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};

use tuiscotti::diff::AlphaPolicy;
use tuiscotti::insta_proto::{PngPixelComparator, insta_string, insta_value};
use tuiscotti::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, Screen, UnderlineStyle};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const GEN1: &str = "gen-001";
const GEN2: &str = "gen-002";
const PNG_GEN_KEYWORD: &str = "tuisnap:generation";

#[allow(clippy::too_many_arguments)]
fn cell(
    x: u16,
    y: u16,
    sym: &str,
    width: u8,
    continuation: bool,
    fg: Color,
    bg: Color,
    mods: Mods,
) -> Cell {
    Cell {
        x,
        y,
        symbol: sym.to_string(),
        width,
        continuation,
        fg,
        bg,
        mods,
        underline_color: Color::Default,
    }
}

fn mods_of(bold: bool, underline: bool, hidden: bool, blink: bool, reverse: bool) -> Mods {
    Mods {
        hidden,
        blink,
        bold,
        dim: false,
        italic: false,
        underline,
        underline_style: UnderlineStyle::None,
        strikethrough: false,
        reverse,
    }
}

/// 4x2 screen: indexed color, wide char + continuation, styled blank,
/// hidden+blink cell, visible blinking cursor. Nonzero origin.
fn screen_gen1() -> Screen {
    let cells = vec![
        cell(
            0,
            0,
            "A",
            1,
            false,
            Color::Indexed(1),
            Color::Default,
            mods_of(true, false, false, false, false),
        ),
        cell(
            1,
            0,
            "中",
            2,
            false,
            Color::Default,
            Color::Default,
            Mods::default(),
        ),
        cell(
            2,
            0,
            "",
            0,
            true,
            Color::Default,
            Color::Default,
            Mods::default(),
        ),
        cell(
            3,
            0,
            " ",
            1,
            false,
            Color::Default,
            Color::Indexed(4),
            Mods::default(),
        ),
        cell(
            0,
            1,
            "B",
            1,
            false,
            Color::Rgb(Rgb::new(1, 2, 3)),
            Color::Default,
            mods_of(false, true, false, false, false),
        ),
        cell(
            1,
            1,
            "s",
            1,
            false,
            Color::Default,
            Color::Default,
            mods_of(false, false, true, true, false),
        ),
        cell(
            2,
            1,
            "C",
            1,
            false,
            Color::Default,
            Color::Default,
            mods_of(false, false, false, false, true),
        ),
        cell(
            3,
            1,
            " ",
            1,
            false,
            Color::Default,
            Color::Default,
            Mods::default(),
        ),
    ];
    Screen::validate(
        4,
        2,
        5,
        7,
        cells,
        Cursor {
            x: 1,
            y: 0,
            visible: true,
            style: CursorStyle::Block,
            blinking: true,
        },
    )
    .unwrap()
}

/// Generation 2: one symbol change (the "app" changed).
fn screen_gen2() -> Screen {
    let mut s = screen_gen1();
    let cells: Vec<Cell> = s
        .cells()
        .iter()
        .map(|c| {
            let mut c = c.clone();
            if c.x == 0 && c.y == 0 {
                c.symbol = "Z".to_string();
            }
            c
        })
        .collect();
    let cursor = *s.cursor();
    s = Screen::validate(4, 2, 5, 7, cells, cursor).unwrap();
    s
}

fn rgba_image(pixels: &[[u8; 4]], w: u32, h: u32) -> image::RgbaImage {
    assert_eq!(pixels.len(), (w * h) as usize);
    let mut img = image::RgbaImage::new(w, h);
    for (i, p) in pixels.iter().enumerate() {
        img.put_pixel(i as u32 % w, i as u32 / w, image::Rgba(*p));
    }
    img
}

fn encode_png(
    img: &image::RgbaImage,
    compression: image::codecs::png::CompressionType,
    filter: image::codecs::png::FilterType,
) -> Vec<u8> {
    use image::ImageEncoder;
    use image::codecs::png::PngEncoder;
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(&mut buf, compression, filter)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    buf
}

fn pixels_gen1() -> [[u8; 4]; 16] {
    let mut p = [[0u8; 4]; 16];
    for (i, cell) in p.iter_mut().enumerate() {
        *cell = [(i as u8) * 16, 255 - (i as u8) * 8, 64, 255];
    }
    p
}

/// Approved PNG bytes for a generation: encoded pixels + tEXt generation tag.
fn png_for_generation(gen_pixels: &[[u8; 4]; 16], generation: &str) -> Vec<u8> {
    let img = rgba_image(gen_pixels, 4, 4);
    let raw = encode_png(
        &img,
        image::codecs::png::CompressionType::Default,
        image::codecs::png::FilterType::Adaptive,
    );
    png_insert_text(&raw, PNG_GEN_KEYWORD, generation)
}

fn png_gen1() -> Vec<u8> {
    png_for_generation(&pixels_gen1(), GEN1)
}

fn png_gen2() -> Vec<u8> {
    let mut p = pixels_gen1();
    p[5] = [9, 9, 9, 255];
    png_for_generation(&p, GEN2)
}

// ---------------------------------------------------------------------------
// PNG tEXt chunk helpers (generation binding inside the PNG bytes)
// ---------------------------------------------------------------------------

const PNG_SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = if crc & 1 == 1 { 0xEDB8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

/// Insert a `tEXt` chunk before `IEND`. Decoders ignore it (pixel verdict
/// unaffected); [`check_consistent`] reads it back.
fn png_insert_text(png: &[u8], keyword: &str, value: &str) -> Vec<u8> {
    assert!(png.starts_with(&PNG_SIG), "not a PNG");
    assert!(!keyword.contains('\0') && keyword.len() <= 79);
    let mut data = Vec::new();
    data.extend_from_slice(keyword.as_bytes());
    data.push(0);
    data.extend_from_slice(value.as_bytes());
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(&data);
    let mut crc_input = b"tEXt".to_vec();
    crc_input.extend_from_slice(&data);
    chunk.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    assert!(png.len() > 12 && &png[png.len() - 8..png.len() - 4] == b"IEND");
    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..png.len() - 12]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[png.len() - 12..]);
    out
}

fn png_find_text(png: &[u8], keyword: &str) -> Option<String> {
    if !png.starts_with(&PNG_SIG) || png.len() < 12 {
        return None;
    }
    let mut i = 8;
    while i + 8 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().ok()?) as usize;
        let typ = &png[i + 4..i + 8];
        if i + 8 + len + 4 > png.len() {
            return None;
        }
        if typ == b"tEXt" {
            let data = &png[i + 8..i + 8 + len];
            if let Some(z) = data.iter().position(|&b| b == 0) {
                if &data[..z] == keyword.as_bytes() {
                    return Some(String::from_utf8_lossy(&data[z + 1..]).into_owned());
                }
            }
        }
        if typ == b"IEND" {
            break;
        }
        i += 8 + len + 4;
    }
    None
}

// ---------------------------------------------------------------------------
// Insta harness: hermetic settings, approved-file writers, review simulation
// ---------------------------------------------------------------------------

/// Pending-dependent simulations (accept/reject/interrupted) need failing
/// assertions to write `.snap.new` pendings AND fail — the `new` behavior —
/// while approvals stay byte-identical (asserted in the reject test, so
/// accidental blessing is still impossible). `INSTA_UPDATE` is ambient-only
/// (`set_var` is an `unsafe fn` in edition 2024), so guard callers skip unless
/// the effective mode writes pendings (see `common`).
fn require_pending_mode() -> bool {
    if common::insta_writes_new_files() {
        return true;
    }
    eprintln!("skip: ambient INSTA_UPDATE does not write `.snap.new` pendings");
    false
}

fn settings_for(dir: &Path, generation: &str) -> insta::Settings {
    let mut s = insta::Settings::new();
    s.set_snapshot_path(dir);
    s.set_prepend_module_to_snapshot(false);
    s.set_description(format!("tuisnap generation {generation}"));
    s
}

fn payload_to_string(p: Box<dyn Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "<non-string panic>".to_string()
    }
}

/// Run a canonical-text assertion; Ok = insta passed, Err = insta failed.
fn run_canonical(dir: &Path, name: &str, screen: &Screen, generation: &str) -> Result<(), String> {
    let settings = settings_for(dir, generation);
    let name = name.to_string();
    let text = insta_string(screen);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        settings.bind(|| {
            insta::assert_snapshot!(name, text);
        });
    })) {
        Ok(()) => Ok(()),
        Err(p) => Err(payload_to_string(p)),
    }
}

/// Run a PNG assertion under the decoded-pixel comparator.
fn run_png(
    dir: &Path,
    name: &str,
    png: Vec<u8>,
    generation: &str,
    alpha: AlphaPolicy,
) -> Result<(), String> {
    let mut settings = settings_for(dir, generation);
    settings.set_comparator(Box::new(PngPixelComparator::new(alpha)));
    let full = format!("{name}.png");
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        settings.bind(|| {
            insta::assert_binary_snapshot!(full.as_str(), png);
        });
    })) {
        Ok(()) => Ok(()),
        Err(p) => Err(payload_to_string(p)),
    }
}

fn write_text_snap(dir: &Path, name: &str, generation: &str, body: &str) {
    let content = format!(
        "---\nsource: tests/insta_spike.rs\ndescription: tuisnap generation {generation}\nexpression: insta_string\n---\n{body}"
    );
    fs::write(dir.join(format!("{name}.snap")), content).unwrap();
}

fn write_binary_snap(dir: &Path, name: &str, generation: &str, sidecar: &[u8]) {
    let meta = format!(
        "---\nsource: tests/insta_spike.rs\ndescription: tuisnap generation {generation}\nexpression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
    );
    fs::write(dir.join(format!("{name}.snap")), meta).unwrap();
    fs::write(dir.join(format!("{name}.snap.png")), sidecar).unwrap();
}

fn snap_description(snap_path: &Path) -> Option<String> {
    let text = fs::read_to_string(snap_path).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(v) = line.trim().strip_prefix("description:") {
            let v = v.trim().trim_matches('"');
            return v.strip_prefix("tuisnap generation ").map(|g| g.to_string());
        }
    }
    None
}

/// Compound consistency gate (I04/C08): the approved canonical `.snap`, the
/// approved PNG `.snap`, and the PNG sidecar bytes must all carry the SAME
/// generation. Any mismatch (or missing binding) is an error.
fn check_consistent(dir: &Path, canonical: &str, png: &str) -> Result<(), String> {
    let c = snap_description(&dir.join(format!("{canonical}.snap")))
        .ok_or_else(|| format!("{canonical}.snap: missing generation binding"))?;
    let p = snap_description(&dir.join(format!("{png}.snap")))
        .ok_or_else(|| format!("{png}.snap: missing generation binding"))?;
    let sidecar = fs::read(dir.join(format!("{png}.snap.png")))
        .map_err(|e| format!("{png}.snap.png unreadable: {e}"))?;
    let t = png_find_text(&sidecar, PNG_GEN_KEYWORD)
        .ok_or_else(|| format!("{png}.snap.png: missing tEXt generation"))?;
    if c == p && p == t {
        Ok(())
    } else {
        Err(format!(
            "mixed compound baseline: canonical={c} png-meta={p} png-bytes={t}"
        ))
    }
}

/// Simulate `cargo insta accept` for ONE artifact: rename `.snap.new` (plus
/// binary sidecar `.snap.new.png`) into place. Refuses incomplete binary
/// pendings (torn write): metadata without pixels is never blessed.
fn accept_sim(dir: &Path, base: &str) -> Result<(), String> {
    let new = dir.join(format!("{base}.snap.new"));
    if !new.exists() {
        return Err(format!("{base}: no pending .snap.new to accept"));
    }
    let meta = fs::read_to_string(&new).map_err(|e| format!("{base}: {e}"))?;
    let is_binary = meta.lines().any(|l| l.trim() == "snapshot_kind: binary");
    let sidecar_new = dir.join(format!("{base}.snap.new.png"));
    if is_binary && !sidecar_new.exists() {
        return Err(format!(
            "{base}: incomplete binary pending (sidecar missing), refusing accept"
        ));
    }
    fs::rename(&new, dir.join(format!("{base}.snap"))).map_err(|e| format!("{base}: {e}"))?;
    if sidecar_new.exists() {
        fs::rename(&sidecar_new, dir.join(format!("{base}.snap.png")))
            .map_err(|e| format!("{base}: {e}"))?;
    }
    Ok(())
}

/// Simulate `cargo insta reject` for ONE artifact.
fn reject_sim(dir: &Path, base: &str) {
    let _ = fs::remove_file(dir.join(format!("{base}.snap.new")));
    let _ = fs::remove_file(dir.join(format!("{base}.snap.new.png")));
}

/// Copy approved state to fresh names (re-run phases; see header).
fn copy_approved(dir: &Path, from: &str, to: &str) {
    fs::copy(
        dir.join(format!("{from}.snap")),
        dir.join(format!("{to}.snap")),
    )
    .unwrap();
    let sidecar = dir.join(format!("{from}.snap.png"));
    if sidecar.exists() {
        fs::copy(sidecar, dir.join(format!("{to}.snap.png"))).unwrap();
    }
}

fn fresh_dir(test: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::Builder::new()
        .prefix(&format!("spike-{test}-"))
        .tempdir()
        .unwrap();
    let dir = tmp.path().join("snaps");
    fs::create_dir(&dir).unwrap();
    (tmp, dir)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

/// Raw gen1 PNG without the generation tag (CRC path exercised separately).
fn png_gen1_no_tag_for_test() -> Vec<u8> {
    let img = rgba_image(&pixels_gen1(), 4, 4);
    encode_png(
        &img,
        image::codecs::png::CompressionType::Default,
        image::codecs::png::FilterType::Adaptive,
    )
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
