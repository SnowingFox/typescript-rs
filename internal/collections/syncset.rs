//! Concurrent set (`SyncSet`) built on [`SyncMap`].
//!
//! 1:1 port of Go `internal/collections/syncset.go`.

use std::hash::Hash;

use crate::syncmap::SyncMap;

/// A concurrent hash set.
///
/// # Examples
/// ```
/// use tsgo_collections::SyncSet;
/// let s: SyncSet<&str> = SyncSet::default();
/// assert!(s.add_if_absent("a"));
/// assert!(!s.add_if_absent("a"));
/// assert!(s.has(&"a"));
/// ```
pub struct SyncSet<T> {
    m: SyncMap<T, ()>,
}

impl<T: Eq + Hash> Default for SyncSet<T> {
    fn default() -> Self {
        SyncSet {
            m: SyncMap::default(),
        }
    }
}

impl<T: Eq + Hash + Clone> SyncSet<T> {
    /// Reports whether `key` is present.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/syncset.go:Has
    pub fn has(&self, key: &T) -> bool {
        self.m.load(key).1
    }

    /// Adds `key` to the set.
    ///
    /// Side effects: mutates the shared set.
    // Go: internal/collections/syncset.go:Add
    pub fn add(&self, key: T) {
        self.add_if_absent(key);
    }

    /// Adds `key` if absent; returns true if it was newly inserted.
    ///
    /// Side effects: may mutate the shared set.
    // Go: internal/collections/syncset.go:AddIfAbsent
    pub fn add_if_absent(&self, key: T) -> bool {
        let (_, loaded) = self.m.load_or_store(key, ());
        !loaded
    }

    /// Removes `key`.
    ///
    /// Side effects: mutates the shared set.
    // Go: internal/collections/syncset.go:Delete
    pub fn delete(&self, key: &T) {
        self.m.delete(key);
    }

    /// Calls `f` for each element until it returns false.
    ///
    /// Side effects: invokes `f`.
    // Go: internal/collections/syncset.go:Range
    pub fn range(&self, mut f: impl FnMut(&T) -> bool) {
        self.m.range(|key, _| f(key));
    }

    /// Returns the approximate number of elements.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/syncset.go:Size
    pub fn size(&self) -> usize {
        self.m.size()
    }

    /// Reports whether the set is empty.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/syncset.go:IsEmpty
    pub fn is_empty(&self) -> bool {
        self.m.is_empty()
    }

    /// Returns a snapshot slice of the elements.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/syncset.go:ToSlice
    pub fn to_slice(&self) -> Vec<T> {
        self.m.keys()
    }

    /// Returns a snapshot of the elements.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/syncset.go:Keys
    pub fn keys(&self) -> Vec<T> {
        self.m.keys()
    }
}

#[cfg(test)]
#[path = "syncset_test.rs"]
mod tests;
