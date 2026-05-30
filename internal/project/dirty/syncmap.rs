//! 1:1 port of Go `internal/project/dirty/syncmap.go`.
//!
//! A concurrent copy-on-write map. The novel piece is `proxy_for`: when two
//! threads each load a fresh entry for the same base key and then race to dirty
//! it, the loser of the `LoadOrStore` race becomes a proxy that forwards all
//! operations to the winner (the entry actually stored in the overlay), so both
//! handles observe one consistent value.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::{Arc, Mutex, Weak};

use tsgo_collections::SyncMap as CollSyncMap;

use crate::entry::MapEntryData;
use crate::interfaces::{Cloneable, Value};

struct SyncMapShared<K, V> {
    base: HashMap<K, V>,
    dirty: CollSyncMap<K, Option<Arc<SyncMapEntry<K, V>>>>,
}

/// A concurrent copy-on-write map over an immutable base map.
///
/// Mirrors Go's `SyncMap[K, V]`. Reads consult the concurrent overlay first and
/// fall back to the base; mutating a base-derived entry clones the value before
/// the change and routes concurrent racing handles through a single winner.
pub struct SyncMap<K, V> {
    shared: Arc<SyncMapShared<K, V>>,
}

struct SyncEntryState<K, V> {
    data: MapEntryData<K, V>,
    proxy_for: Option<Arc<SyncMapEntry<K, V>>>,
}

/// A handle to one key of a [`SyncMap`].
///
/// Mirrors Go's `*SyncMapEntry[K, V]`. Concurrent handles to the same key are
/// kept consistent through the `proxy_for` mechanism described on [`SyncMap`].
pub struct SyncMapEntry<K, V> {
    m: Weak<SyncMapShared<K, V>>,
    me: Weak<SyncMapEntry<K, V>>,
    state: Mutex<SyncEntryState<K, V>>,
}

/// Callbacks invoked by [`SyncMap::finalize_with`] as the overlay is applied.
///
/// Mirrors Go's `FinalizationHooks[K, V]`. Each hook is optional; `on_change`
/// fires for keys already present in the base, `on_add` for keys that are not.
pub struct FinalizationHooks<'a, K, V> {
    /// Called for each deleted key with its last value.
    #[allow(clippy::type_complexity)]
    pub on_delete: Option<std::boxed::Box<dyn Fn(&K, &V) + 'a>>,
    /// Called for each changed key already present in the base, with its
    /// original and new value.
    #[allow(clippy::type_complexity)]
    pub on_change: Option<std::boxed::Box<dyn Fn(&K, &V, &V) + 'a>>,
    /// Called for each newly added key with its value.
    #[allow(clippy::type_complexity)]
    pub on_add: Option<std::boxed::Box<dyn Fn(&K, &V) + 'a>>,
}

impl<K, V> Default for FinalizationHooks<'_, K, V> {
    fn default() -> Self {
        FinalizationHooks {
            on_delete: None,
            on_change: None,
            on_add: None,
        }
    }
}

fn new_entry<K, V>(
    shared: &Arc<SyncMapShared<K, V>>,
    key: K,
    original: V,
    value: V,
    dirty: bool,
    delete: bool,
) -> Arc<SyncMapEntry<K, V>> {
    Arc::new_cyclic(|me| SyncMapEntry {
        m: Arc::downgrade(shared),
        me: me.clone(),
        state: Mutex::new(SyncEntryState {
            data: MapEntryData {
                key,
                original,
                value,
                dirty,
                delete,
            },
            proxy_for: None,
        }),
    })
}

impl<K: Clone + Eq + Hash, V: Cloneable + Default> SyncMap<K, V> {
    /// Creates a concurrent map over `base`.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/syncmap.go:NewSyncMap
    pub fn new(base: HashMap<K, V>) -> Self {
        SyncMap {
            shared: Arc::new(SyncMapShared {
                base,
                dirty: CollSyncMap::default(),
            }),
        }
    }

    /// Loads the entry for `key`, or `None` if absent or deleted.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/syncmap.go:SyncMap.Load
    pub fn load(&self, key: &K) -> Option<Arc<SyncMapEntry<K, V>>> {
        let (entry_opt, ok) = self.shared.dirty.load(key);
        if ok {
            let entry = entry_opt.expect("dirty stores Some");
            {
                let guard = entry.state.lock().unwrap();
                if guard.data.delete {
                    return None;
                }
            }
            return Some(entry);
        }
        let value = self.shared.base.get(key)?;
        Some(new_entry(
            &self.shared,
            key.clone(),
            value.clone_cow(),
            value.clone_cow(),
            false,
            false,
        ))
    }

    /// Loads the existing entry for `key`, or stores a fresh dirty entry with
    /// `value`. Returns `(entry, loaded)` where `loaded` is true if the key
    /// already existed (in the base or overlay).
    ///
    /// Side effects: may insert into the overlay.
    // Go: internal/project/dirty/syncmap.go:SyncMap.LoadOrStore
    pub fn load_or_store(&self, key: K, value: V) -> (Option<Arc<SyncMapEntry<K, V>>>, bool) {
        // Check the base map first so the overlay access stays atomic.
        if let Some(base_value) = self.shared.base.get(&key) {
            let original = base_value.clone_cow();
            let value_clone = base_value.clone_cow();
            let (dirty_opt, ok) = self.shared.dirty.load(&key);
            if ok {
                let dirty = dirty_opt.expect("dirty stores Some");
                {
                    let guard = dirty.state.lock().unwrap();
                    if guard.data.delete {
                        return (None, false);
                    }
                }
                return (Some(dirty), true);
            }
            let entry = new_entry(&self.shared, key, original, value_clone, false, false);
            return (Some(entry), true);
        }
        let fresh = new_entry(&self.shared, key.clone(), V::default(), value, true, false);
        let (stored, loaded) = self.shared.dirty.load_or_store(key, Some(fresh.clone()));
        let entry = stored.expect("dirty stores Some");
        if loaded {
            let guard = entry.state.lock().unwrap();
            if guard.data.delete {
                return (None, false);
            }
        }
        (Some(entry), loaded)
    }

    /// Marks `key` deleted in the overlay.
    ///
    /// Side effects: inserts/updates the overlay.
    // Go: internal/project/dirty/syncmap.go:SyncMap.Delete
    pub fn delete(&self, key: K) {
        let original = self
            .shared
            .base
            .get(&key)
            .map(|v| v.clone_cow())
            .unwrap_or_default();
        let fresh = new_entry(
            &self.shared,
            key.clone(),
            original,
            V::default(),
            false,
            true,
        );
        let (stored, loaded) = self.shared.dirty.load_or_store(key, Some(fresh.clone()));
        if loaded {
            let entry = stored.expect("dirty stores Some");
            entry.delete();
        }
    }

    /// Calls `f` for each live entry until it returns false.
    ///
    /// Side effects: invokes `f`.
    // Go: internal/project/dirty/syncmap.go:SyncMap.Range
    pub fn range<F: FnMut(&Arc<SyncMapEntry<K, V>>) -> bool>(&self, mut f: F) {
        let mut seen_in_dirty: HashSet<K> = HashSet::new();
        self.shared.dirty.range(|key, entry_opt| {
            seen_in_dirty.insert(key.clone());
            let entry = entry_opt.as_ref().expect("dirty stores Some");
            let deleted = entry.state.lock().unwrap().data.delete;
            if !deleted && !f(entry) {
                return false;
            }
            true
        });
        for (key, value) in self.shared.base.iter() {
            if seen_in_dirty.contains(key) {
                continue; // already processed in dirty entries
            }
            let entry = new_entry(
                &self.shared,
                key.clone(),
                value.clone_cow(),
                value.clone_cow(),
                false,
                false,
            );
            if !f(&entry) {
                break;
            }
        }
    }

    /// Merges the base and overlay into a fresh map and reports whether the
    /// result differs from the base.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/syncmap.go:SyncMap.Finalize
    pub fn finalize(&self) -> (HashMap<K, V>, bool) {
        self.finalize_with(FinalizationHooks::default())
    }

    /// Like [`finalize`](SyncMap::finalize) but invokes `hooks` for each change.
    ///
    /// Side effects: invokes the hooks.
    // Go: internal/project/dirty/syncmap.go:SyncMap.FinalizeWith
    pub fn finalize_with(&self, hooks: FinalizationHooks<'_, K, V>) -> (HashMap<K, V>, bool) {
        let mut changed = false;
        let mut result = self.clone_base();
        self.shared.dirty.range(|key, entry_opt| {
            let entry = entry_opt.as_ref().expect("dirty stores Some");
            let st = entry.state.lock().unwrap();
            if st.data.delete {
                changed = true;
                if let Some(on_delete) = &hooks.on_delete {
                    on_delete(key, &st.data.value);
                }
                result.remove(key);
            } else if st.data.dirty {
                changed = true;
                if hooks.on_change.is_some() || hooks.on_add.is_some() {
                    if self.shared.base.contains_key(key) {
                        if let Some(on_change) = &hooks.on_change {
                            on_change(key, &st.data.original, &st.data.value);
                        }
                    } else if let Some(on_add) = &hooks.on_add {
                        on_add(key, &st.data.value);
                    }
                }
                result.insert(key.clone(), st.data.value.clone_cow());
            }
            true
        });
        (result, changed)
    }

    fn clone_base(&self) -> HashMap<K, V> {
        self.shared
            .base
            .iter()
            .map(|(k, v)| (k.clone(), v.clone_cow()))
            .collect()
    }
}

impl<K: Clone + Eq + Hash, V: Cloneable + Default> SyncMapEntry<K, V> {
    /// Returns the current value, or `V::default()` when deleted.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/syncmap.go:SyncMapEntry.Value
    pub fn value(&self) -> V {
        let guard = self.state.lock().unwrap();
        if let Some(proxy) = guard.proxy_for.clone() {
            drop(guard);
            return proxy.value();
        }
        guard.data.value()
    }

    /// Returns the original (loaded) value of the entry.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/entry.go:mapEntry.Original
    pub fn original(&self) -> V {
        self.state.lock().unwrap().data.original.clone_cow()
    }

    /// Reports whether the entry has been changed.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/syncmap.go:SyncMapEntry.Dirty
    pub fn dirty(&self) -> bool {
        let guard = self.state.lock().unwrap();
        if let Some(proxy) = guard.proxy_for.clone() {
            drop(guard);
            return proxy.dirty();
        }
        guard.data.dirty
    }

    /// Mutates the value, cloning on first change and routing racing handles.
    ///
    /// Side effects: mutates the value, marks it dirty, may install a proxy.
    // Go: internal/project/dirty/syncmap.go:SyncMapEntry.Change
    pub fn change<F: FnMut(&mut V)>(&self, mut apply: F) {
        self.change_dyn(&mut apply);
    }

    /// Mutates the value only when `cond` holds for the current value.
    ///
    /// Side effects: may mutate the value and mark it dirty.
    // Go: internal/project/dirty/syncmap.go:SyncMapEntry.ChangeIf
    pub fn change_if<C: FnMut(&V) -> bool, F: FnMut(&mut V)>(
        &self,
        mut cond: C,
        mut apply: F,
    ) -> bool {
        self.change_if_dyn(&mut cond, &mut apply)
    }

    /// Marks the entry for deletion.
    ///
    /// Side effects: marks the entry deleted, may insert into the overlay.
    // Go: internal/project/dirty/syncmap.go:SyncMapEntry.Delete
    pub fn delete(&self) {
        let mut guard = self.state.lock().unwrap();
        if let Some(proxy) = guard.proxy_for.clone() {
            drop(guard);
            proxy.delete();
            return;
        }
        if guard.data.dirty {
            guard.data.delete = true;
            return;
        }
        let me = self.me.upgrade().expect("entry alive");
        let shared = self.m.upgrade().expect("map alive");
        let key = guard.data.key.clone();
        let (stored, loaded) = shared.dirty.load_or_store(key, Some(me));
        let _canonical = stored.expect("dirty stores Some");
        if loaded {
            // Go locks the canonical entry but marks only this handle deleted.
            let _canonical_guard = _canonical.state.lock().unwrap();
            guard.data.delete = true;
        } else {
            // We are the canonical entry (already locked via `guard`).
            guard.data.delete = true;
        }
    }

    /// Marks the entry deleted only when `cond` holds for the current value.
    ///
    /// Side effects: may mark the entry deleted.
    // Go: internal/project/dirty/syncmap.go:SyncMapEntry.DeleteIf
    pub fn delete_if<C: FnMut(&V) -> bool>(&self, mut cond: C) {
        self.delete_if_dyn(&mut cond)
    }

    /// Invokes `f` with a locked [`Value`] view, holding the entry lock for the
    /// duration of the call.
    ///
    /// Side effects: invokes `f`; mutations through the view take effect.
    // Go: internal/project/dirty/syncmap.go:SyncMapEntry.Locked
    pub fn locked<F: FnMut(&dyn Value<V>)>(&self, mut f: F) {
        self.locked_dyn(&mut f)
    }

    fn change_dyn(&self, apply: &mut dyn FnMut(&mut V)) {
        let mut guard = self.state.lock().unwrap();
        if let Some(proxy) = guard.proxy_for.clone() {
            drop(guard);
            proxy.change_dyn(apply);
            return;
        }
        self.change_locked(&mut guard, apply);
    }

    // Go: internal/project/dirty/syncmap.go:SyncMapEntry.changeLocked
    fn change_locked(&self, st: &mut SyncEntryState<K, V>, apply: &mut dyn FnMut(&mut V)) {
        if st.data.dirty {
            apply(&mut st.data.value);
            return;
        }
        let me = self.me.upgrade().expect("entry alive");
        let shared = self.m.upgrade().expect("map alive");
        let key = st.data.key.clone();
        let (stored, loaded) = shared.dirty.load_or_store(key, Some(me));
        let canonical = stored.expect("dirty stores Some");
        if !loaded {
            // We won the race and are the canonical entry; mutate our own
            // already-locked state.
            // PERF(port): Go clones `value` here; this port cloned it at load,
            // so only the dirty flag flips.
            st.data.dirty = true;
            apply(&mut st.data.value);
        } else {
            // Another handle is canonical; become its proxy and route to it.
            let mut canonical_state = canonical.state.lock().unwrap();
            if !canonical_state.data.dirty {
                canonical_state.data.dirty = true;
            }
            st.proxy_for = Some(canonical.clone());
            st.data.value = canonical_state.data.value.clone_cow();
            st.data.dirty = true;
            st.data.delete = canonical_state.data.delete;
            apply(&mut canonical_state.data.value);
        }
    }

    fn change_if_dyn(
        &self,
        cond: &mut dyn FnMut(&V) -> bool,
        apply: &mut dyn FnMut(&mut V),
    ) -> bool {
        let mut guard = self.state.lock().unwrap();
        if let Some(proxy) = guard.proxy_for.clone() {
            drop(guard);
            return proxy.change_if_dyn(cond, apply);
        }
        // Go uses the raw `value` (not the deleted-zeroing read) here.
        if cond(&guard.data.value) {
            self.change_locked(&mut guard, apply);
            true
        } else {
            false
        }
    }

    fn delete_if_dyn(&self, cond: &mut dyn FnMut(&V) -> bool) {
        let mut guard = self.state.lock().unwrap();
        if let Some(proxy) = guard.proxy_for.clone() {
            drop(guard);
            proxy.delete_if_dyn(cond);
            return;
        }
        if cond(&guard.data.value) {
            self.delete_locked(&mut guard);
        }
    }

    fn locked_dyn(&self, f: &mut dyn FnMut(&dyn Value<V>)) {
        let mut guard = self.state.lock().unwrap();
        if let Some(proxy) = guard.proxy_for.clone() {
            drop(guard);
            proxy.locked_dyn(f);
            return;
        }
        let view = LockedEntry {
            entry: self,
            st: RefCell::new(&mut *guard),
        };
        f(&view);
    }

    // Go: internal/project/dirty/syncmap.go:SyncMapEntry.deleteLocked
    fn delete_locked(&self, st: &mut SyncEntryState<K, V>) {
        if st.data.dirty {
            st.data.delete = true;
            return;
        }
        let me = self.me.upgrade().expect("entry alive");
        let shared = self.m.upgrade().expect("map alive");
        let key = st.data.key.clone();
        let (stored, loaded) = shared.dirty.load_or_store(key, Some(me));
        let canonical = stored.expect("dirty stores Some");
        if loaded {
            let mut canonical_state = canonical.state.lock().unwrap();
            st.proxy_for = Some(canonical.clone());
            st.data.value = canonical_state.data.value.clone_cow();
            st.data.delete = true;
            st.data.dirty = canonical_state.data.dirty;
            canonical_state.data.delete = true;
        } else {
            // We are the canonical entry; `entry.delete = true` applies to us.
            st.data.delete = true;
        }
    }

    #[cfg(test)]
    fn proxy_for(&self) -> Option<Arc<SyncMapEntry<K, V>>> {
        self.state.lock().unwrap().proxy_for.clone()
    }
}

/// A [`Value`] view over a `SyncMapEntry` whose lock is already held.
///
/// Mirrors Go's `lockedEntry`: its methods operate on the already-locked state
/// without re-locking, so they can be used inside a [`SyncMapEntry::locked`]
/// callback. The `RefCell` provides the interior mutability needed to expose
/// the borrowed state through `Value`'s `&self` methods; the surrounding mutex
/// guard ensures exclusivity across threads.
struct LockedEntry<'a, K, V> {
    entry: &'a SyncMapEntry<K, V>,
    st: RefCell<&'a mut SyncEntryState<K, V>>,
}

impl<K: Clone + Eq + Hash, V: Cloneable + Default> Value<V> for LockedEntry<'_, K, V> {
    // Go: internal/project/dirty/syncmap.go:lockedEntry.Value
    fn value(&self) -> V {
        self.st.borrow().data.value()
    }

    // Go: internal/project/dirty/syncmap.go:lockedEntry.Original
    fn original(&self) -> V {
        self.st.borrow().data.original.clone_cow()
    }

    // Go: internal/project/dirty/syncmap.go:lockedEntry.Dirty
    fn dirty(&self) -> bool {
        self.st.borrow().data.dirty
    }

    // Go: internal/project/dirty/syncmap.go:lockedEntry.Change
    fn change(&self, apply: &mut dyn FnMut(&mut V)) {
        let mut st = self.st.borrow_mut();
        self.entry.change_locked(&mut **st, apply);
    }

    // Go: internal/project/dirty/syncmap.go:lockedEntry.ChangeIf
    fn change_if(&self, cond: &mut dyn FnMut(&V) -> bool, apply: &mut dyn FnMut(&mut V)) -> bool {
        let current = self.st.borrow().data.value();
        if cond(&current) {
            let mut st = self.st.borrow_mut();
            self.entry.change_locked(&mut **st, apply);
            true
        } else {
            false
        }
    }

    // Go: internal/project/dirty/syncmap.go:lockedEntry.Delete
    fn delete(&self) {
        let mut st = self.st.borrow_mut();
        self.entry.delete_locked(&mut **st);
    }

    // Go: internal/project/dirty/syncmap.go:lockedEntry.Locked
    fn locked(&self, f: &mut dyn FnMut(&dyn Value<V>)) {
        f(self);
    }
}

#[cfg(test)]
#[path = "syncmap_test.rs"]
mod tests;
