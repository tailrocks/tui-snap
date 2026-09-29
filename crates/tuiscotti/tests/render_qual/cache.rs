use super::*;
use tuiscotti::profile::{BlinkPhase, MissingGlyphPolicy, RenderProfile, VENDORED_FALLBACK_FACES};
use tuiscotti::render::{RenderCache, render_cache_disabled, render_screen, screen_content_hash};

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
    tuiscotti::render::set_no_cache_override(true);
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
    tuiscotti::render::set_no_cache_override(false);
    if std::env::var("RENDER_NO_CACHE").is_err() {
        assert!(!render_cache_disabled());
    }
}
