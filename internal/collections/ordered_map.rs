//! Insertion-ordered map (`OrderedMap`) backed by `indexmap::IndexMap`, plus
//! order-preserving JSON (de)serialization and map diffing.
//!
//! 1:1 port of Go `internal/collections/ordered_map.go`. Go maintains
//! `keys []K` + `mp map[K]V` by hand; here `IndexMap` provides insertion order
//! and O(1) index access. Deletion uses `shift_remove` to preserve order
//! (never `swap_remove`).

use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;

use indexmap::IndexMap;
use rustc_hash::FxBuildHasher;
use serde::de::{Deserialize, Deserializer, Error as DeError, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

type FxIndexMap<K, V> = IndexMap<K, V, FxBuildHasher>;

/// An insertion-ordered map.
///
/// # Examples
/// ```
/// use tsgo_collections::OrderedMap;
/// let mut m: OrderedMap<i32, &str> = OrderedMap::default();
/// m.set(1, "one");
/// m.set(2, "two");
/// assert_eq!(m.size(), 2);
/// assert_eq!(m.keys().copied().collect::<Vec<_>>(), vec![1, 2]);
/// ```
#[derive(Clone, Debug)]
pub struct OrderedMap<K, V>(FxIndexMap<K, V>);

impl<K, V> Default for OrderedMap<K, V> {
    fn default() -> Self {
        OrderedMap(IndexMap::default())
    }
}

/// A key-value pair used to build an [`OrderedMap`] from a list.
#[derive(Clone, Debug)]
pub struct MapEntry<K, V> {
    /// The entry key.
    pub key: K,
    /// The entry value.
    pub value: V,
}

impl<K: Eq + Hash, V> OrderedMap<K, V> {
    /// Creates an empty map with capacity for at least `hint` entries.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_map.go:NewOrderedMapWithSizeHint
    pub fn with_size_hint(hint: usize) -> Self {
        OrderedMap(IndexMap::with_capacity_and_hasher(hint, FxBuildHasher))
    }

    /// Builds a map from a list of entries, preserving order.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_map.go:NewOrderedMapFromList
    pub fn from_list(items: Vec<MapEntry<K, V>>) -> Self {
        let mut m = Self::with_size_hint(items.len());
        for item in items {
            m.set(item.key, item.value);
        }
        m
    }

    /// Sets a key-value pair, keeping the position of an existing key.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/ordered_map.go:Set
    pub fn set(&mut self, key: K, value: V) {
        self.0.insert(key, value);
    }

    /// Returns the value for `key`, if present.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_map.go:Get
    pub fn get(&self, key: &K) -> Option<&V> {
        self.0.get(key)
    }

    /// Returns the value for `key`, or the value type's default if absent.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_map.go:GetOrZero
    pub fn get_or_zero(&self, key: &K) -> V
    where
        V: Clone + Default,
    {
        self.0.get(key).cloned().unwrap_or_default()
    }

    /// Returns the key-value pair at insertion index `index`, if any.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_map.go:EntryAt
    pub fn entry_at(&self, index: usize) -> Option<(&K, &V)> {
        self.0.get_index(index)
    }

    /// Reports whether `key` is present.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_map.go:Has
    pub fn has(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    /// Removes `key`, returning its value if present (preserving order).
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/ordered_map.go:Delete
    pub fn delete(&mut self, key: &K) -> Option<V> {
        self.0.shift_remove(key)
    }
}

impl<K, V> OrderedMap<K, V> {
    /// Returns an iterator over the keys in insertion order.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_map.go:Keys
    pub fn keys(&self) -> impl Iterator<Item = &K> + '_ {
        self.0.keys()
    }

    /// Returns an iterator over the values in insertion order.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_map.go:Values
    pub fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.0.values()
    }

    /// Returns an iterator over the key-value pairs in insertion order.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_map.go:Entries
    pub fn entries(&self) -> impl Iterator<Item = (&K, &V)> + '_ {
        self.0.iter()
    }

    /// Removes all entries, keeping allocated capacity.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/ordered_map.go:Clear
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Returns the number of entries.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/ordered_map.go:Size
    pub fn size(&self) -> usize {
        self.0.len()
    }

    /// Reports whether the map is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// Go: internal/collections/ordered_map.go:MarshalJSONTo
impl<K: Serialize, V: Serialize> Serialize for OrderedMap<K, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

struct OrderedMapVisitor<K, V>(PhantomData<(K, V)>);

impl<'de, K, V> Visitor<'de> for OrderedMapVisitor<K, V>
where
    K: Deserialize<'de> + Eq + Hash,
    V: Deserialize<'de>,
{
    type Value = OrderedMap<K, V>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON object")
    }

    // Go convention: null is a no-op; here it yields an empty map.
    fn visit_unit<E: DeError>(self) -> Result<Self::Value, E> {
        Ok(OrderedMap::default())
    }

    fn visit_none<E: DeError>(self) -> Result<Self::Value, E> {
        Ok(OrderedMap::default())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut m = OrderedMap::with_size_hint(access.size_hint().unwrap_or(0));
        while let Some((k, v)) = access.next_entry()? {
            m.set(k, v);
        }
        Ok(m)
    }

    fn visit_str<E: DeError>(self, _v: &str) -> Result<Self::Value, E> {
        Err(E::custom("cannot unmarshal non-object JSON value into Map"))
    }

    fn visit_string<E: DeError>(self, _v: String) -> Result<Self::Value, E> {
        Err(E::custom("cannot unmarshal non-object JSON value into Map"))
    }

    fn visit_bool<E: DeError>(self, _v: bool) -> Result<Self::Value, E> {
        Err(E::custom("cannot unmarshal non-object JSON value into Map"))
    }

    fn visit_i64<E: DeError>(self, _v: i64) -> Result<Self::Value, E> {
        Err(E::custom("cannot unmarshal non-object JSON value into Map"))
    }

    fn visit_u64<E: DeError>(self, _v: u64) -> Result<Self::Value, E> {
        Err(E::custom("cannot unmarshal non-object JSON value into Map"))
    }

    fn visit_f64<E: DeError>(self, _v: f64) -> Result<Self::Value, E> {
        Err(E::custom("cannot unmarshal non-object JSON value into Map"))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, _seq: A) -> Result<Self::Value, A::Error> {
        Err(A::Error::custom(
            "cannot unmarshal non-object JSON value into Map",
        ))
    }
}

// Go: internal/collections/ordered_map.go:UnmarshalJSONFrom
impl<'de, K, V> Deserialize<'de> for OrderedMap<K, V>
where
    K: Deserialize<'de> + Eq + Hash,
    V: Deserialize<'de>,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(OrderedMapVisitor(PhantomData))
    }
}

/// Diffs two ordered maps by value equality, invoking the callbacks for added,
/// removed, and modified entries.
///
/// Side effects: invokes the supplied callbacks.
// Go: internal/collections/ordered_map.go:DiffOrderedMaps
pub fn diff_ordered_maps<K, V>(
    m1: &OrderedMap<K, V>,
    m2: &OrderedMap<K, V>,
    on_added: impl FnMut(&K, &V),
    on_removed: impl FnMut(&K, &V),
    on_modified: impl FnMut(&K, &V, &V),
) where
    K: Eq + Hash,
    V: PartialEq,
{
    diff_ordered_maps_func(m1, m2, |a, b| a == b, on_added, on_removed, on_modified);
}

/// Like [`diff_ordered_maps`], but with a custom value-equality predicate.
///
/// Side effects: invokes the supplied callbacks.
// Go: internal/collections/ordered_map.go:DiffOrderedMapsFunc
pub fn diff_ordered_maps_func<K, V>(
    m1: &OrderedMap<K, V>,
    m2: &OrderedMap<K, V>,
    mut equal_values: impl FnMut(&V, &V) -> bool,
    mut on_added: impl FnMut(&K, &V),
    mut on_removed: impl FnMut(&K, &V),
    mut on_modified: impl FnMut(&K, &V, &V),
) where
    K: Eq + Hash,
{
    for (k, v2) in m2.entries() {
        if m1.get(k).is_none() {
            on_added(k, v2);
        }
    }
    for (k, v1) in m1.entries() {
        if let Some(v2) = m2.get(k) {
            if !equal_values(v1, v2) {
                on_modified(k, v1, v2);
            }
        } else {
            on_removed(k, v1);
        }
    }
}

#[cfg(test)]
#[path = "ordered_map_test.rs"]
mod tests;
