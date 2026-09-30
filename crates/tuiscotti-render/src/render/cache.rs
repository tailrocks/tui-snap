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
use entry::{decode_entry, encode_entry, fully_valid_png};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tuiscotti_core::screen::Screen;

mod entry;
mod key;

pub use key::{CACHE_FINGERPRINT_VERSION, CacheKey, screen_content_hash};

// ---------------------------------------------------------------------------
// No-cache mode (qualification): explicit context + process config.
// ---------------------------------------------------------------------------

/// `RENDER_NO_CACHE` set (to anything but `""`/`"0"`) disables every
/// [`RenderCache`]: every `get` misses, every `put` is dropped. Qualification
/// runs set this in the invoking shell so no cache entry can mask a renderer
/// change. This reads process configuration only — there is no in-process
/// global override (F12): tests select no-cache mode per cache through
/// [`CacheOptions`], so independent tests never serialize on shared state.
#[must_use]
pub fn render_cache_disabled() -> bool {
    matches!(std::env::var("RENDER_NO_CACHE"), Ok(v) if v != "0" && !v.is_empty())
}

/// Explicit per-cache options (F12): no-cache mode travels with the cache
/// handle, never through process-global state.
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheOptions {
    /// When true, this cache behaves as if caching were disabled: every
    /// `get` misses, every `put` is dropped (renders still work). OR-ed
    /// with [`render_cache_disabled`].
    pub no_cache: bool,
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
    no_cache: bool,
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
        Self::open_with_options(dir, approved_roots, CacheOptions::default())
    }

    /// [`RenderCache::open`] with explicit [`CacheOptions`]: `no_cache`
    /// disables this cache instance (qualification mode per cache, no
    /// process-global state), OR-ed with [`render_cache_disabled`].
    ///
    /// # Errors
    ///
    /// Returns `RenderError` when `dir` overlaps an approved root or cannot
    /// be created/resolved.
    pub fn open_with_options(
        dir: &Path,
        approved_roots: &[&Path],
        options: CacheOptions,
    ) -> Result<Self, RenderError> {
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
            no_cache: options.no_cache,
            rejected: 0,
            hits: 0,
            stores: 0,
        })
    }

    /// True when this cache instance was opened with no-cache mode.
    #[must_use]
    pub fn is_no_cache(&self) -> bool {
        self.no_cache
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
    /// (this instance's [`CacheOptions::no_cache`] or
    /// [`render_cache_disabled`]), the key is absent, or the entry is
    /// corrupt/incompatible — the last case also removes the entry and
    /// increments [`Self::rejected`]. Served bytes are always a fully
    /// decode-validated PNG for THIS key.
    pub fn get(&mut self, key: &CacheKey) -> Option<Vec<u8>> {
        if self.no_cache || render_cache_disabled() {
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
        if self.no_cache || render_cache_disabled() {
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
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
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
