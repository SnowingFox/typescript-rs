//! Unordered set (`Set`) backed by `rustc_hash::FxHashSet`, with set algebra.
//!
//! 1:1 port of Go `internal/collections/set.go`. Go's nil-receiver semantics
//! are represented here with an ordinary empty set (Rust has no nil).

use std::hash::Hash;

use rustc_hash::FxHashSet;

/// An unordered hash set.
///
/// # Examples
/// ```
/// use tsgo_collections::Set;
/// let mut s: Set<&str> = Set::default();
/// s.add("a");
/// assert!(s.has(&"a"));
/// assert_eq!(s.len(), 1);
/// ```
#[derive(Clone, Debug)]
pub struct Set<T>(FxHashSet<T>);

impl<T> Default for Set<T> {
    fn default() -> Self {
        Set(FxHashSet::default())
    }
}

impl<T: Eq + Hash> Set<T> {
    /// Creates an empty set with capacity for at least `hint` elements.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/set.go:NewSetWithSizeHint
    pub fn with_size_hint(hint: usize) -> Self {
        Set(FxHashSet::with_capacity_and_hasher(
            hint,
            Default::default(),
        ))
    }

    /// Reports whether `key` is present.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/set.go:Has
    pub fn has(&self, key: &T) -> bool {
        self.0.contains(key)
    }

    /// Adds `key` to the set.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/set.go:Add
    pub fn add(&mut self, key: T) {
        self.0.insert(key);
    }

    /// Removes `key` from the set.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/set.go:Delete
    pub fn delete(&mut self, key: &T) {
        self.0.remove(key);
    }

    /// Adds `key` if absent; returns whether it was newly inserted.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/set.go:AddIfAbsent
    pub fn add_if_absent(&mut self, key: T) -> bool {
        self.0.insert(key)
    }

    /// Merges `other` into this set (in place).
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/set.go:Union
    pub fn union(&mut self, other: &Set<T>)
    where
        T: Clone,
    {
        for k in &other.0 {
            self.0.insert(k.clone());
        }
    }

    /// Returns a new set that is the union of this set and `other`.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/set.go:UnionedWith
    pub fn unioned_with(&self, other: &Set<T>) -> Set<T>
    where
        T: Clone,
    {
        let mut result = self.clone();
        for k in &other.0 {
            result.0.insert(k.clone());
        }
        result
    }

    /// Reports whether this set equals `other`.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/set.go:Equals
    pub fn equals(&self, other: &Set<T>) -> bool {
        self.0 == other.0
    }

    /// Reports whether every element of this set is in `other`.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/set.go:IsSubsetOf
    pub fn is_subset_of(&self, other: &Set<T>) -> bool {
        self.0.iter().all(|k| other.0.contains(k))
    }

    /// Reports whether this set shares any element with `other`.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/set.go:Intersects
    pub fn intersects(&self, other: &Set<T>) -> bool {
        self.0.iter().any(|k| other.0.contains(k))
    }

    /// Builds a set from an iterator of items (deduplicating).
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/set.go:NewSetFromItems
    pub fn from_items(items: impl IntoIterator<Item = T>) -> Set<T> {
        let mut s = Set::default();
        for item in items {
            s.add(item);
        }
        s
    }
}

impl<T> Set<T> {
    /// Returns the number of elements.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/set.go:Len
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Reports whether the set is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns an iterator over the elements (unordered).
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/set.go:Keys
    pub fn keys(&self) -> impl Iterator<Item = &T> + '_ {
        self.0.iter()
    }

    /// Removes all elements.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/set.go:Clear
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
#[path = "set_test.rs"]
mod tests;
