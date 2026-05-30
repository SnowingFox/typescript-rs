//! 1:1 port of Go `internal/project/dirty/box.go`.

use std::cell::RefCell;

use crate::interfaces::{Cloneable, Value};

struct BoxState<T> {
    value: T,
    dirty: bool,
    delete: bool,
}

/// A copy-on-write, dirty-trackable wrapper around a single value.
///
/// A `Box` starts undirtied with its value equal to the original it was created
/// from. Mutating through [`change`](Box::change) clones the value on the first
/// write so the original is preserved; [`set`](Box::set) replaces it outright.
/// [`finalize`](Box::finalize) reports the resulting value together with whether
/// it changed (was set, mutated, or deleted).
pub struct Box<T> {
    original: T,
    state: RefCell<BoxState<T>>,
}

impl<T: Cloneable + Default> Box<T> {
    /// Creates a box whose current value equals `original` and is not dirty.
    ///
    /// # Examples
    /// ```
    /// use tsgo_project_dirty::{Box, Cloneable};
    /// #[derive(Clone, Default, PartialEq, Debug)]
    /// struct V(i32);
    /// impl Cloneable for V { fn clone_cow(&self) -> Self { self.clone() } }
    /// let b = Box::new(V(1));
    /// assert_eq!(b.value(), V(1));
    /// assert!(!b.dirty());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/box.go:NewBox
    pub fn new(original: T) -> Self {
        let value = original.clone_cow();
        Box {
            original,
            state: RefCell::new(BoxState {
                value,
                dirty: false,
                delete: false,
            }),
        }
    }

    /// Returns the current value, or `T::default()` when the box is deleted.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/box.go:Box.Value
    pub fn value(&self) -> T {
        let state = self.state.borrow();
        if state.delete {
            return T::default();
        }
        state.value.clone_cow()
    }

    /// Returns the original value the box was created from.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/box.go:Box.Original
    pub fn original(&self) -> T {
        self.original.clone_cow()
    }

    /// Reports whether the box has been changed.
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/box.go:Box.Dirty
    pub fn dirty(&self) -> bool {
        self.state.borrow().dirty
    }

    /// Replaces the value, clearing the delete flag and marking the box dirty.
    ///
    /// Side effects: replaces the value and marks the box dirty.
    // Go: internal/project/dirty/box.go:Box.Set
    pub fn set(&self, value: T) {
        let mut state = self.state.borrow_mut();
        state.value = value;
        state.delete = false;
        state.dirty = true;
    }

    /// Applies `apply` to the value, cloning it on the first change.
    ///
    /// Side effects: mutates the value and marks the box dirty.
    // Go: internal/project/dirty/box.go:Box.Change
    pub fn change<F: FnMut(&mut T)>(&self, mut apply: F) {
        self.change_inner(&mut apply);
    }

    fn change_inner(&self, apply: &mut dyn FnMut(&mut T)) {
        let mut state = self.state.borrow_mut();
        if !state.dirty {
            // PERF(port): Go clones lazily on first write (`value` aliases the
            // original until then); here `value` is already an independent copy
            // made at construction, so only the dirty flag flips.
            state.dirty = true;
        }
        apply(&mut state.value);
    }

    /// Applies `apply` only when `cond` holds for the current value.
    ///
    /// Returns whether a change was applied.
    ///
    /// Side effects: may mutate the value and mark the box dirty.
    // Go: internal/project/dirty/box.go:Box.ChangeIf
    pub fn change_if<C: FnMut(&T) -> bool, F: FnMut(&mut T)>(
        &self,
        mut cond: C,
        mut apply: F,
    ) -> bool {
        self.change_if_inner(&mut cond, &mut apply)
    }

    fn change_if_inner(
        &self,
        cond: &mut dyn FnMut(&T) -> bool,
        apply: &mut dyn FnMut(&mut T),
    ) -> bool {
        let current = self.state.borrow().value.clone_cow();
        if cond(&current) {
            self.change_inner(apply);
            return true;
        }
        false
    }

    /// Marks the box for deletion.
    ///
    /// Side effects: marks the box deleted.
    // Go: internal/project/dirty/box.go:Box.Delete
    pub fn delete(&self) {
        self.state.borrow_mut().delete = true;
    }

    /// Invokes `f` with this box viewed as a [`Value`].
    ///
    /// Side effects: invokes `f`.
    // Go: internal/project/dirty/box.go:Box.Locked
    pub fn locked<F: FnMut(&dyn Value<T>)>(&self, mut f: F) {
        f(self);
    }

    /// Returns the final value and whether the box changed (dirty or deleted).
    ///
    /// Side effects: none (pure).
    // Go: internal/project/dirty/box.go:Box.Finalize
    pub fn finalize(&self) -> (T, bool) {
        let state = self.state.borrow();
        let changed = state.dirty || state.delete;
        let value = if state.delete {
            T::default()
        } else {
            state.value.clone_cow()
        };
        (value, changed)
    }
}

impl<T: Cloneable + Default> Value<T> for Box<T> {
    fn value(&self) -> T {
        Box::value(self)
    }

    fn original(&self) -> T {
        Box::original(self)
    }

    fn dirty(&self) -> bool {
        Box::dirty(self)
    }

    fn change(&self, apply: &mut dyn FnMut(&mut T)) {
        self.change_inner(apply);
    }

    fn change_if(&self, cond: &mut dyn FnMut(&T) -> bool, apply: &mut dyn FnMut(&mut T)) -> bool {
        self.change_if_inner(cond, apply)
    }

    fn delete(&self) {
        Box::delete(self);
    }

    fn locked(&self, f: &mut dyn FnMut(&dyn Value<T>)) {
        f(self);
    }
}

#[cfg(test)]
#[path = "box_test.rs"]
mod tests;
