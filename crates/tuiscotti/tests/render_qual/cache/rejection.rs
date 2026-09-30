//! Entry rejection: truncation, key binding, containment, byte equality.

use super::super::*;
use super::helpers::{placeholder_rp, screen_of, styled_lead};
use tuiscotti::render::{CacheKey, RenderCache, render_screen};

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
