//! Cache behavior: round trip, rejection counting, no-cache mode.

use super::super::*;
use tuiscotti::profile::{BlinkPhase, MissingGlyphPolicy, RenderProfile, VENDORED_FALLBACK_FACES};
use tuiscotti::render::{CacheOptions, RenderCache, render_screen, screen_content_hash};

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
