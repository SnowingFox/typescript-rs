//! `tsgo_project_dirty` — 1:1 Rust port of Go `internal/project/dirty`.
//!
//! A copy-on-write / dirty-tracking layer over immutable base maps and values.
//! Reads prefer a dirty overlay and fall back to the base; the first write to a
//! base-derived value clones it (via [`Cloneable::clone_cow`]) before mutating,
//! so the base is never disturbed. Finalizing merges the base and the overlay
//! into a fresh map (reporting whether anything changed).
//!
//! This crate hosts the port of `map.go` (the [`Map`] / [`MapEntry`] types)
//! directly in the crate root, with the remaining Go files mounted as sibling
//! modules.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use crate::entry::MapEntryData;

#[path = "interfaces.rs"]
mod interfaces;

#[path = "entry.rs"]
mod entry;

#[path = "box.rs"]
mod boxed;

#[path = "cloneablemap.rs"]
mod cloneablemap;

#[path = "util.rs"]
mod util;

#[path = "mapbuilder.rs"]
mod mapbuilder;

#[path = "syncmap.rs"]
mod syncmap;

pub use boxed::Box;
pub use cloneablemap::CloneableMap;
pub use interfaces::{Cloneable, Value};
pub use mapbuilder::MapBuilder;
pub use syncmap::{FinalizationHooks, SyncMap, SyncMapEntry};
pub use util::clone_map_if_nil;

type EntryCell<K, V> = Rc<RefCell<MapEntryData<K, V>>>;
type DirtyEntries<K, V> = HashMap<K, EntryCell<K, V>>;

/// A single-threaded copy-on-write map over an immutable base map.
///
/// Reads consult the dirty overlay first and fall back to the base. Entries
/// obtained from the base are clean; mutating one clones the value before the
/// change and registers the entry in the overlay. [`finalize`](Map::finalize)
/// merges base + overlay into a fresh map.
pub struct Map<K, V> {
    base: HashMap<K, V>,
    dirty: Rc<RefCell<DirtyEntries<K, V>>>,
}

/// A handle to one key of a [`Map`], usable to read and mutate that entry.
///
/// An entry shares its state with the map's overlay once it becomes dirty, so
/// mutations are visible through the map and across handles to the same dirty
/// key.
pub struct MapEntry<K, V> {
    dirty: Rc<RefCell<DirtyEntries<K, V>>>,
    data: EntryCell<K, V>,
}

impl<K: Clone + Eq + Hash, V: Cloneable + Default> Map<K, V> {
    /// Creates a map over `base`.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/map.go:NewMap
    pub fn new(base: HashMap<K, V>) -> Self {
        Map {
            base,
            dirty: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Returns the entry for `key`, or `None` if absent or deleted.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/map.go:Map.Get
    pub fn get(&self, key: &K) -> Option<MapEntry<K, V>> {
        {
            let dirty = self.dirty.borrow();
            if let Some(data) = dirty.get(key) {
                if data.borrow().delete {
                    return None;
                }
                return Some(MapEntry {
                    dirty: self.dirty.clone(),
                    data: data.clone(),
                });
            }
        }
        let value = self.base.get(key)?;
        let data = Rc::new(RefCell::new(MapEntryData {
            key: key.clone(),
            original: value.clone_cow(),
            value: value.clone_cow(),
            dirty: false,
            delete: false,
        }));
        Some(MapEntry {
            dirty: self.dirty.clone(),
            data,
        })
    }

    /// Adds a fresh, already-dirty entry without consulting the base map.
    ///
    /// Side effects: inserts an entry into the overlay.
    // Go: internal/project/dirty/map.go:Map.Add
    pub fn add(&self, key: K, value: V) {
        let data = Rc::new(RefCell::new(MapEntryData {
            key: key.clone(),
            original: V::default(),
            value,
            dirty: true,
            delete: false,
        }));
        self.dirty.borrow_mut().insert(key, data);
    }

    /// Changes the entry for `key`, panicking if it does not exist.
    ///
    /// Side effects: mutates the entry and marks it dirty.
    // Go: internal/project/dirty/map.go:Map.Change
    pub fn change<F: FnMut(&mut V)>(&self, key: &K, mut apply: F) {
        match self.get(key) {
            Some(entry) => entry.change_inner(&mut apply),
            None => panic!("tried to change a non-existent entry"),
        }
    }

    /// Deletes the entry for `key` if present, returning whether it existed.
    ///
    /// Side effects: marks the entry deleted when present.
    // Go: internal/project/dirty/map.go:Map.TryDelete
    pub fn try_delete(&self, key: &K) -> bool {
        match self.get(key) {
            Some(entry) => {
                entry.delete();
                true
            }
            None => false,
        }
    }

    /// Deletes the entry for `key`, panicking if it does not exist.
    ///
    /// Side effects: marks the entry deleted.
    // Go: internal/project/dirty/map.go:Map.Delete
    pub fn delete(&self, key: &K) {
        if !self.try_delete(key) {
            panic!("tried to delete a non-existent entry");
        }
    }

    /// Calls `f` for each live entry until it returns false.
    ///
    /// Side effects: invokes `f`.
    // Go: internal/project/dirty/map.go:Map.Range
    pub fn range<F: FnMut(&MapEntry<K, V>) -> bool>(&self, mut f: F) {
        let mut seen_in_dirty: HashSet<K> = HashSet::new();
        // Snapshot the overlay so `f` may mutate the map without aliasing the
        // dirty borrow.
        let dirty_entries: Vec<(K, EntryCell<K, V>)> = self
            .dirty
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (key, data) in dirty_entries {
            seen_in_dirty.insert(key);
            let is_delete = data.borrow().delete;
            if !is_delete {
                let entry = MapEntry {
                    dirty: self.dirty.clone(),
                    data,
                };
                if !f(&entry) {
                    break;
                }
            }
        }
        for (key, value) in self.base.iter() {
            if seen_in_dirty.contains(key) {
                continue; // already processed in dirty entries
            }
            let data = Rc::new(RefCell::new(MapEntryData {
                key: key.clone(),
                original: value.clone_cow(),
                value: value.clone_cow(),
                dirty: false,
                delete: false,
            }));
            let entry = MapEntry {
                dirty: self.dirty.clone(),
                data,
            };
            if !f(&entry) {
                break;
            }
        }
    }

    /// Clears both the overlay and the base map.
    ///
    /// Side effects: empties the overlay and the base.
    // Go: internal/project/dirty/map.go:Map.Clear
    pub fn clear(&mut self) {
        self.dirty.borrow_mut().clear();
        self.base.clear();
    }

    /// Merges the base and overlay into a fresh map and reports whether the
    /// result differs from the base.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/map.go:Map.Finalize
    pub fn finalize(&self) -> (HashMap<K, V>, bool) {
        let dirty = self.dirty.borrow();
        if dirty.is_empty() {
            return (self.clone_base(), false);
        }
        let mut result = self.clone_base();
        for (key, data) in dirty.iter() {
            let entry = data.borrow();
            if entry.delete {
                result.remove(key);
            } else {
                result.insert(key.clone(), entry.value.clone_cow());
            }
        }
        (result, true)
    }

    fn clone_base(&self) -> HashMap<K, V> {
        self.base
            .iter()
            .map(|(k, v)| (k.clone(), v.clone_cow()))
            .collect()
    }
}

impl<K: Clone + Eq + Hash, V: Cloneable + Default> MapEntry<K, V> {
    /// Returns the entry's key.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/entry.go:mapEntry.Key
    pub fn key(&self) -> K {
        self.data.borrow().key.clone()
    }

    /// Returns the original (loaded) value of the entry.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/entry.go:mapEntry.Original
    pub fn original(&self) -> V {
        self.data.borrow().original.clone_cow()
    }

    /// Returns the current value, or `V::default()` when deleted.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/entry.go:mapEntry.Value
    pub fn value(&self) -> V {
        self.data.borrow().value()
    }

    /// Reports whether the entry has been changed.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/entry.go:mapEntry.Dirty
    pub fn dirty(&self) -> bool {
        self.data.borrow().dirty
    }

    /// Mutates the value, cloning on first change and registering the entry.
    ///
    /// Panics if the entry is already deleted.
    ///
    /// Side effects: mutates the value, marks it dirty, registers the overlay.
    // Go: internal/project/dirty/map.go:MapEntry.Change
    pub fn change<F: FnMut(&mut V)>(&self, mut apply: F) {
        self.change_inner(&mut apply);
    }

    /// Replaces the value outright, registering the entry on first change.
    ///
    /// Panics if the entry is already deleted.
    ///
    /// Side effects: replaces the value, marks it dirty, registers the overlay.
    // Go: internal/project/dirty/map.go:MapEntry.Replace
    pub fn replace(&self, new_value: V) {
        let mut data = self.data.borrow_mut();
        if data.delete {
            panic!("tried to change a deleted entry");
        }
        if !data.dirty {
            data.dirty = true;
            self.dirty
                .borrow_mut()
                .insert(data.key.clone(), self.data.clone());
        }
        data.value = new_value;
    }

    /// Mutates the value only when `cond` holds for the current value.
    ///
    /// Side effects: may mutate the value and mark it dirty.
    // Go: internal/project/dirty/map.go:MapEntry.ChangeIf
    pub fn change_if<C: FnMut(&V) -> bool, F: FnMut(&mut V)>(
        &self,
        mut cond: C,
        mut apply: F,
    ) -> bool {
        self.change_if_inner(&mut cond, &mut apply)
    }

    /// Marks the entry for deletion, registering it in the overlay.
    ///
    /// Side effects: marks the entry deleted, registers the overlay.
    // Go: internal/project/dirty/map.go:MapEntry.Delete
    pub fn delete(&self) {
        let mut data = self.data.borrow_mut();
        if !data.dirty {
            self.dirty
                .borrow_mut()
                .insert(data.key.clone(), self.data.clone());
        }
        data.delete = true;
    }

    /// Invokes `f` with this entry viewed as a [`Value`].
    ///
    /// Side effects: invokes `f`.
    // Go: internal/project/dirty/map.go:MapEntry.Locked
    pub fn locked<F: FnMut(&dyn Value<V>)>(&self, mut f: F) {
        f(self);
    }

    fn change_inner(&self, apply: &mut dyn FnMut(&mut V)) {
        let mut data = self.data.borrow_mut();
        if data.delete {
            panic!("tried to change a deleted entry");
        }
        if !data.dirty {
            // PERF(port): Go clones `value` here (it aliased the base value);
            // this port already cloned at `get`/`add`, so only flag + register.
            data.dirty = true;
            self.dirty
                .borrow_mut()
                .insert(data.key.clone(), self.data.clone());
        }
        apply(&mut data.value);
    }

    fn change_if_inner(
        &self,
        cond: &mut dyn FnMut(&V) -> bool,
        apply: &mut dyn FnMut(&mut V),
    ) -> bool {
        let current = self.data.borrow().value();
        if cond(&current) {
            self.change_inner(apply);
            return true;
        }
        false
    }
}

impl<K: Clone + Eq + Hash, V: Cloneable + Default> Value<V> for MapEntry<K, V> {
    fn value(&self) -> V {
        MapEntry::value(self)
    }

    fn original(&self) -> V {
        MapEntry::original(self)
    }

    fn dirty(&self) -> bool {
        MapEntry::dirty(self)
    }

    fn change(&self, apply: &mut dyn FnMut(&mut V)) {
        self.change_inner(apply);
    }

    fn change_if(&self, cond: &mut dyn FnMut(&V) -> bool, apply: &mut dyn FnMut(&mut V)) -> bool {
        self.change_if_inner(cond, apply)
    }

    fn delete(&self) {
        MapEntry::delete(self);
    }

    fn locked(&self, f: &mut dyn FnMut(&dyn Value<V>)) {
        f(self);
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
