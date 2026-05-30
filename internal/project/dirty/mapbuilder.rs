//! 1:1 port of Go `internal/project/dirty/mapbuilder.go`.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// A builder that overlays inserts and deletes onto an immutable base map,
/// keeping in-progress entries in a separate "builder" form.
///
/// `VBase` is the value type of the finished map; `VBuilder` is the mutable
/// form used while building. [`build`](MapBuilder::build) materializes a fresh
/// base map by applying the overlay: deleted keys are removed and dirty
/// (builder) values are converted back to base values.
///
/// Mirrors Go's `MapBuilder[K, VBase, VBuilder]`.
pub struct MapBuilder<K, VBase, VBuilder> {
    base: HashMap<K, VBase>,
    dirty: HashMap<K, VBuilder>,
    deleted: HashSet<K>,
    // Retained for parity with Go's `NewMapBuilder` (which stores `toBuilder`
    // but does not call it within the package); kept so the public constructor
    // surface matches. Callers convert a base value to a builder value with it.
    #[allow(dead_code)]
    to_builder: std::boxed::Box<dyn Fn(&VBase) -> VBuilder>,
    build_fn: std::boxed::Box<dyn Fn(&VBuilder) -> VBase>,
}

impl<K: Clone + Eq + Hash, VBase: Clone, VBuilder> MapBuilder<K, VBase, VBuilder> {
    /// Creates a builder over `base`, given converters between the base and
    /// builder value forms.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/mapbuilder.go:NewMapBuilder
    pub fn new(
        base: HashMap<K, VBase>,
        to_builder: impl Fn(&VBase) -> VBuilder + 'static,
        build: impl Fn(&VBuilder) -> VBase + 'static,
    ) -> Self {
        MapBuilder {
            base,
            dirty: HashMap::new(),
            deleted: HashSet::new(),
            to_builder: std::boxed::Box::new(to_builder),
            build_fn: std::boxed::Box::new(build),
        }
    }

    /// Sets `key` to a builder value, clearing any pending delete for it.
    ///
    /// Side effects: mutates the overlay.
    // Go: internal/project/dirty/mapbuilder.go:MapBuilder.Set
    pub fn set(&mut self, key: K, value: VBuilder) {
        self.deleted.remove(&key);
        self.dirty.insert(key, value);
    }

    /// Marks `key` deleted, discarding any pending dirty value for it.
    ///
    /// Side effects: mutates the overlay.
    // Go: internal/project/dirty/mapbuilder.go:MapBuilder.Delete
    pub fn delete(&mut self, key: K) {
        self.dirty.remove(&key);
        self.deleted.insert(key);
    }

    /// Clears all entries: drops dirty values and marks every base key deleted.
    ///
    /// Side effects: mutates the overlay.
    // Go: internal/project/dirty/mapbuilder.go:MapBuilder.Clear
    pub fn clear(&mut self) {
        self.dirty.clear();
        self.deleted = self.base.keys().cloned().collect();
    }

    /// Reports whether `key` is present (not deleted, and in the overlay or
    /// base).
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/mapbuilder.go:MapBuilder.Has
    pub fn has(&self, key: &K) -> bool {
        if self.deleted.contains(key) {
            return false;
        }
        if self.dirty.contains_key(key) {
            return true;
        }
        self.base.contains_key(key)
    }

    /// Materializes the final map from base + overlay.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/mapbuilder.go:MapBuilder.Build
    pub fn build(&self) -> HashMap<K, VBase> {
        if self.dirty.is_empty() && self.deleted.is_empty() {
            return self.base.clone();
        }
        let mut result = self.base.clone();
        for key in &self.deleted {
            result.remove(key);
        }
        for (key, value) in &self.dirty {
            result.insert(key.clone(), (self.build_fn)(value));
        }
        result
    }
}

#[cfg(test)]
#[path = "mapbuilder_test.rs"]
mod tests;
