//! One-to-many map (`MultiMap`) backed by `FxHashMap<K, Vec<V>>`.
//!
//! 1:1 port of Go `internal/collections/multimap.go`.

use std::hash::Hash;

use rustc_hash::FxHashMap;

/// A map from keys to lists of values.
///
/// # Examples
/// ```
/// use tsgo_collections::MultiMap;
/// let mut m: MultiMap<&str, i32> = MultiMap::default();
/// m.add("k", 1);
/// m.add("k", 2);
/// assert_eq!(m.get(&"k"), &[1, 2]);
/// ```
#[derive(Clone, Debug)]
pub struct MultiMap<K, V>(FxHashMap<K, Vec<V>>);

impl<K, V> Default for MultiMap<K, V> {
    fn default() -> Self {
        MultiMap(FxHashMap::default())
    }
}

impl<K: Eq + Hash, V: PartialEq> MultiMap<K, V> {
    /// Creates an empty multimap with capacity for at least `hint` keys.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/multimap.go:NewMultiMapWithSizeHint
    pub fn with_size_hint(hint: usize) -> Self {
        MultiMap(FxHashMap::with_capacity_and_hasher(
            hint,
            Default::default(),
        ))
    }

    /// Reports whether `key` has any values.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/multimap.go:Has
    pub fn has(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    /// Returns the values for `key` (empty slice if absent).
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/multimap.go:Get
    pub fn get(&self, key: &K) -> &[V] {
        self.0.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Appends `value` to the list for `key`.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/multimap.go:Add
    pub fn add(&mut self, key: K, value: V) {
        self.0.entry(key).or_default().push(value);
    }

    /// Removes a single `value` from the list for `key` (preserving order);
    /// removes the key entirely if it becomes empty.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/multimap.go:Remove
    pub fn remove(&mut self, key: &K, value: &V) {
        if let Some(values) = self.0.get_mut(key) {
            if let Some(i) = values.iter().position(|v| v == value) {
                if values.len() == 1 {
                    self.0.remove(key);
                } else {
                    values.remove(i);
                }
            }
        }
    }

    /// Removes all values for `key`.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/multimap.go:RemoveAll
    pub fn remove_all(&mut self, key: &K) {
        self.0.remove(key);
    }
}

impl<K, V> MultiMap<K, V> {
    /// Returns the number of keys.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/multimap.go:Len
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Reports whether the multimap is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns an iterator over the keys (unordered).
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/multimap.go:Keys
    pub fn keys(&self) -> impl Iterator<Item = &K> + '_ {
        self.0.keys()
    }

    /// Returns an iterator over the value lists (unordered).
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/multimap.go:Values
    pub fn values(&self) -> impl Iterator<Item = &Vec<V>> + '_ {
        self.0.values()
    }

    /// Removes all keys and values.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/multimap.go:Clear
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// Groups `items` into a multimap keyed by `group_id(item)`.
///
/// # Examples
/// ```
/// use tsgo_collections::group_by;
/// let m = group_by(vec![1, 2, 3, 4], |v| v % 2);
/// assert_eq!(m.get(&0), &[2, 4]);
/// assert_eq!(m.get(&1), &[1, 3]);
/// ```
///
/// Side effects: none (pure).
// Go: internal/collections/multimap.go:GroupBy
pub fn group_by<K: Eq + Hash, V: PartialEq + Clone>(
    items: impl IntoIterator<Item = V>,
    mut group_id: impl FnMut(&V) -> K,
) -> MultiMap<K, V> {
    let mut m = MultiMap::default();
    for item in items {
        let key = group_id(&item);
        m.add(key, item);
    }
    m
}

#[cfg(test)]
#[path = "multimap_test.rs"]
mod tests;
