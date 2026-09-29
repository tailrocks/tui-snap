//! Content-addressed render cache (renderer version is part of the key).

use super::RenderError;
use crate::profile::RENDERER_VERSION;
use crate::profile::RenderProfile;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tuiscotti_core::screen::Screen;

// ---------------------------------------------------------------------------
// Content-addressed render cache (V08).
// ---------------------------------------------------------------------------

/// `RENDER_NO_CACHE` set (to anything but `""`/`"0"`) disables the
/// [`RenderCache`]: every `get` misses, every `put` is dropped. Qualification
/// runs set this so no cache entry can mask a renderer change.
#[must_use]
pub fn render_cache_disabled() -> bool {
    NO_CACHE_OVERRIDE.load(std::sync::atomic::Ordering::SeqCst)
        || matches!(std::env::var("RENDER_NO_CACHE"), Ok(v) if v != "0" && !v.is_empty())
}

static NO_CACHE_OVERRIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Test-only no-cache override, OR-ed with `RENDER_NO_CACHE` (no `unsafe`,
/// unlike `set_var`, which is an `unsafe fn` in edition 2024 and cannot be
/// used under the workspace lints). Callers sharing a process must serialize
/// (see the render tests' `CACHE_LOCK`); pass `false` to clear. Never set in
/// production code.
pub fn set_no_cache_override(enabled: bool) {
    NO_CACHE_OVERRIDE.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

/// Deterministic content hash of a screen's approval-relevant state (dims,
/// cells, cursor — no provenance, no origin). One input of the cache key.
#[must_use]
pub fn screen_content_hash(screen: &Screen) -> String {
    let mut h = Sha256::new();
    h.update(b"tuisnap-screen/1\n");
    h.update(screen.cols().to_le_bytes());
    h.update(screen.rows().to_le_bytes());
    for c in screen.cells() {
        h.update(c.x.to_le_bytes());
        h.update(c.y.to_le_bytes());
        h.update(c.symbol.as_bytes());
        h.update([c.width, u8::from(c.continuation)]);
        h.update(format!("{:?}|{:?}|{:?}", c.fg, c.bg, c.mods).as_bytes());
    }
    let cur = screen.cursor();
    h.update(
        format!(
            "{},{},{},{:?},{}",
            cur.x, cur.y, cur.visible, cur.style, cur.blinking
        )
        .as_bytes(),
    );
    let digest = h.finalize();
    crate::hex_bytes(&digest)
}

/// Content-addressed PNG cache. The key covers the screen hash, the profile
/// hash, every pinned face hash (styles + ordered fallbacks), and the
/// renderer version — any input or renderer change is a different key, never
/// a false hit. Entries carry their renderer version up front; corrupt or
/// version-mismatched entries are rejected, REMOVED, and counted
/// ([`RenderCache::rejected`]), never served.
///
/// Approved artifacts are NEVER a render cache: [`RenderCache::open`]
/// refuses a cache dir equal to any approved root, so review evidence can
/// neither be read as cache hits nor overwritten by cache writes.
#[derive(Debug)]
pub struct RenderCache {
    dir: PathBuf,
    rejected: u64,
    hits: u64,
    stores: u64,
}

impl RenderCache {
    /// Open (creating) `dir` as a cache. Fails when `dir` equals any path in
    /// `approved_roots` — approved trees are review evidence, not cache
    /// storage, in either direction.
    ///
    /// # Errors
    ///
    /// Returns `RenderError` when `dir` equals an approved root or cannot be created.
    pub fn open(dir: &Path, approved_roots: &[&Path]) -> Result<Self, RenderError> {
        for root in approved_roots {
            if dir == *root {
                return Err(RenderError(format!(
                    "cache dir {} equals approved root {}: approved artifacts are never a render cache",
                    dir.display(),
                    root.display()
                )));
            }
        }
        std::fs::create_dir_all(dir)
            .map_err(|e| RenderError(format!("cannot create cache dir {}: {e}", dir.display())))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            rejected: 0,
            hits: 0,
            stores: 0,
        })
    }

    /// Cache key for a screen under a strict profile: SHA-256 over the
    /// screen content hash, the profile hash, every pinned face hash
    /// (styles, then fallbacks IN ORDER), and the renderer version.
    #[must_use]
    pub fn key_for(screen: &Screen, rp: &RenderProfile<'_>) -> String {
        Self::key(&screen_content_hash(screen), rp)
    }

    /// [`Self::key_for`] from a precomputed screen hash.
    #[must_use]
    pub fn key(screen_hash: &str, rp: &RenderProfile<'_>) -> String {
        let mut h = Sha256::new();
        h.update(b"tuisnap-render-cache/1\n");
        h.update(screen_hash.as_bytes());
        h.update(b"\n");
        h.update(rp.hash().as_bytes());
        h.update(b"\n");
        for pin in rp.face_hashes() {
            h.update(pin.as_bytes());
            h.update(b"\n");
        }
        for f in rp.fallback_order() {
            h.update(f.sha256.as_bytes());
            h.update(b"\n");
        }
        h.update(RENDERER_VERSION.to_le_bytes());
        let digest = h.finalize();
        crate::hex_bytes(&digest)
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.cache"))
    }

    /// Fetch a cached PNG. Returns `None` (miss) when caching is disabled
    /// ([`render_cache_disabled`]), the key is absent, or the entry is
    /// corrupt/incompatible — the last case also removes the entry and
    /// increments [`Self::rejected`].
    pub fn get(&mut self, key: &str) -> Option<Vec<u8>> {
        if render_cache_disabled() {
            return None;
        }
        let bytes = std::fs::read(self.path(key)).ok()?;
        if Self::valid_entry(&bytes) {
            self.hits += 1;
            Some(bytes[4..].to_vec())
        } else {
            self.rejected += 1;
            let _evicted = std::fs::remove_file(self.path(key));
            None
        }
    }

    /// Store a PNG under `key`. Silently dropped when caching is disabled
    /// (qualification mode). Overwrites any previous entry for the key.
    ///
    /// # Errors
    ///
    /// Returns `RenderError` when the entry cannot be written.
    pub fn put(&mut self, key: &str, png: &[u8]) -> Result<(), RenderError> {
        if render_cache_disabled() {
            return Ok(());
        }
        let mut entry = RENDERER_VERSION.to_le_bytes().to_vec();
        entry.extend_from_slice(png);
        std::fs::write(self.path(key), &entry)
            .map_err(|e| RenderError(format!("cache write failed: {e}")))?;
        self.stores += 1;
        Ok(())
    }

    /// A valid entry: the pinned renderer version up front, then a real PNG.
    fn valid_entry(bytes: &[u8]) -> bool {
        const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
        bytes.len() > 12
            && bytes[0..4] == RENDERER_VERSION.to_le_bytes()
            && bytes[4..12] == *PNG_MAGIC
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
