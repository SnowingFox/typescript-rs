//! Lazily-allocated link store (`LinkStore`).
//!
//! 1:1 port of Go `internal/core/linkstore.go`. Go stores `map[K]*V` backed by
//! an arena; here a `HashMap<K, V>` owns the values and `get` lazily inserts a
//! default value (returning a mutable reference) to mirror Go's lazy `New()`.

use std::hash::Hash;

use rustc_hash::FxHashMap;

/// A map that lazily creates default values on first access.
///
/// # Examples
/// ```
/// use tsgo_core::linkstore::LinkStore;
/// let mut s: LinkStore<&str, i32> = LinkStore::default();
/// assert!(!s.has(&"a"));
/// *s.get("a") = 5;
/// assert_eq!(s.try_get(&"a"), Some(&5));
/// ```
#[derive(Clone, Debug)]
pub struct LinkStore<K, V> {
    entries: FxHashMap<K, V>,
}

impl<K, V> Default for LinkStore<K, V> {
    fn default() -> Self {
        LinkStore {
            entries: FxHashMap::default(),
        }
    }
}

impl<K: Eq + Hash, V: Default> LinkStore<K, V> {
    /// Returns a mutable reference to the value for `key`, creating a default
    /// value if absent.
    ///
    /// Side effects: mutates `self` when `key` is absent.
    // Go: internal/core/linkstore.go:Get
    pub fn get(&mut self, key: K) -> &mut V {
        self.entries.entry(key).or_default()
    }
}

impl<K: Eq + Hash, V> LinkStore<K, V> {
    /// Reports whether `key` is present.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/linkstore.go:Has
    pub fn has(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// Returns the value for `key` without creating one.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/linkstore.go:TryGet
    pub fn try_get(&self, key: &K) -> Option<&V> {
        self.entries.get(key)
    }
}

#[cfg(test)]
#[path = "linkstore_test.rs"]
mod tests;
