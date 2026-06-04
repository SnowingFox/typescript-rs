//! Port of Go's `internal/execute/tsc/extendedconfigcache.go`.
//!
//! A minimal [`ExtendedConfigCache`] that stores resolved extended-config
//! entries permanently (i.e. no eviction). Suitable for single-compilation
//! runs; for long-lived processes (watch mode), a cache with invalidation
//! should be used instead.

use std::collections::HashMap;
use std::sync::Mutex;

use tsgo_tspath::Path;

/// A simple concurrency-safe cache keyed by canonical [`Path`].
///
/// Entries are computed once and stored permanently.
///
/// Side effects: none on its own; callers compute entries externally and
/// insert them.
// Go: internal/execute/tsc/extendedconfigcache.go:ExtendedConfigCache
pub struct ExtendedConfigCache<V: Clone> {
    entries: Mutex<HashMap<Path, V>>,
}

impl<V: Clone + std::fmt::Debug> std::fmt::Debug for ExtendedConfigCache<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.entries.lock().map(|m| m.len()).unwrap_or(0);
        f.debug_struct("ExtendedConfigCache")
            .field("entry_count", &count)
            .finish()
    }
}

impl<V: Clone> Default for ExtendedConfigCache<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Clone> ExtendedConfigCache<V> {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Retrieves the cached entry for `path`, or inserts `value` if not yet
    /// cached. Returns the (possibly pre-existing) entry.
    // Go: internal/execute/tsc/extendedconfigcache.go:loadOrStoreNewLockedEntry
    pub fn get_or_insert(&self, path: Path, value: V) -> V {
        let mut map = self.entries.lock().expect("lock poisoned");
        map.entry(path).or_insert(value).clone()
    }

    /// Inserts a value, overwriting any existing entry.
    pub fn insert(&self, path: Path, value: V) {
        let mut map = self.entries.lock().expect("lock poisoned");
        map.insert(path, value);
    }

    /// Returns the cached entry for `path`, or `None` if not yet cached.
    pub fn get(&self, path: &Path) -> Option<V> {
        self.entries.lock().ok()?.get(path).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Path {
        Path(s.to_string())
    }

    #[test]
    fn new_cache_is_empty() {
        let cache = ExtendedConfigCache::<String>::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn get_or_insert_stores_first_entry() {
        let cache = ExtendedConfigCache::new();
        let path = p("/configs/base.json");
        let result = cache.get_or_insert(path.clone(), "base.json".to_string());
        assert_eq!(result, "base.json");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn get_or_insert_returns_existing_on_duplicate() {
        let cache = ExtendedConfigCache::new();
        let path = p("/configs/base.json");
        cache.get_or_insert(path.clone(), "first".to_string());
        let result = cache.get_or_insert(path, "second".to_string());
        assert_eq!(result, "first");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn insert_overwrites_existing() {
        let cache = ExtendedConfigCache::new();
        let path = p("/configs/base.json");
        cache.insert(path.clone(), "first".to_string());
        cache.insert(path.clone(), "second".to_string());
        assert_eq!(cache.get(&path), Some("second".to_string()));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn get_returns_none_for_missing() {
        let cache = ExtendedConfigCache::<i32>::new();
        let path = p("/not/cached.json");
        assert!(cache.get(&path).is_none());
    }

    #[test]
    fn get_returns_some_for_existing() {
        let cache = ExtendedConfigCache::new();
        let path = p("/configs/strict.json");
        cache.get_or_insert(path.clone(), 42i32);
        assert_eq!(cache.get(&path), Some(42));
    }
}
