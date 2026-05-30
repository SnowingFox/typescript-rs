//! 1:1 port of Go `internal/project/dirty/cloneablemap.go`.

use std::collections::HashMap;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

use crate::interfaces::Cloneable;

/// A plain map that satisfies [`Cloneable`] by shallow-cloning its entries.
///
/// This mirrors Go's `CloneableMap[K, V] map[K]V` whose `Clone` is
/// `maps.Clone`. It exists so an ordinary map can be stored as the value of a
/// dirty container (which requires [`Cloneable`]). The clone copies the entries
/// of the map, not the values they may point to.
///
/// It derefs to the underlying [`HashMap`], so it can be indexed and iterated
/// like a normal map.
///
/// # Examples
/// ```
/// use tsgo_project_dirty::{Cloneable, CloneableMap};
/// let mut m = CloneableMap::new();
/// m.insert("a", 1);
/// let mut c = m.clone_cow();
/// c.insert("b", 2);
/// assert_eq!(m.len(), 1);
/// assert_eq!(c.len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct CloneableMap<K, V>(HashMap<K, V>);

impl<K, V> CloneableMap<K, V> {
    /// Creates an empty map.
    ///
    /// Side effects: none (pure).
    pub fn new() -> Self {
        CloneableMap(HashMap::new())
    }
}

impl<K, V> Default for CloneableMap<K, V> {
    fn default() -> Self {
        CloneableMap(HashMap::new())
    }
}

impl<K, V> From<HashMap<K, V>> for CloneableMap<K, V> {
    fn from(map: HashMap<K, V>) -> Self {
        CloneableMap(map)
    }
}

impl<K, V> Deref for CloneableMap<K, V> {
    type Target = HashMap<K, V>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K, V> DerefMut for CloneableMap<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<K: Clone + Eq + Hash, V: Clone> Cloneable for CloneableMap<K, V> {
    // Go: internal/project/dirty/cloneablemap.go:CloneableMap.Clone
    fn clone_cow(&self) -> Self {
        CloneableMap(self.0.clone())
    }
}

#[cfg(test)]
#[path = "cloneablemap_test.rs"]
mod tests;
