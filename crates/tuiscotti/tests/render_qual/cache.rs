use super::*;
use tuiscotti::profile::{BlinkPhase, MissingGlyphPolicy, RenderProfile, VENDORED_FALLBACK_FACES};
use tuiscotti::render::{CacheKey, CacheOptions, RenderCache, render_screen, screen_content_hash};
use tuiscotti::{CursorStyle, FallbackFace, Rgb, UnderlineStyle};

#[test]
fn cache_roundtrip_and_key_sensitivity() {
    let dir = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let approved = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let mut cache = RenderCache::open(dir.path(), &[approved.path()])
        .expect("RenderCache::open(dir.path(), &[approved.path()]) succeeds");
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let screen =
        screen_from_leads(4, 2, vec![cell(0, 0, "Q", 1)]).expect("screen_from_leads succeeds");
    let key = RenderCache::key_for(&screen, &rp);
    assert_eq!(key.hex().len(), 64);
    assert!(cache.get(&key).is_none());
    let png = cache_png().expect("cache_png succeeds");
    cache
        .put(&key, &png)
        .expect("cache.put(&key, &png) succeeds");
    assert_eq!(cache.stores(), 1);
    assert_eq!(cache.get(&key).expect("cache.get(&key) is some"), png);
    assert_eq!(cache.hits(), 1);
    // Key moves with screen, profile, phase, and fallback order.
    let other =
        screen_from_leads(4, 2, vec![cell(0, 0, "R", 1)]).expect("screen_from_leads succeeds");
    assert_ne!(RenderCache::key_for(&other, &rp), key);
    assert_ne!(
        RenderCache::key_for(&screen, &rp.with_phase(BlinkPhase::Off)),
        key
    );
    let mut rev = VENDORED_FALLBACK_FACES.to_vec();
    rev.reverse();
    assert_ne!(
        RenderCache::key_for(
            &screen,
            &strict_placeholder(rev).expect("strict_placeholder succeeds")
        ),
        key
    );
    assert_eq!(screen_content_hash(&screen), screen_content_hash(&screen));
    assert_ne!(screen_content_hash(&screen), screen_content_hash(&other));
}

#[test]
fn corrupt_and_incompatible_entries_rejected_and_counted() {
    let dir = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let mut cache =
        RenderCache::open(dir.path(), &[]).expect("RenderCache::open(dir.path(), &[]) succeeds");
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let screen =
        screen_from_leads(4, 2, vec![cell(0, 0, "Q", 1)]).expect("screen_from_leads succeeds");
    let key = RenderCache::key_for(&screen, &rp);
    let entry = dir.path().join(key.file_name());
    // Garbage bytes.
    std::fs::write(&entry, b"definitely not a cache entry")
        .expect("std::fs::write(&entry, b\"definitely not a cache entry\") succeeds");
    assert!(cache.get(&key).is_none());
    assert_eq!(cache.rejected(), 1);
    assert!(!entry.exists(), "corrupt entry must be removed");
    // V1 entry (bare renderer-version prefix + PNG, no header/checksum): rejected.
    let mut v1 = RENDERER_VERSION.to_le_bytes().to_vec();
    v1.extend_from_slice(&cache_png().expect("cache_png succeeds"));
    std::fs::write(&entry, &v1).expect("std::fs::write(&entry, &v1) succeeds");
    assert!(cache.get(&key).is_none());
    assert_eq!(cache.rejected(), 2);
    assert!(!entry.exists());
    assert_eq!(cache.hits(), 0);
}

#[test]
fn approved_roots_are_never_cache_dirs() {
    let dir = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let err =
        RenderCache::open(dir.path(), &[dir.path()]).expect_err("approved root must be refused");
    assert!(err.to_string().contains("never a render cache"), "{err}");
}

#[test]
fn no_cache_mode_disables_reads_and_writes_but_not_renders() {
    // No-cache mode is an explicit per-cache context (F12): no global is
    // flipped, so this test needs no lock and cannot affect its neighbors.
    let dir = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let mut cache =
        RenderCache::open_with_options(dir.path(), &[], CacheOptions { no_cache: true })
            .expect("RenderCache::open_with_options succeeds");
    assert!(cache.is_no_cache());
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let screen =
        screen_from_leads(4, 2, vec![cell(0, 0, "Q", 1)]).expect("screen_from_leads succeeds");
    let key = RenderCache::key_for(&screen, &rp);
    cache
        .put(&key, &cache_png().expect("cache_png succeeds"))
        .expect("cache.put(&key, &cache_png()) succeeds");
    assert_eq!(cache.stores(), 0, "put must be dropped in no-cache mode");
    assert!(cache.get(&key).is_none());
    assert!(
        dir.path()
            .read_dir()
            .expect("dir.path().read_dir() succeeds")
            .next()
            .is_none()
    );
    // Qualification renders still work with the option set.
    assert!(
        !render_screen(&screen, &rp)
            .expect("render_screen(&screen, &rp) succeeds")
            .png
            .is_empty()
    );
}

#[test]
fn independent_caches_keep_their_own_options_concurrently() {
    // A no-cache instance beside a plain instance, driven from two
    // threads: each behaves per its own options (F12 explicit contexts —
    // no process-global mode can leak between them).
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let screen =
        screen_from_leads(4, 2, vec![cell(0, 0, "Q", 1)]).expect("screen_from_leads succeeds");
    let key = RenderCache::key_for(&screen, &rp);
    let png = cache_png().expect("cache_png succeeds");
    std::thread::scope(|scope| {
        let plain = scope.spawn(|| {
            let dir = tempfile::tempdir().expect("tempdir succeeds");
            let mut cache = RenderCache::open(dir.path(), &[]).expect("RenderCache::open succeeds");
            cache.put(&key, &png).expect("put succeeds");
            assert_eq!(cache.get(&key).expect("hit"), png);
            assert_eq!((cache.stores(), cache.hits()), (1, 1));
        });
        let uncached = scope.spawn(|| {
            let dir = tempfile::tempdir().expect("tempdir succeeds");
            let mut cache =
                RenderCache::open_with_options(dir.path(), &[], CacheOptions { no_cache: true })
                    .expect("open_with_options succeeds");
            cache.put(&key, &png).expect("put succeeds");
            assert_eq!(cache.stores(), 0);
            assert!(cache.get(&key).is_none());
        });
        plain.join().expect("plain joins");
        uncached.join().expect("uncached joins");
    });
}

// ---------------------------------------------------------------------------
// F09: fingerprint soundness.
// ---------------------------------------------------------------------------

fn styled_lead() -> Cell {
    Cell {
        x: 0,
        y: 0,
        symbol: "A".to_string(),
        width: 1,
        continuation: false,
        fg: Color::Indexed(1),
        bg: Color::Rgb(Rgb::new(10, 20, 30)),
        mods: Mods {
            bold: true,
            italic: true,
            underline: true,
            underline_style: UnderlineStyle::Single,
            ..Mods::default()
        },
        underline_color: Color::Indexed(5),
    }
}

fn screen_of(lead: Cell) -> Result<Screen, String> {
    screen_from_leads(4, 2, vec![lead]).map_err(|e| e.to_string())
}

fn screen_with_cursor(cursor: Cursor) -> Result<Screen, String> {
    let mut cells = Vec::with_capacity(8);
    for y in 0..2 {
        for x in 0..4 {
            cells.push(Cell::blank(x, y));
        }
    }
    cells[0] = styled_lead();
    Screen::validate(4, 2, 0, 0, cells, cursor).map_err(|e| e.to_string())
}

fn screen_at_origin(ox: i32, oy: i32) -> Result<Screen, String> {
    let mut cells = Vec::with_capacity(8);
    for y in 0..2 {
        for x in 0..4 {
            cells.push(Cell::blank(x, y));
        }
    }
    cells[0] = styled_lead();
    Screen::validate(4, 2, ox, oy, cells, Cursor::default()).map_err(|e| e.to_string())
}

fn placeholder_rp() -> RenderProfile<'static> {
    RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder)
}

/// One-field cell mutations of [`styled_lead`]: (label, mutated cell).
fn cell_variants() -> Vec<(&'static str, Cell)> {
    let mut variants: Vec<(&str, Cell)> = Vec::new();
    // Symbol + position.
    let mut c = styled_lead();
    c.symbol = "B".to_string();
    variants.push(("symbol", c));
    let mut c = styled_lead();
    c.x = 1;
    variants.push(("x", c));
    // Foreground across all three color shapes.
    let mut c = styled_lead();
    c.fg = Color::Rgb(Rgb::new(1, 2, 3));
    variants.push(("fg-rgb", c));
    let mut c = styled_lead();
    c.fg = Color::Default;
    variants.push(("fg-default", c));
    let mut c = styled_lead();
    c.fg = Color::Indexed(2);
    variants.push(("fg-index", c));
    // Background across all three color shapes.
    let mut c = styled_lead();
    c.bg = Color::Indexed(4);
    variants.push(("bg-index", c));
    let mut c = styled_lead();
    c.bg = Color::Default;
    variants.push(("bg-default", c));
    // Every modifier bit, one at a time.
    for label in ["hidden", "blink", "dim", "strikethrough", "reverse"] {
        let mut cell = styled_lead();
        match label {
            "hidden" => cell.mods.hidden = true,
            "blink" => cell.mods.blink = true,
            "dim" => cell.mods.dim = true,
            "strikethrough" => cell.mods.strikethrough = true,
            _ => cell.mods.reverse = true,
        }
        variants.push((label, cell));
    }
    for label in ["bold", "italic", "underline"] {
        let mut cell = styled_lead();
        match label {
            "bold" => cell.mods.bold = false,
            "italic" => cell.mods.italic = false,
            _ => {
                cell.mods.underline = false;
                cell.mods.underline_style = UnderlineStyle::None;
            }
        }
        variants.push((label, cell));
    }
    // Underline style refinement alone (F09: the renderer draws styles).
    for style in [
        UnderlineStyle::Double,
        UnderlineStyle::Curly,
        UnderlineStyle::Dotted,
        UnderlineStyle::Dashed,
    ] {
        let mut cell = styled_lead();
        cell.mods.underline_style = style;
        variants.push(("underline-style", cell));
    }
    // Underline color alone (F09: the headline omission).
    let mut c = styled_lead();
    c.underline_color = Color::Indexed(6);
    variants.push(("underline-color-index", c));
    let mut c = styled_lead();
    c.underline_color = Color::Rgb(Rgb::new(9, 9, 9));
    variants.push(("underline-color-rgb", c));
    let mut c = styled_lead();
    c.underline_color = Color::Default;
    variants.push(("underline-color-default", c));
    variants
}

#[test]
fn one_field_screen_mutations_each_move_the_key() {
    let rp = placeholder_rp();
    let base = screen_of(styled_lead()).expect("screen_of succeeds");
    let base_key = RenderCache::key_for(&base, &rp);
    let base_hash = screen_content_hash(&base);

    let mut variants: Vec<(&str, Screen)> = Vec::new();
    for (label, cell) in cell_variants() {
        variants.push((label, screen_of(cell).expect("screen_of succeeds")));
    }
    // Dims.
    variants.push((
        "cols",
        screen_from_leads(5, 2, vec![styled_lead()]).expect("screen_from_leads succeeds"),
    ));
    variants.push((
        "rows",
        screen_from_leads(4, 3, vec![styled_lead()]).expect("screen_from_leads succeeds"),
    ));

    for (label, screen) in &variants {
        assert_ne!(
            screen_content_hash(screen),
            base_hash,
            "screen hash must move for {label}"
        );
        assert_ne!(
            RenderCache::key_for(screen, &rp),
            base_key,
            "cache key must move for {label}"
        );
    }
}

#[test]
fn one_field_cursor_mutations_each_move_the_key() {
    let rp = placeholder_rp();
    // Cursor fields, one at a time (validated cursor on a 4x2 grid).
    let cursor_base = Cursor::default();
    let cursor_variants = [
        (
            "cursor-x",
            Cursor {
                x: 1,
                ..cursor_base
            },
        ),
        (
            "cursor-y",
            Cursor {
                y: 1,
                ..cursor_base
            },
        ),
        (
            "cursor-visible",
            Cursor {
                visible: !cursor_base.visible,
                ..cursor_base
            },
        ),
        (
            "cursor-underline",
            Cursor {
                style: CursorStyle::Underline,
                ..cursor_base
            },
        ),
        (
            "cursor-bar",
            Cursor {
                style: CursorStyle::Bar,
                ..cursor_base
            },
        ),
        (
            "cursor-blinking",
            Cursor {
                blinking: !cursor_base.blinking,
                ..cursor_base
            },
        ),
    ];
    let base_cursor_key = RenderCache::key_for(
        &screen_with_cursor(cursor_base).expect("screen_with_cursor succeeds"),
        &rp,
    );
    for (label, cursor) in cursor_variants {
        assert_ne!(
            RenderCache::key_for(
                &screen_with_cursor(cursor).expect("screen_with_cursor succeeds"),
                &rp
            ),
            base_cursor_key,
            "cache key must move for {label}"
        );
    }

    // Origin is NOT rendering-relevant (the screen→frame adaptation drops
    // it): same pixels, same key.
    assert_eq!(
        RenderCache::key_for(
            &screen_at_origin(0, 0).expect("screen_at_origin succeeds"),
            &rp
        ),
        RenderCache::key_for(
            &screen_at_origin(7, -3).expect("screen_at_origin succeeds"),
            &rp
        ),
        "grid origin must not affect the key"
    );
}

struct ProfileParts {
    name: String,
    font_px: f32,
    cell_w: u32,
    cell_h: u32,
    pad: u32,
    scale: u32,
    palette: PalettePolicy,
    cursor: CursorPolicy,
    blink: BlinkPhase,
    missing: MissingGlyphPolicy,
    fallbacks: Vec<FallbackFace<'static>>,
}

impl ProfileParts {
    fn base() -> Self {
        Self {
            name: "qual".to_string(),
            font_px: 16.0,
            cell_w: 10,
            cell_h: 21,
            pad: 12,
            scale: 2,
            palette: PalettePolicy::xterm(),
            cursor: CursorPolicy::Show,
            blink: BlinkPhase::On,
            missing: MissingGlyphPolicy::Placeholder,
            fallbacks: VENDORED_FALLBACK_FACES.to_vec(),
        }
    }

    fn build(self) -> Result<RenderProfile<'static>, String> {
        RenderProfile::strict(
            self.name,
            VENDORED_FACES,
            [
                VENDORED_FONT_SHA256,
                VENDORED_FONT_BOLD_SHA256,
                VENDORED_FONT_ITALIC_SHA256,
                VENDORED_FONT_BOLD_ITALIC_SHA256,
            ],
            self.fallbacks,
            self.font_px,
            self.cell_w,
            self.cell_h,
            self.pad,
            self.scale,
            self.palette,
            self.cursor,
            self.blink,
            self.missing,
            RENDERER_VERSION,
        )
        .map_err(|e| e.to_string())
    }
}

#[test]
fn one_field_profile_mutations_each_move_the_key() {
    let screen = screen_of(styled_lead()).expect("screen_of succeeds");
    let base_key = RenderCache::key_for(
        &screen,
        &ProfileParts::base().build().expect("profile builds"),
    );

    let mut named: Vec<(&str, RenderProfile<'static>)> = Vec::new();
    let mut p = ProfileParts::base();
    p.name = "other".to_string();
    named.push(("name", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.font_px = 20.0;
    named.push(("font_px", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.cell_w = 12;
    named.push(("cell_w", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.cell_h = 22;
    named.push(("cell_h", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.pad = 0;
    named.push(("pad", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.scale = 1;
    named.push(("scale", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.palette.default_fg = Rgb::new(1, 1, 1);
    named.push(("palette-fg", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.palette.default_bg = Rgb::new(2, 2, 2);
    named.push(("palette-bg", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.cursor = CursorPolicy::Hide;
    named.push(("cursor", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.blink = BlinkPhase::Off;
    named.push(("blink", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.missing = MissingGlyphPolicy::Strict;
    named.push(("missing", p.build().expect("profile builds")));
    // Fallback chain: order AND per-face identity participate.
    let mut p = ProfileParts::base();
    p.fallbacks.reverse();
    named.push(("fallback-order", p.build().expect("profile builds")));
    let mut p = ProfileParts::base();
    p.fallbacks[0].desc = "renamed-face";
    named.push(("fallback-desc", p.build().expect("profile builds")));

    for (label, rp) in &named {
        assert_ne!(
            RenderCache::key_for(&screen, rp),
            base_key,
            "cache key must move for profile field {label}"
        );
    }

    // Face pins cannot vary on a VALID profile: strict construction refuses
    // a pin that does not match the bytes, so no second key exists there.
    let err = RenderProfile::strict(
        "qual".to_string(),
        VENDORED_FACES,
        [
            "00",
            VENDORED_FONT_BOLD_SHA256,
            VENDORED_FONT_ITALIC_SHA256,
            VENDORED_FONT_BOLD_ITALIC_SHA256,
        ],
        VENDORED_FALLBACK_FACES.to_vec(),
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
    .expect_err("wrong face pin must refuse the profile");
    assert!(err.to_string().contains("sha256 mismatch"), "{err}");
}

#[test]
fn typed_keys_reject_traversal_and_garbage() {
    assert!(CacheKey::parse(&"ab".repeat(32)).is_ok());
    let bad_keys = [
        String::new(),
        "abc".to_string(),
        "ab".repeat(31),
        "ab".repeat(33),
        "../escape-the-cache-dir..............................".to_string(),
        "AB".repeat(32),
        "zz".repeat(32),
        "ab cd".repeat(13),
        "\0".repeat(64),
    ];
    for bad in &bad_keys {
        assert!(
            CacheKey::parse(bad).is_err(),
            "key {bad:?} must be rejected"
        );
    }
    // Round-trip: parsed keys keep their bytes.
    let key = CacheKey::parse(&"ab".repeat(32)).expect("valid key parses");
    assert_eq!(key.hex(), "ab".repeat(32));
    assert_eq!(key.file_name(), format!("{}.cache", "ab".repeat(32)));
    assert_eq!(key.to_string(), "ab".repeat(32));
}

#[test]
fn truncation_is_rejected_and_removed() {
    let dir = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let mut cache =
        RenderCache::open(dir.path(), &[]).expect("RenderCache::open(dir.path(), &[]) succeeds");
    let rp = placeholder_rp();
    let screen = screen_of(styled_lead()).expect("screen_of succeeds");
    let key = RenderCache::key_for(&screen, &rp);
    let png = cache_png().expect("cache_png succeeds");
    cache.put(&key, &png).expect("put succeeds");
    let entry = dir.path().join(key.file_name());
    let full = std::fs::read(&entry).expect("read entry");
    assert!(full.len() > 64);
    // Every truncation point: magic intact or not, all must miss + evict.
    for cut in [10, 81, 100, full.len() / 2, full.len() - 1] {
        std::fs::write(&entry, &full[..cut]).expect("write truncation");
        assert!(cache.get(&key).is_none(), "truncated@{cut} must miss");
        assert!(!entry.exists(), "truncated@{cut} must be removed");
    }
    assert_eq!(cache.rejected(), 5);
    assert_eq!(cache.hits(), 0);
    // A single flipped payload byte breaks the checksum the same way.
    std::fs::write(&entry, &full).expect("rewrite full entry");
    let mut bad = full.clone();
    let last = bad.len() - 1;
    bad[last] ^= 0xFF;
    std::fs::write(&entry, &bad).expect("write bit-flipped entry");
    assert!(cache.get(&key).is_none());
    assert_eq!(cache.rejected(), 6);
}

#[test]
fn wrong_entry_payload_is_rejected_and_removed() {
    let dir = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let mut cache =
        RenderCache::open(dir.path(), &[]).expect("RenderCache::open(dir.path(), &[]) succeeds");
    let rp = placeholder_rp();
    let key_a = RenderCache::key_for(&screen_of(styled_lead()).expect("screen_of succeeds"), &rp);
    let mut other = styled_lead();
    other.symbol = "Z".to_string();
    let key_b = RenderCache::key_for(&screen_of(other).expect("screen_of succeeds"), &rp);
    assert_ne!(key_a, key_b);
    let png = cache_png().expect("cache_png succeeds");
    cache.put(&key_b, &png).expect("put under B succeeds");
    // A fully VALID entry for B planted under A's name: key binding rejects it.
    let bytes_b = std::fs::read(dir.path().join(key_b.file_name())).expect("read B entry");
    std::fs::write(dir.path().join(key_a.file_name()), &bytes_b).expect("plant under A");
    assert!(cache.get(&key_a).is_none(), "wrong-entry payload must miss");
    assert_eq!(cache.rejected(), 1);
    assert!(
        !dir.path().join(key_a.file_name()).exists(),
        "wrong-entry file must be removed"
    );
    // B itself still hits: the eviction was scoped to A's path.
    assert_eq!(cache.get(&key_b).expect("B still hits"), png);
}

#[test]
fn undecodable_payloads_are_never_stored() {
    let dir = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let mut cache =
        RenderCache::open(dir.path(), &[]).expect("RenderCache::open(dir.path(), &[]) succeeds");
    let rp = placeholder_rp();
    let key = RenderCache::key_for(&screen_of(styled_lead()).expect("screen_of succeeds"), &rp);
    // Magic + length alone would have passed the old check; full decode refuses.
    let mut fake = b"\x89PNG\r\n\x1a\n".to_vec();
    fake.extend(std::iter::repeat_n(0u8, 256));
    assert!(cache.put(&key, &fake).is_err());
    assert!(cache.put(&key, b"short").is_err());
    assert_eq!(cache.stores(), 0);
    assert!(cache.get(&key).is_none());
    assert!(
        dir.path()
            .read_dir()
            .expect("read_dir succeeds")
            .next()
            .is_none(),
        "refused puts must leave no files behind"
    );
}

#[test]
fn interrupted_writes_leave_no_live_entry() {
    let dir = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let mut cache =
        RenderCache::open(dir.path(), &[]).expect("RenderCache::open(dir.path(), &[]) succeeds");
    let rp = placeholder_rp();
    let key = RenderCache::key_for(&screen_of(styled_lead()).expect("screen_of succeeds"), &rp);
    // A crashed publish leaves only an orphaned temp file: no live name exists.
    std::fs::write(dir.path().join(".deadbeef.tmp-1-1"), b"partial").expect("write orphan tmp");
    assert!(cache.get(&key).is_none());
    assert_eq!(
        cache.rejected(),
        0,
        "missing entry is a miss, not a rejection"
    );
    // A short write that DID reach the live name is rejected + removed, and
    // the next publish recovers cleanly.
    std::fs::write(dir.path().join(key.file_name()), b"partial-entry")
        .expect("write partial live entry");
    assert!(cache.get(&key).is_none());
    assert_eq!(cache.rejected(), 1);
    let png = cache_png().expect("cache_png succeeds");
    cache.put(&key, &png).expect("put recovers");
    assert_eq!(cache.get(&key).expect("republished entry hits"), png);
}

#[test]
fn cache_and_approved_roots_must_not_overlap_in_either_direction() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    // Cache dir INSIDE the approved tree.
    let approved = tmp.path().join("approved");
    std::fs::create_dir(&approved).expect("create approved");
    let inside = approved.join("cache");
    let err = RenderCache::open(&inside, &[&approved]).expect_err("cache inside approved refused");
    assert!(err.to_string().contains("never a render cache"), "{err}");
    // Approved root INSIDE the cache dir (approved tree need not exist yet).
    let cache = tmp.path().join("cache");
    let err = RenderCache::open(&cache, &[&cache.join("nested-approved")])
        .expect_err("approved inside cache refused");
    assert!(err.to_string().contains("never a render cache"), "{err}");
    // `..` segments cannot smuggle past the comparison.
    let sneaky = tmp.path().join("x").join("..").join("approved");
    let err = RenderCache::open(&approved, &[&sneaky]).expect_err("dot-dot alias refused");
    assert!(err.to_string().contains("never a render cache"), "{err}");
    // Disjoint trees still open fine.
    RenderCache::open(&tmp.path().join("ok-cache"), &[&approved]).expect("disjoint cache opens");
}

#[cfg(unix)]
#[test]
fn symlink_aliases_of_approved_roots_are_refused() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let real = tmp.path().join("real-approved");
    std::fs::create_dir(&real).expect("create real approved");
    let alias = tmp.path().join("alias-approved");
    std::os::unix::fs::symlink(&real, &alias).expect("symlink succeeds");
    // Same tree through two spellings: refused.
    let err = RenderCache::open(&real, &[&alias]).expect_err("symlink alias of approved refused");
    assert!(err.to_string().contains("never a render cache"), "{err}");
    // Cache dir reached THROUGH a symlinked parent, inside the approved tree.
    let parent_link = tmp.path().join("parent-link");
    std::os::unix::fs::symlink(tmp.path(), &parent_link).expect("symlink succeeds");
    let err = RenderCache::open(&parent_link.join("real-approved").join("cache"), &[&real])
        .expect_err("symlinked descendant refused");
    assert!(err.to_string().contains("never a render cache"), "{err}");
    // A symlink that resolves OUTSIDE the approved tree is fine.
    let outside = tmp.path().join("outside");
    std::fs::create_dir(&outside).expect("create outside");
    let cache_link = tmp.path().join("cache-link");
    std::os::unix::fs::symlink(&outside, &cache_link).expect("symlink succeeds");
    RenderCache::open(&cache_link, &[&real]).expect("external symlinked cache opens");
}

#[test]
fn cached_bytes_equal_uncached_renders() {
    let dir = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let mut cache =
        RenderCache::open(dir.path(), &[]).expect("RenderCache::open(dir.path(), &[]) succeeds");
    let rp = placeholder_rp();
    let screen = screen_of(styled_lead()).expect("screen_of succeeds");
    let key = RenderCache::key_for(&screen, &rp);
    // Uncached render straight from the engine.
    let fresh = render_screen(&screen, &rp)
        .expect("render_screen succeeds")
        .png;
    assert!(!fresh.is_empty());
    cache.put(&key, &fresh).expect("put succeeds");
    let hit = cache.get(&key).expect("cache hits");
    assert_eq!(hit, fresh, "cached bytes must equal the uncached render");
    let hit_img = decode(&hit).expect("decode hit");
    let fresh_img = decode(&fresh).expect("decode fresh");
    assert_eq!(hit_img.width(), fresh_img.width());
    assert_eq!(hit_img.height(), fresh_img.height());
}
