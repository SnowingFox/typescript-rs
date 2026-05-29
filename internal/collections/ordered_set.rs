//! Insertion-ordered set (`OrderedSet`) backed by `indexmap::IndexSet`.
//!
//! 1:1 port of Go `internal/collections/ordered_set.go`. Deletion uses
//! `shift_remove` to preserve insertion order.

use std::hash::Hash;

use indexmap::IndexSet;
use rustc_hash::FxBuildHasher;

type FxIndexSet<T> = IndexSet<T, FxBuildHasher>;

/// An insertion-ordered set.
///
/// # Examples
/// ```
/// use tsgo_collections::OrderedSet;
/// let mut s: OrderedSet<i32> = OrderedSet::default();
/// s.add(1);
/// s.add(2);
/// assert!(s.has(&1));
/// assert_eq!(s.values().copied().collect::<Vec<_>>(), vec![1, 2]);
/// ```
#[derive(Clone, Debug)]
pub struct OrderedSet<T>(FxIndexSet<T>);

impl<T> Default for OrderedSet<T> {
    fn default() -> Self {
        OrderedSet(IndexSet::default())
    }
}

impl<T: Eq + Hash> OrderedSet<T> {
    /// Creates an empty set with capacity for at least `hint` elements.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_set.go:NewOrderedSetWithSizeHint
    pub fn with_size_hint(hint: usize) -> Self {
        OrderedSet(IndexSet::with_capacity_and_hasher(hint, FxBuildHasher))
    }

    /// Adds `value` to the set.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/ordered_set.go:Add
    pub fn add(&mut self, value: T) {
        self.0.insert(value);
    }

    /// Reports whether `value` is present.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_set.go:Has
    pub fn has(&self, value: &T) -> bool {
        self.0.contains(value)
    }

    /// Removes `value`, returning whether it was present (preserving order).
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/ordered_set.go:Delete
    pub fn delete(&mut self, value: &T) -> bool {
        self.0.shift_remove(value)
    }
}

impl<T> OrderedSet<T> {
    /// Returns an iterator over the values in insertion order.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_set.go:Values
    pub fn values(&self) -> impl Iterator<Item = &T> + '_ {
        self.0.iter()
    }

    /// Removes all elements, keeping allocated capacity.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/ordered_set.go:Clear
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Returns the number of elements.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_set.go:Size
    pub fn size(&self) -> usize {
        self.0.len()
    }

    /// Reports whether the set is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
#[path = "ordered_set_test.rs"]
mod tests;
