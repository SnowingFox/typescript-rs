// Go: internal/project/refcountcache.go (unit tests — Go integration tests
// in refcountcache_test.go require the full Session stack and are
// DEFER(phase-8-session): blocked-by Session/Snapshot/ConfigFileRegistry)
use super::*;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

fn make_cache() -> RefCountCache<String, String, ()> {
    RefCountCache::new(RefCountCacheOptions::default(), |key: &String, _: ()| {
        format!("parsed-{}", key)
    })
}

fn make_cache_no_delete() -> RefCountCache<String, String, ()> {
    RefCountCache::new(
        RefCountCacheOptions {
            disable_deletion: true,
        },
        |key: &String, _: ()| format!("parsed-{}", key),
    )
}

// -- Acquire ---------------------------------------------------------

#[test]
fn acquire_creates_entry_with_refcount_1() {
    // Go: internal/project/refcountcache.go:Acquire (new entry path)
    let cache = make_cache();
    let val = cache.acquire("foo".to_string(), ());
    assert_eq!(val, "parsed-foo");
    assert!(cache.has(&"foo".to_string()));
}

#[test]
fn acquire_returns_cached_value_for_same_key() {
    // Go: internal/project/refcountcache.go:Acquire (existing entry path)
    let parse_count = Arc::new(AtomicI32::new(0));
    let pc = Arc::clone(&parse_count);
    let cache = RefCountCache::new(
        RefCountCacheOptions::default(),
        move |key: &String, _: ()| {
            pc.fetch_add(1, Ordering::SeqCst);
            format!("parsed-{}", key)
        },
    );
    let v1 = cache.acquire("foo".to_string(), ());
    let v2 = cache.acquire("foo".to_string(), ());
    assert_eq!(v1, v2);
    assert_eq!(
        parse_count.load(Ordering::SeqCst),
        1,
        "parse called only once"
    );
}

#[test]
fn acquire_different_keys_are_independent() {
    let cache = make_cache();
    let a = cache.acquire("a".to_string(), ());
    let b = cache.acquire("b".to_string(), ());
    assert_eq!(a, "parsed-a");
    assert_eq!(b, "parsed-b");
    assert!(cache.has(&"a".to_string()));
    assert!(cache.has(&"b".to_string()));
}

// -- Has -------------------------------------------------------------

#[test]
fn has_returns_false_for_missing_key() {
    // Go: internal/project/refcountcache.go:Has
    let cache = make_cache();
    assert!(!cache.has(&"missing".to_string()));
}

#[test]
fn has_returns_true_after_acquire() {
    let cache = make_cache();
    cache.acquire("x".to_string(), ());
    assert!(cache.has(&"x".to_string()));
}

// -- Deref -----------------------------------------------------------

#[test]
fn deref_removes_entry_at_zero_refcount() {
    // Go: internal/project/refcountcache.go:Deref (removal path)
    let cache = make_cache();
    cache.acquire("k".to_string(), ());
    assert!(cache.has(&"k".to_string()));
    cache.deref(&"k".to_string());
    assert!(!cache.has(&"k".to_string()));
}

#[test]
fn deref_noop_for_missing_key() {
    // Go: internal/project/refcountcache.go:Deref (missing key early return)
    let cache = make_cache();
    cache.deref(&"nonexistent".to_string());
}

#[test]
fn deref_does_not_remove_with_remaining_refs() {
    // Go: internal/project/refcountcache.go:Deref (refCount > 0 path)
    let cache = make_cache();
    cache.acquire("k".to_string(), ());
    cache.add_ref(&"k".to_string());
    cache.deref(&"k".to_string());
    assert!(
        cache.has(&"k".to_string()),
        "entry should remain with refCount 1"
    );
    cache.deref(&"k".to_string());
    assert!(
        !cache.has(&"k".to_string()),
        "entry should be removed at refCount 0"
    );
}

// -- Ref (add_ref) ---------------------------------------------------

#[test]
#[should_panic(expected = "cache entry not found")]
fn ref_panics_for_missing_key() {
    // Go: internal/project/refcountcache.go:Ref (panic path)
    let cache = make_cache();
    cache.add_ref(&"missing".to_string());
}

#[test]
fn ref_increments_refcount() {
    // Go: internal/project/refcountcache.go:Ref (happy path)
    let cache = make_cache();
    cache.acquire("k".to_string(), ());
    cache.add_ref(&"k".to_string());
    // Now refCount should be 2 — need two derefs to remove
    cache.deref(&"k".to_string());
    assert!(
        cache.has(&"k".to_string()),
        "entry should survive first deref"
    );
    cache.deref(&"k".to_string());
    assert!(
        !cache.has(&"k".to_string()),
        "entry should be removed after second deref"
    );
}

// -- DisableDeletion -------------------------------------------------

#[test]
fn disable_deletion_prevents_removal() {
    // Go: internal/project/refcountcache.go:Deref with DisableDeletion
    let cache = make_cache_no_delete();
    cache.acquire("k".to_string(), ());
    cache.deref(&"k".to_string());
    assert!(
        cache.has(&"k".to_string()),
        "entry should survive deref when deletion is disabled"
    );
}

// -- Acquire after Deref (re-parse) ----------------------------------

#[test]
fn acquire_after_deref_reparses() {
    let parse_count = Arc::new(AtomicI32::new(0));
    let pc = Arc::clone(&parse_count);
    let cache = RefCountCache::new(
        RefCountCacheOptions::default(),
        move |key: &String, _: ()| {
            pc.fetch_add(1, Ordering::SeqCst);
            format!("parsed-{}", key)
        },
    );
    cache.acquire("k".to_string(), ());
    cache.deref(&"k".to_string());
    assert!(!cache.has(&"k".to_string()));
    let v = cache.acquire("k".to_string(), ());
    assert_eq!(v, "parsed-k");
    assert_eq!(
        parse_count.load(Ordering::SeqCst),
        2,
        "should have parsed twice"
    );
}

// -- Concurrent access -----------------------------------------------

#[test]
fn concurrent_acquire_same_key() {
    let cache = Arc::new(make_cache());
    let mut handles = Vec::new();
    for _ in 0..10 {
        let c = Arc::clone(&cache);
        handles.push(std::thread::spawn(move || {
            c.acquire("shared".to_string(), ())
        }));
    }
    for h in handles {
        let val = h.join().unwrap();
        assert_eq!(val, "parsed-shared");
    }
}
