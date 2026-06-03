//! Reference-counted cache.
//!
//! 1:1 port of Go `internal/project/refcountcache.go`.
//!
//! A generic concurrent cache that tracks reference counts per entry.
//! When the reference count reaches zero the entry is removed (unless
//! [`RefCountCacheOptions::disable_deletion`] is set).
//!
//! # Differences from Go
//! - Go uses `collections.SyncMap[K, *refCountCacheEntry[V]]` + per-entry
//!   `sync.Mutex`. Rust uses `DashMap<K, Arc<Mutex<RefCountCacheEntry<V>>>>`.
//! - The `parse` callback takes `&K` (borrowed) instead of `K` (owned) to
//!   avoid cloning the key for the common read-hit path.

use std::hash::Hash;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;

/// Options controlling [`RefCountCache`] behavior.
///
/// # Examples
/// ```
/// use tsgo_project::refcountcache::RefCountCacheOptions;
/// let opts = RefCountCacheOptions { disable_deletion: true };
/// assert!(opts.disable_deletion);
/// ```
// Go: internal/project/refcountcache.go:RefCountCacheOptions
#[derive(Debug, Clone, Default)]
pub struct RefCountCacheOptions {
    /// When `true`, entries are never removed from the cache (useful for testing).
    pub disable_deletion: bool,
}

/// A single cache entry with a value and reference count.
// Go: internal/project/refcountcache.go:refCountCacheEntry
pub(crate) struct RefCountCacheEntry<V> {
    pub(crate) value: Option<V>,
    pub(crate) ref_count: i32,
}

/// The parse-callback type stored inside [`RefCountCache`].
type ParseFn<K, V, A> = Box<dyn Fn(&K, A) -> V + Send + Sync>;

/// A generic concurrent cache with per-entry reference counting.
///
/// `K` is the cache key, `V` is the cached value, and `A` is the type of
/// extra arguments passed to the `parse` callback when creating a new entry.
///
/// # Examples
/// ```
/// use tsgo_project::refcountcache::{RefCountCache, RefCountCacheOptions};
/// let cache = RefCountCache::new(
///     RefCountCacheOptions::default(),
///     |key: &String, _: ()| format!("val-{}", key),
/// );
/// let v = cache.acquire("hello".to_string(), ());
/// assert_eq!(v, "val-hello");
/// assert!(cache.has(&"hello".to_string()));
/// cache.deref(&"hello".to_string());
/// assert!(!cache.has(&"hello".to_string()));
/// ```
// Go: internal/project/refcountcache.go:RefCountCache
pub struct RefCountCache<K, V, A> {
    /// Public options (matches Go `Options` field visibility).
    pub options: RefCountCacheOptions,
    pub(crate) entries: DashMap<K, Arc<Mutex<RefCountCacheEntry<V>>>>,
    parse: ParseFn<K, V, A>,
}

impl<K, V, A> RefCountCache<K, V, A>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Creates a new cache with the given options and parse function.
    ///
    /// The `parse` function is invoked to create a value whenever a cache miss
    /// occurs during [`Self::acquire`].
    // Go: internal/project/refcountcache.go:NewRefCountCache
    pub fn new<F>(options: RefCountCacheOptions, parse: F) -> Self
    where
        F: Fn(&K, A) -> V + Send + Sync + 'static,
    {
        RefCountCache {
            options,
            entries: DashMap::new(),
            parse: Box::new(parse),
        }
    }

    /// Retrieves or creates a cache entry for `identity`.
    ///
    /// If an entry exists, its reference count is incremented and the cached
    /// value is returned. Otherwise `parse()` is called to create the value,
    /// which is stored with reference count 1.
    ///
    /// The caller must call [`Self::deref`] when done with the value.
    // Go: internal/project/refcountcache.go:Acquire
    pub fn acquire(&self, identity: K, acquire_args: A) -> V {
        let (entry, loaded) = self.load_or_store_new_locked_entry(identity.clone());
        let mut guard = entry.lock().unwrap();
        if !loaded {
            let value = (self.parse)(&identity, acquire_args);
            guard.value = Some(value.clone());
            return value;
        }
        guard.value.clone().expect("entry value must be set")
    }

    /// Reports whether an entry exists for `identity`.
    // Go: internal/project/refcountcache.go:Has
    pub fn has(&self, identity: &K) -> bool {
        self.entries.contains_key(identity)
    }

    /// Increments the reference count for an existing entry.
    ///
    /// # Panics
    /// Panics if no entry exists for `identity`.
    // Go: internal/project/refcountcache.go:Ref
    pub fn add_ref(&self, identity: &K) {
        let entry = self
            .entries
            .get(identity)
            .unwrap_or_else(|| panic!("cache entry not found"))
            .value()
            .clone();
        let mut guard = entry.lock().unwrap();
        if guard.ref_count <= 0 && !self.options.disable_deletion {
            drop(guard);
            let (new_entry, _) = self.load_or_store_new_locked_entry(identity.clone());
            let old_guard = entry.lock().unwrap();
            let mut new_guard = new_entry.lock().unwrap();
            new_guard.value.clone_from(&old_guard.value);
            return;
        }
        guard.ref_count += 1;
    }

    /// Decrements the reference count for an entry.
    ///
    /// When the count reaches zero, the entry is removed from the cache
    /// (unless [`RefCountCacheOptions::disable_deletion`] is set).
    // Go: internal/project/refcountcache.go:Deref
    pub fn deref(&self, identity: &K) {
        let entry = match self.entries.get(identity) {
            Some(e) => e.value().clone(),
            None => return,
        };
        let mut guard = entry.lock().unwrap();
        guard.ref_count -= 1;
        if guard.ref_count <= 0 && !self.options.disable_deletion {
            self.entries.remove(identity);
        }
    }

    /// Loads an existing entry or creates a new one with `ref_count = 1`.
    ///
    /// Returns `(arc, loaded)` where `loaded` is `true` if the entry already
    /// existed (and its ref_count was incremented).
    // Go: internal/project/refcountcache.go:loadOrStoreNewLockedEntry
    fn load_or_store_new_locked_entry(&self, key: K) -> (Arc<Mutex<RefCountCacheEntry<V>>>, bool) {
        let new_entry = Arc::new(Mutex::new(RefCountCacheEntry {
            value: None,
            ref_count: 1,
        }));
        match self.entries.entry(key.clone()) {
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                vacant.insert(Arc::clone(&new_entry));
                (new_entry, false)
            }
            dashmap::mapref::entry::Entry::Occupied(occupied) => {
                let existing = occupied.get().clone();
                let mut guard = existing.lock().unwrap();
                if guard.ref_count <= 0 && !self.options.disable_deletion {
                    drop(guard);
                    drop(occupied);
                    self.entries.remove(&key);
                    return self.load_or_store_new_locked_entry(key);
                }
                guard.ref_count += 1;
                drop(guard);
                (existing, true)
            }
        }
    }
}

#[cfg(test)]
#[path = "refcountcache_test.rs"]
mod tests;
