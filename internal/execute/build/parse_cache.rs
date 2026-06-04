//! Port of Go's `internal/execute/build/parseCache.go`.
//!
//! A generic concurrent cache that computes entries on first access. Used by
//! the `--build` host to cache parsed source files and resolved project
//! references.

use std::collections::HashMap;
use std::sync::Mutex;

/// A concurrent parse cache: entries are computed on first access by calling a
/// user-supplied `parse` function, and stored for subsequent lookups.
///
/// Go's implementation uses per-entry mutexes for fine-grained concurrency;
/// this Rust port uses a single `Mutex<HashMap>` (sufficient for the
/// sequential build path; see the `ExtendedConfigCache` note).
///
/// Side effects: none on its own; the `parse` closure may have side effects.
// Go: internal/execute/build/parseCache.go:parseCache
pub struct ParseCache<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    entries: Mutex<HashMap<K, V>>,
}

impl<K, V> Default for ParseCache<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> std::fmt::Debug for ParseCache<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.entries.lock().map(|m| m.len()).unwrap_or(0);
        f.debug_struct("ParseCache")
            .field("entry_count", &count)
            .finish()
    }
}

impl<K, V> ParseCache<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the cached value for `key`, or computes it by calling `parse`
    /// on first access.
    ///
    /// When `allow_zero` is `false` and the cached value equals the type's
    /// default (as determined by `V: Default + PartialEq`), the entry is
    /// re-computed.
    // Go: internal/execute/build/parseCache.go:parseCache.loadOrStore
    pub fn load_or_store(&self, key: K, parse: impl FnOnce(&K) -> V) -> V
    where
        V: Default + PartialEq,
    {
        self.load_or_store_inner(key, parse, false)
    }

    /// Like [`load_or_store`](Self::load_or_store), but allows a "zero" value
    /// to be cached (i.e. does not re-compute when the cached value equals the
    /// default).
    pub fn load_or_store_allowing_zero(&self, key: K, parse: impl FnOnce(&K) -> V) -> V
    where
        V: Default + PartialEq,
    {
        self.load_or_store_inner(key, parse, true)
    }

    fn load_or_store_inner(&self, key: K, parse: impl FnOnce(&K) -> V, allow_zero: bool) -> V
    where
        V: Default + PartialEq,
    {
        let mut map = self.entries.lock().expect("lock poisoned");
        if let Some(existing) = map.get(&key) {
            if allow_zero || *existing != V::default() {
                return existing.clone();
            }
        }
        let value = parse(&key);
        map.insert(key, value.clone());
        value
    }

    /// Inserts a value directly, overwriting any existing entry.
    // Go: internal/execute/build/parseCache.go:parseCache.store
    pub fn store(&self, key: K, value: V) {
        let mut map = self.entries.lock().expect("lock poisoned");
        map.insert(key, value);
    }

    /// Removes the entry for `key`.
    // Go: internal/execute/build/parseCache.go:parseCache.delete
    pub fn delete(&self, key: &K) {
        let mut map = self.entries.lock().expect("lock poisoned");
        map.remove(key);
    }

    /// Clears all entries.
    // Go: internal/execute/build/parseCache.go:parseCache.reset
    pub fn reset(&self) {
        let mut map = self.entries.lock().expect("lock poisoned");
        map.clear();
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cache_is_empty() {
        let cache = ParseCache::<String, i32>::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn load_or_store_computes_on_first_access() {
        let cache = ParseCache::new();
        let result = cache.load_or_store("hello".to_string(), |k| k.len() as i32);
        assert_eq!(result, 5);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn load_or_store_returns_cached_on_second_access() {
        let cache = ParseCache::new();
        cache.load_or_store("hello".to_string(), |k| k.len() as i32);
        let mut called = false;
        let result = cache.load_or_store("hello".to_string(), |_k| {
            called = true;
            999
        });
        assert_eq!(result, 5);
        assert!(!called);
    }

    #[test]
    fn load_or_store_recomputes_zero_value() {
        let cache = ParseCache::new();
        cache.store("key".to_string(), 0i32);
        let result = cache.load_or_store("key".to_string(), |_k| 42);
        assert_eq!(result, 42);
    }

    #[test]
    fn load_or_store_allowing_zero_keeps_zero() {
        let cache = ParseCache::new();
        cache.store("key".to_string(), 0i32);
        let result = cache.load_or_store_allowing_zero("key".to_string(), |_k| 42);
        assert_eq!(result, 0);
    }

    #[test]
    fn store_overwrites() {
        let cache = ParseCache::new();
        cache.store("key".to_string(), 1i32);
        cache.store("key".to_string(), 2);
        let result = cache.load_or_store_allowing_zero("key".to_string(), |_k| 999);
        assert_eq!(result, 2);
    }

    #[test]
    fn delete_removes_entry() {
        let cache = ParseCache::new();
        cache.store("key".to_string(), 42i32);
        assert_eq!(cache.len(), 1);
        cache.delete(&"key".to_string());
        assert!(cache.is_empty());
    }

    #[test]
    fn reset_clears_all() {
        let cache = ParseCache::new();
        cache.store("a".to_string(), 1i32);
        cache.store("b".to_string(), 2);
        assert_eq!(cache.len(), 2);
        cache.reset();
        assert!(cache.is_empty());
    }
}
