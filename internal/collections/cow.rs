//! Copy-on-write map/set with scoped rollback (`CopyOnWriteMap` /
//! `CopyOnWriteSet`).
//!
//! 1:1 port of Go `internal/collections/cow.go`. Go uses an `owned bool` flag
//! plus a returned restore closure; here the backing map is an `Rc` cloned on
//! first write (`ensure_owned`), and [`CopyOnWriteMap::enter_scope`] returns a
//! saved snapshot passed back to [`CopyOnWriteMap::restore`] (mirroring Go's
//! returned restore function called explicitly).

use std::hash::Hash;
use std::rc::Rc;

use rustc_hash::FxHashMap;

/// A saved copy-on-write scope, restored via [`CopyOnWriteMap::restore`].
#[derive(Clone)]
pub struct CowScope<K, V> {
    m: Rc<FxHashMap<K, V>>,
    owned: bool,
}

/// A map that defers cloning an inherited backing map until the first mutation
/// and supports nested scopes that share the parent's storage for reads.
///
/// # Examples
/// ```
/// use tsgo_collections::CopyOnWriteMap;
/// let mut c: CopyOnWriteMap<&str, i32> = CopyOnWriteMap::default();
/// c.set("a", 1);
/// let scope = c.enter_scope();
/// c.set("a", 2);
/// assert_eq!(c.get(&"a"), Some(&2));
/// c.restore(scope);
/// assert_eq!(c.get(&"a"), Some(&1));
/// ```
pub struct CopyOnWriteMap<K, V> {
    m: Rc<FxHashMap<K, V>>,
    owned: bool,
}

impl<K, V> Default for CopyOnWriteMap<K, V> {
    fn default() -> Self {
        CopyOnWriteMap {
            m: Rc::new(FxHashMap::default()),
            owned: false,
        }
    }
}

impl<K: Eq + Hash + Clone, V: Clone> CopyOnWriteMap<K, V> {
    /// Returns the value for `k`, if present.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/cow.go:Get
    pub fn get(&self, k: &K) -> Option<&V> {
        self.m.get(k)
    }

    /// Reports whether `k` is present.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/cow.go:Has
    pub fn has(&self, k: &K) -> bool {
        self.m.contains_key(k)
    }

    /// Assigns `v` to `k`, cloning the inherited backing map first if necessary.
    ///
    /// Side effects: mutates `self` (clones the backing map on first write).
    // Go: internal/collections/cow.go:Set
    pub fn set(&mut self, k: K, v: V) {
        self.ensure_owned();
        Rc::make_mut(&mut self.m).insert(k, v);
    }

    // Go: internal/collections/cow.go:ensureOwned
    fn ensure_owned(&mut self) {
        if self.owned {
            return;
        }
        self.m = Rc::new((*self.m).clone());
        self.owned = true;
    }

    /// Enters a nested scope sharing the current backing storage for reads.
    ///
    /// The returned snapshot restores this map's state when passed to
    /// [`CopyOnWriteMap::restore`].
    ///
    /// Side effects: mutates `self` (marks the backing map as inherited).
    // Go: internal/collections/cow.go:EnterScope
    pub fn enter_scope(&mut self) -> CowScope<K, V> {
        let saved = CowScope {
            m: Rc::clone(&self.m),
            owned: self.owned,
        };
        self.owned = false;
        saved
    }

    /// Restores this map to the state captured by `scope`.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/cow.go:EnterScope (restore closure)
    pub fn restore(&mut self, scope: CowScope<K, V>) {
        self.m = scope.m;
        self.owned = scope.owned;
    }
}

/// A copy-on-write set built on [`CopyOnWriteMap`].
///
/// # Examples
/// ```
/// use tsgo_collections::CopyOnWriteSet;
/// let mut c: CopyOnWriteSet<&str> = CopyOnWriteSet::default();
/// c.add("a");
/// assert!(c.has(&"a"));
/// ```
pub struct CopyOnWriteSet<K> {
    m: CopyOnWriteMap<K, ()>,
}

impl<K> Default for CopyOnWriteSet<K> {
    fn default() -> Self {
        CopyOnWriteSet {
            m: CopyOnWriteMap::default(),
        }
    }
}

impl<K: Eq + Hash + Clone> CopyOnWriteSet<K> {
    /// Reports whether `k` is in the set.
    ///
    /// Side effects: none (pure).
    // Go: internal/collections/cow.go:Has
    pub fn has(&self, k: &K) -> bool {
        self.m.has(k)
    }

    /// Adds `k` to the set, cloning the inherited backing map first if needed.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/cow.go:Add
    pub fn add(&mut self, k: K) {
        self.m.set(k, ());
    }

    /// Enters a nested scope; the returned snapshot restores via
    /// [`CopyOnWriteSet::restore`].
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/cow.go:EnterScope
    pub fn enter_scope(&mut self) -> CowScope<K, ()> {
        self.m.enter_scope()
    }

    /// Restores this set to the state captured by `scope`.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/collections/cow.go:EnterScope (restore closure)
    pub fn restore(&mut self, scope: CowScope<K, ()>) {
        self.m.restore(scope);
    }
}

#[cfg(test)]
#[path = "cow_test.rs"]
mod tests;
