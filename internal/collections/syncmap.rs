//! Concurrent map (`SyncMap`) backed by `dashmap::DashMap`.
//!
//! 1:1 port of Go `internal/collections/syncmap.go`. Go's `SyncMap[K, any]`
//! tolerates nil values; model nilable value types as `Option<T>` in Rust
//! (where `None` represents Go's nil).

use std::collections::HashMap;
use std::hash::Hash;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;

/// A concurrent hash map.
///
/// # Examples
/// ```
/// use tsgo_collections::SyncMap;
/// let m: SyncMap<&str, i32> = SyncMap::default();
/// m.store("a", 1);
/// assert_eq!(m.load(&"a"), (1, true));
/// ```
pub struct SyncMap<K, V> {
    m: DashMap<K, V>,
}

impl<K: Eq + Hash, V> Default for SyncMap<K, V> {
    fn default() -> Self {
        SyncMap { m: DashMap::new() }
    }
}

impl<K: Eq + Hash + Clone, V: Clone> SyncMap<K, V> {
    /// Stores `value` under `key`.
    ///
    /// Side effects: mutates the shared map.
    // Go: internal/collections/syncmap.go:Store
    pub fn store(&self, key: K, value: V) {
        self.m.insert(key, value);
    }

    /// Loads the existing value, or stores `value` if absent.
    ///
    /// Returns `(actual, loaded)` where `loaded` is true if a value already
    /// existed.
    ///
    /// Side effects: may mutate the shared map.
    // Go: internal/collections/syncmap.go:LoadOrStore
    pub fn load_or_store(&self, key: K, value: V) -> (V, bool) {
        match self.m.entry(key) {
            Entry::Occupied(e) => (e.get().clone(), true),
            Entry::Vacant(e) => {
                let actual = value.clone();
                e.insert(value);
                (actual, false)
            }
        }
    }

    /// Removes `key`.
    ///
    /// Side effects: mutates the shared map.
    // Go: internal/collections/syncmap.go:Delete
    pub fn delete(&self, key: &K) {
        self.m.remove(key);
    }

    /// Removes all entries.
    ///
    /// Side effects: mutates the shared map.
    // Go: internal/collections/syncmap.go:Clear
    pub fn clear(&self) {
        self.m.clear();
    }

    /// Calls `f` for each entry until it returns false.
    ///
    /// Side effects: invokes `f`.
    // Go: internal/collections/syncmap.go:Range
    pub fn range(&self, mut f: impl FnMut(&K, &V) -> bool) {
        for entry in self.m.iter() {
            if !f(entry.key(), entry.value()) {
                break;
            }
        }
    }

    /// Returns the approximate number of entries.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/syncmap.go:Size
    pub fn size(&self) -> usize {
        self.m.len()
    }

    /// Reports whether the map is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.m.is_empty()
    }

    /// Returns a plain snapshot map of the current entries.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/syncmap.go:ToMap
    pub fn to_map(&self) -> HashMap<K, V> {
        let mut m = HashMap::with_capacity(self.m.len());
        for entry in self.m.iter() {
            m.insert(entry.key().clone(), entry.value().clone());
        }
        m
    }

    /// Returns a snapshot of the keys.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/syncmap.go:Keys
    pub fn keys(&self) -> Vec<K> {
        self.m.iter().map(|e| e.key().clone()).collect()
    }
}

// Go: internal/collections/syncmap.go:Clone
impl<K: Eq + Hash + Clone, V: Clone> Clone for SyncMap<K, V> {
    fn clone(&self) -> Self {
        let clone = SyncMap::default();
        for entry in self.m.iter() {
            clone.store(entry.key().clone(), entry.value().clone());
        }
        clone
    }
}

impl<K: Eq + Hash, V: Clone + Default> SyncMap<K, V> {
    /// Loads the value for `key`, returning `(value, ok)`.
    ///
    /// A missing key returns `(V::default(), false)`. For nilable value types
    /// modeled as `Option<T>`, a stored `None` returns `(None, true)`.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/syncmap.go:Load
    pub fn load(&self, key: &K) -> (V, bool) {
        match self.m.get(key) {
            Some(r) => (r.value().clone(), true),
            None => (V::default(), false),
        }
    }
}

#[cfg(test)]
#[path = "syncmap_test.rs"]
mod tests;
