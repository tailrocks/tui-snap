//! Content-addressed render cache with a versioned canonical fingerprint.
//!
//! The cache key is a typed [`CacheKey`]: SHA-256 over a domain-separated,
//! canonically encoded pre-image covering the screen (dims, every cell field
//! including underline color/style, cursor) and the full strict profile
//! (name, geometry, scale, palette, cursor/blink/missing policies, ordered
//! face pins + fallback chain, renderer version). No `Debug` formatting
//! participates: every field has an explicit byte encoding, so a key can
//! neither collide across a rendering-relevant change nor drift with a
//! `Debug` impl.
//!
//! Entries bind their key and payload checksum in a versioned header and are
//! validated by FULL bounded PNG decode on read (magic + length is never
//! enough). Publication is atomic (exclusive temp file + rename); a crash
//! leaves at most an orphaned temp file, never a half entry under a live
//! name.
//!
//! Approved artifacts are NEVER a render cache: [`RenderCache::open`]
//! refuses a cache dir that equals, contains, or is contained in any
//! approved root, comparing canonicalized paths so symlinks/aliases cannot
//! smuggle one tree inside the other.

use super::RenderError;
use crate::profile::RenderProfile;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tuiscotti_core::frame::{Color, CursorStyle, Mods, Rgb, UnderlineStyle};
use tuiscotti_core::screen::Screen;

// ---------------------------------------------------------------------------
// No-cache override (qualification mode).
// ---------------------------------------------------------------------------

/// `RENDER_NO_CACHE` set (to anything but `""`/`"0"`) disables the
/// [`RenderCache`]: every `get` misses, every `put` is dropped. Qualification
/// runs set this so no cache entry can mask a renderer change.
#[must_use]
pub fn render_cache_disabled() -> bool {
    NO_CACHE_OVERRIDE.load(Ordering::SeqCst)
        || matches!(std::env::var("RENDER_NO_CACHE"), Ok(v) if v != "0" && !v.is_empty())
}

static NO_CACHE_OVERRIDE: AtomicBool = AtomicBool::new(false);

/// Test-only no-cache override, OR-ed with `RENDER_NO_CACHE` (no `unsafe`,
/// unlike `set_var`, which is an `unsafe fn` in edition 2024 and cannot be
/// used under the workspace lints). Callers sharing a process must serialize
/// (see the render tests' `CACHE_LOCK`); pass `false` to clear. Never set in
/// production code.
pub fn set_no_cache_override(enabled: bool) {
    NO_CACHE_OVERRIDE.store(enabled, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// Canonical field encodings (explicit bytes, never `Debug`).
// ---------------------------------------------------------------------------

/// Fingerprint format version. Bumped whenever the pre-image changes (v2:
/// underline color/style included, all `Debug`-fed fields canonically
/// encoded). Old `.cache` files carry the v1 header and are rejected by
/// [`RenderCache::get`], never served.
pub const CACHE_FINGERPRINT_VERSION: u32 = 2;

fn update_len_prefixed(h: &mut Sha256, bytes: &[u8]) {
    h.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
    h.update(bytes);
}

fn update_color(h: &mut Sha256, color: Color) {
    match color {
        Color::Default => h.update([0x00]),
        Color::Indexed(i) => h.update([0x01, i]),
        Color::Rgb(Rgb { r, g, b }) => h.update([0x02, r, g, b]),
    }
}

fn update_mods(h: &mut Sha256, mods: Mods) {
    let mut flags: u16 = 0;
    flags |= u16::from(mods.hidden);
    flags |= u16::from(mods.blink) << 1;
    flags |= u16::from(mods.bold) << 2;
    flags |= u16::from(mods.dim) << 3;
    flags |= u16::from(mods.italic) << 4;
    flags |= u16::from(mods.underline) << 5;
    flags |= u16::from(mods.strikethrough) << 6;
    flags |= u16::from(mods.reverse) << 7;
    let style: u16 = match mods.underline_style {
        UnderlineStyle::None => 0,
        UnderlineStyle::Single => 1,
        UnderlineStyle::Double => 2,
        UnderlineStyle::Curly => 3,
        UnderlineStyle::Dotted => 4,
        UnderlineStyle::Dashed => 5,
    };
    h.update((flags | (style << 8)).to_le_bytes());
}

fn cursor_style_byte(style: CursorStyle) -> u8 {
    match style {
        CursorStyle::Block => 0,
        CursorStyle::Underline => 1,
        CursorStyle::Bar => 2,
    }
}

/// Deterministic content hash of a screen's approval-relevant state (dims,
/// cells incl. underline color/style, cursor — no provenance, no origin: the
/// screen→frame adaptation drops both, so they cannot affect pixels). One
/// input of the cache key.
#[must_use]
pub fn screen_content_hash(screen: &Screen) -> String {
    let mut h = Sha256::new();
    h.update(b"tuiscotti-screen/2\n");
    h.update(screen.cols().to_le_bytes());
    h.update(screen.rows().to_le_bytes());
    for c in screen.cells() {
        h.update(c.x.to_le_bytes());
        h.update(c.y.to_le_bytes());
        update_len_prefixed(&mut h, c.symbol.as_bytes());
        h.update([c.width, u8::from(c.continuation)]);
        update_color(&mut h, c.fg);
        update_color(&mut h, c.bg);
        update_mods(&mut h, c.mods);
        update_color(&mut h, c.underline_color);
    }
    let cur = screen.cursor();
    h.update(cur.x.to_le_bytes());
    h.update(cur.y.to_le_bytes());
    h.update([u8::from(cur.visible), cursor_style_byte(cur.style)]);
    h.update([u8::from(cur.blinking)]);
    let digest = h.finalize();
    crate::hex_bytes(&digest)
}

fn update_profile(h: &mut Sha256, rp: &RenderProfile<'_>) {
    use crate::profile::{BlinkPhase, CursorPolicy, IndexedPalette, MissingGlyphPolicy};
    h.update(b"tuiscotti-render-profile-fingerprint/2\n");
    update_len_prefixed(h, rp.name().as_bytes());
    for pin in rp.face_hashes() {
        update_len_prefixed(h, pin.as_bytes());
    }
    // Ordered fallback chain: pins stand in for the bytes (strict
    // construction verified bytes-against-pin, and the renderer re-verifies
    // at render time), but ORDER and identity both participate.
    let order = rp.fallback_order();
    h.update(u32::try_from(order.len()).unwrap_or(u32::MAX).to_le_bytes());
    for f in order {
        update_len_prefixed(h, f.sha256.as_bytes());
        update_len_prefixed(h, f.desc.as_bytes());
    }
    h.update(rp.font_px().to_bits().to_le_bytes());
    h.update(rp.cell_w().to_le_bytes());
    h.update(rp.cell_h().to_le_bytes());
    h.update(rp.pad().to_le_bytes());
    h.update(rp.scale().to_le_bytes());
    let p = rp.palette();
    h.update([p.default_fg.r, p.default_fg.g, p.default_fg.b]);
    h.update([p.default_bg.r, p.default_bg.g, p.default_bg.b]);
    let indexed: u8 = match p.indexed {
        IndexedPalette::Xterm => 0,
    };
    let cursor: u8 = match rp.cursor() {
        CursorPolicy::Show => 0,
        CursorPolicy::Hide => 1,
    };
    let blink: u8 = match rp.blink_phase() {
        BlinkPhase::On => 0,
        BlinkPhase::Off => 1,
    };
    let missing: u8 = match rp.missing() {
        MissingGlyphPolicy::Strict => 0,
        MissingGlyphPolicy::Placeholder => 1,
    };
    h.update([indexed, cursor, blink, missing]);
    h.update(rp.renderer_version().to_le_bytes());
}

// ---------------------------------------------------------------------------
// Typed cache keys.
// ---------------------------------------------------------------------------

/// Validated content-addressed cache key: 64 lowercase hex chars (32 raw
/// bytes). The ONLY way to name a cache entry — raw strings never reach path
/// joins, so traversal is impossible by construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    hex: String,
    raw: [u8; 32],
}

impl CacheKey {
    /// Canonical key for a screen under a strict profile: SHA-256 over the
    /// screen content hash and the full canonical profile encoding above,
    /// domain-separated and fingerprinted at
    /// [`CACHE_FINGERPRINT_VERSION`].
    #[must_use]
    pub fn for_screen(screen: &Screen, rp: &RenderProfile<'_>) -> Self {
        let mut h = Sha256::new();
        h.update(b"tuiscotti-render-cache/2\n");
        h.update(CACHE_FINGERPRINT_VERSION.to_le_bytes());
        h.update(screen_content_hash(screen).as_bytes());
        h.update(b"\n");
        update_profile(&mut h, rp);
        let digest = h.finalize();
        let mut raw = [0u8; 32];
        raw.copy_from_slice(&digest);
        Self {
            hex: crate::hex_bytes(&digest),
            raw,
        }
    }

    /// Parse an externally supplied key (directory scan, test fixture).
    /// Strict: exactly 64 lowercase hex chars, nothing else.
    ///
    /// # Errors
    ///
    /// Returns `RenderError` when `s` is not 64 lowercase hex chars.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
            return Err(RenderError(format!(
                "invalid cache key: expected 64 lowercase hex chars, got {s:?}"
            )));
        }
        let mut raw = [0u8; 32];
        let (chunks, _) = s.as_bytes().as_chunks::<2>();
        for (i, chunk) in chunks.iter().enumerate() {
            raw[i] = (hex_val(chunk[0]) << 4) | hex_val(chunk[1]);
        }
        Ok(Self {
            hex: s.to_string(),
            raw,
        })
    }

    /// Lowercase hex form (the entry file stem).
    #[must_use]
    pub fn hex(&self) -> &str {
        &self.hex
    }

    /// Raw key bytes (bound into the entry header).
    #[must_use]
    pub fn raw(&self) -> &[u8; 32] {
        &self.raw
    }

    /// Entry file name: `<hex>.cache`. Safe to join onto the cache dir: the
    /// stem is validated hex, so it cannot escape.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.cache", self.hex)
    }
}

impl std::fmt::Display for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.hex)
    }
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Entry format + bounded full-decode validation.
// ---------------------------------------------------------------------------

/// Entry header magic (v2 format; v1 entries are rejected outright).
const ENTRY_MAGIC: &[u8; 9] = b"TSCACHE02";
/// `magic(9) + key(32) + png_len u64le(8) + sha256(32)`.
const HEADER_LEN: usize = 9 + 32 + 8 + 32;

/// Largest PNG side the cache will decode (pixels). Real renders are
/// `cols*cell_w*scale`-sized (tens of megapixels at most); anything larger
/// is a corrupt or hostile entry, rejected BEFORE the full decode allocates.
const MAX_PNG_DIM: u32 = 16_384;
/// Largest PNG pixel count the cache will decode.
const MAX_PNG_PIXELS: u64 = 1 << 26;

const PNG_SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Full bounded PNG validation: IHDR dims are bounds-checked BEFORE the
/// decode allocates, then the whole image is decoded and the decoded dims
/// must match the header. Magic + length alone never validates.
fn fully_valid_png(png: &[u8]) -> bool {
    if png.len() < 33 || png[0..8] != PNG_SIG || png[12..16] != *b"IHDR" {
        return false;
    }
    let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    if w == 0 || h == 0 || w > MAX_PNG_DIM || h > MAX_PNG_DIM {
        return false;
    }
    if u64::from(w) * u64::from(h) > MAX_PNG_PIXELS {
        return false;
    }
    match image::load_from_memory(png) {
        Ok(img) => {
            use image::GenericImageView;
            img.dimensions() == (w, h)
        }
        Err(_) => false,
    }
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn encode_entry(key: &CacheKey, png: &[u8]) -> Vec<u8> {
    let mut entry = Vec::with_capacity(HEADER_LEN + png.len());
    entry.extend_from_slice(ENTRY_MAGIC);
    entry.extend_from_slice(key.raw());
    entry.extend_from_slice(&u64::try_from(png.len()).unwrap_or(u64::MAX).to_le_bytes());
    entry.extend_from_slice(&sha256_bytes(png));
    entry.extend_from_slice(png);
    entry
}

/// Decode one entry, bound to the expected key. `None` on ANY defect:
/// bad magic, key mismatch (wrong-entry payload), length/checksum mismatch,
/// truncation, or a PNG that does not fully decode within bounds.
fn decode_entry(key: &CacheKey, bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < HEADER_LEN || bytes[0..9] != *ENTRY_MAGIC {
        return None;
    }
    if bytes[9..41] != *key.raw() {
        return None;
    }
    let len = usize::try_from(u64::from_le_bytes(bytes[41..49].try_into().ok()?)).ok()?;
    if bytes.len() != HEADER_LEN + len {
        return None;
    }
    let png = &bytes[HEADER_LEN..];
    if bytes[49..81] != sha256_bytes(png) {
        return None;
    }
    if !fully_valid_png(png) {
        return None;
    }
    Some(png.to_vec())
}

// ---------------------------------------------------------------------------
// The cache.
// ---------------------------------------------------------------------------

/// Content-addressed PNG cache keyed by [`CacheKey`].
///
/// Entries carry their key and payload checksum up front; corrupt,
/// truncated, key-mismatched, or version-mismatched entries are rejected,
/// REMOVED, and counted ([`RenderCache::rejected`]), never served.
///
/// Approved artifacts are NEVER a render cache: [`RenderCache::open`]
/// refuses a cache dir that equals, contains, or is contained in any
/// approved root (canonicalized, so aliases/symlinks cannot smuggle one
/// tree inside the other), so review evidence can neither be read as cache
/// hits nor overwritten by cache writes.
#[derive(Debug)]
pub struct RenderCache {
    dir: PathBuf,
    rejected: u64,
    hits: u64,
    stores: u64,
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl RenderCache {
    /// Open (creating) `dir` as a cache. Fails when `dir` equals, contains,
    /// or is contained in any path in `approved_roots` — approved trees are
    /// review evidence, not cache storage, in either direction. Paths are
    /// canonicalized before comparison, so symlink/alias/descendant escapes
    /// are refused, not just lexical equality.
    ///
    /// # Errors
    ///
    /// Returns `RenderError` when `dir` overlaps an approved root or cannot
    /// be created/resolved.
    pub fn open(dir: &Path, approved_roots: &[&Path]) -> Result<Self, RenderError> {
        std::fs::create_dir_all(dir)
            .map_err(|e| RenderError(format!("cannot create cache dir {}: {e}", dir.display())))?;
        let dir = std::fs::canonicalize(dir)
            .map_err(|e| RenderError(format!("cannot resolve cache dir {}: {e}", dir.display())))?;
        for root in approved_roots {
            let canon = resolved_against_cwd(root);
            if dir == canon || dir.starts_with(&canon) || canon.starts_with(&dir) {
                return Err(RenderError(format!(
                    "cache dir {} overlaps approved root {}: approved artifacts are never a render cache",
                    dir.display(),
                    root.display()
                )));
            }
        }
        Ok(Self {
            dir,
            rejected: 0,
            hits: 0,
            stores: 0,
        })
    }

    /// Cache key for a screen under a strict profile ([`CacheKey::for_screen`]).
    #[must_use]
    pub fn key_for(screen: &Screen, rp: &RenderProfile<'_>) -> CacheKey {
        CacheKey::for_screen(screen, rp)
    }

    fn path(&self, key: &CacheKey) -> PathBuf {
        self.dir.join(key.file_name())
    }

    /// The entry path stays inside the cache dir (keys are validated hex, so
    /// this always holds; checked anyway so a future key change fails
    /// closed instead of escaping).
    fn contained(&self, path: &Path) -> bool {
        path.parent() == Some(self.dir.as_path())
    }

    /// Fetch a cached PNG. Returns `None` (miss) when caching is disabled
    /// ([`render_cache_disabled`]), the key is absent, or the entry is
    /// corrupt/incompatible — the last case also removes the entry and
    /// increments [`Self::rejected`]. Served bytes are always a fully
    /// decode-validated PNG for THIS key.
    pub fn get(&mut self, key: &CacheKey) -> Option<Vec<u8>> {
        if render_cache_disabled() {
            return None;
        }
        let path = self.path(key);
        if !self.contained(&path) {
            return None;
        }
        let bytes = std::fs::read(&path).ok()?;
        if let Some(png) = decode_entry(key, &bytes) {
            self.hits += 1;
            Some(png)
        } else {
            self.rejected += 1;
            // `remove_file` on a planted symlink removes the link, never the target.
            let _evicted = std::fs::remove_file(&path);
            None
        }
    }

    /// Store a PNG under `key`, published atomically (exclusive temp file +
    /// rename): readers never see a half entry, and an interrupted write
    /// leaves at most an orphaned temp file. Silently dropped when caching
    /// is disabled (qualification mode). Overwrites any previous entry for
    /// the key. The payload must fully decode within bounds — undecodable
    /// bytes are refused, never stored.
    ///
    /// # Errors
    ///
    /// Returns `RenderError` when the payload is not a valid bounded PNG or
    /// the entry cannot be published.
    pub fn put(&mut self, key: &CacheKey, png: &[u8]) -> Result<(), RenderError> {
        if render_cache_disabled() {
            return Ok(());
        }
        if !fully_valid_png(png) {
            return Err(RenderError(
                "refusing to cache bytes that do not fully decode as a bounded PNG".to_string(),
            ));
        }
        let path = self.path(key);
        if !self.contained(&path) {
            return Err(RenderError("cache key escapes the cache dir".to_string()));
        }
        let entry = encode_entry(key, png);
        let tmp = self.dir.join(format!(
            ".{}.tmp-{}-{}",
            key.hex(),
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let write = (|| -> std::io::Result<()> {
            use std::io::Write;
            // Exclusive create: never follow a planted symlink, never
            // truncate a live name; the rename below replaces the ENTRY
            // name itself, never a symlink target.
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            f.write_all(&entry)?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&tmp, &path)?;
            Ok(())
        })();
        if let Err(e) = write {
            let _orphan = std::fs::remove_file(&tmp);
            return Err(RenderError(format!("cache write failed: {e}")));
        }
        self.stores += 1;
        Ok(())
    }

    /// Entries rejected as corrupt or version-incompatible (and removed).
    #[must_use]
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// Successful cache reads.
    #[must_use]
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// Successful cache writes.
    #[must_use]
    pub fn stores(&self) -> u64 {
        self.stores
    }
}

/// Resolve a path for containment comparison: canonicalize when it exists
/// (symlinks/aliases resolved); otherwise canonicalize the nearest existing
/// ancestor and rejoin the remainder (so `/var/...` vs `/private/var/...`
/// style aliases still compare correctly), falling back to a lexical clean
/// when nothing exists.
fn resolved_against_cwd(path: &Path) -> PathBuf {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return canon;
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
    };
    // Clean FIRST so `..` segments are gone before the ancestor walk
    // (`Path::file_name` returns `None` for a trailing `..`).
    let abs = lexical_clean(&abs);
    let mut existing = abs.as_path();
    let mut stash: Vec<&std::ffi::OsStr> = Vec::new();
    while !existing.exists() {
        match existing.file_name() {
            Some(name) => {
                stash.push(name);
                match existing.parent() {
                    Some(parent) => existing = parent,
                    None => return lexical_clean(&abs),
                }
            }
            None => return lexical_clean(&abs),
        }
    }
    let mut canon = std::fs::canonicalize(existing).unwrap_or_else(|_| existing.to_path_buf());
    for name in stash.into_iter().rev() {
        canon.push(name);
    }
    lexical_clean(&canon)
}

/// Lexical `.`/`..` normalization (no filesystem access).
fn lexical_clean(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}
