//! Cache caps: entry-count + byte eviction, survivors still agree.

use super::super::*;
use tuiscotti::profile::{MissingGlyphPolicy, RenderProfile};
use tuiscotti::render::{MAX_CACHE_BYTES, MAX_CACHE_ENTRIES, RenderCache};

fn key_for_digit(digit: u32, rp: &RenderProfile<'_>) -> tuiscotti::render::CacheKey {
    let screen = screen_from_leads(4, 2, vec![cell(0, 0, &digit.to_string(), 1)])
        .expect("screen_from_leads succeeds");
    RenderCache::key_for(&screen, rp)
}

#[test]
fn count_cap_evicts_oldest_and_survivors_agree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cache = RenderCache::open(dir.path(), &[]).expect("open");
    cache.set_limits(3, MAX_CACHE_BYTES);
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let png = cache_png().expect("cache_png");
    let mut keys = Vec::new();
    for i in 0..5 {
        let key = key_for_digit(i, &rp);
        cache.put(&key, &png).expect("put succeeds");
        keys.push(key);
    }
    assert_eq!(cache.stores(), 5);
    assert_eq!(cache.evicted(), 2);
    // Exactly 3 survivors, each still decoding to the stored bytes.
    let mut hits = 0;
    for key in &keys {
        if let Some(got) = cache.get(key) {
            assert_eq!(got, png, "survivor must agree");
            hits += 1;
        }
    }
    assert_eq!(hits, 3);
    assert_eq!(cache.hits(), 3);
    let live: Vec<_> = std::fs::read_dir(dir.path())
        .expect("list cache dir")
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.strip_suffix(".cache").is_some())
        })
        .collect();
    assert_eq!(live.len(), 3);
}

#[test]
fn byte_cap_bounds_total_entry_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cache = RenderCache::open(dir.path(), &[]).expect("open");
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let png = cache_png().expect("cache_png");
    // Same PNG under every key: every entry file is the same size.
    let first = key_for_digit(0, &rp);
    cache.put(&first, &png).expect("put succeeds");
    let entry_len = std::fs::metadata(dir.path().join(first.file_name()))
        .expect("stat entry")
        .len();
    cache.set_limits(MAX_CACHE_ENTRIES, entry_len * 2);
    let mut keys = vec![first];
    for i in 1..4 {
        let key = key_for_digit(i, &rp);
        cache.put(&key, &png).expect("put succeeds");
        keys.push(key);
    }
    let total: u64 = std::fs::read_dir(dir.path())
        .expect("list cache dir")
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.strip_suffix(".cache").is_some())
        })
        .filter_map(|e| std::fs::metadata(e.path()).ok())
        .map(|m| m.len())
        .sum();
    assert!(total <= entry_len * 2, "total {total} over cap");
    assert!(cache.evicted() >= 1, "byte pressure must evict");
    for key in &keys {
        if let Some(got) = cache.get(key) {
            assert_eq!(got, png, "survivor must agree");
        }
    }
}

#[test]
fn single_entry_over_byte_cap_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cache = RenderCache::open(dir.path(), &[]).expect("open");
    cache.set_limits(MAX_CACHE_ENTRIES, 64);
    let rp = RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder);
    let png = cache_png().expect("cache_png");
    let key = key_for_digit(0, &rp);
    let err = cache
        .put(&key, &png)
        .expect_err("oversize entry must be refused");
    assert!(err.to_string().contains("byte cap"), "{err}");
    assert_eq!(cache.stores(), 0);
    assert!(!dir.path().join(key.file_name()).exists());
}

#[test]
fn default_caps_are_sane_floors() {
    assert!(MAX_CACHE_ENTRIES >= 16, "count cap must fit a suite");
    assert!(MAX_CACHE_BYTES >= 1024 * 1024, "byte cap must fit renders");
}
