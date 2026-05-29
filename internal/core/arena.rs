//! Index-based arena allocator (`Arena`).
//!
//! 1:1 port of Go `internal/core/arena.go`. Go's `Arena.New` returns stable raw
//! pointers backed by a bump allocator; to stay zero-`unsafe` and to unify with
//! the AST/symbol arenas used later in the port, this wraps `la_arena::Arena`
//! and hands out [`Idx`] handles that remain valid for the arena's lifetime.
//!
//! Divergence: references are index handles (`Idx<T>`) rather than `*T`. This
//! is the documented arena ownership model (see `core/impl.md`).

use la_arena::Arena as LaArena;
pub use la_arena::Idx;

/// A bump-style arena that hands out stable [`Idx`] handles.
///
/// # Examples
/// ```
/// use tsgo_core::arena::Arena;
/// let mut a: Arena<i32> = Arena::new();
/// let id1 = a.alloc(1);
/// let id2 = a.alloc(2);
/// assert_eq!(a[id1], 1);
/// a[id1] = 10;
/// assert_eq!(a[id1], 10);
/// assert_eq!(a[id2], 2);
/// ```
#[derive(Clone, Debug)]
pub struct Arena<T> {
    inner: LaArena<T>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena {
            inner: LaArena::new(),
        }
    }
}

impl<T> Arena<T> {
    /// Creates an empty arena.
    ///
    /// Side effects: none (pure).
    pub fn new() -> Self {
        Arena::default()
    }

    /// Allocates `value` and returns its stable handle.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/core/arena.go:New
    pub fn alloc(&mut self, value: T) -> Idx<T> {
        self.inner.alloc(value)
    }

    /// Returns the number of allocated elements.
    ///
    /// Side effects: none (pure).
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Reports whether the arena is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl<T> std::ops::Index<Idx<T>> for Arena<T> {
    type Output = T;
    fn index(&self, id: Idx<T>) -> &T {
        &self.inner[id]
    }
}

impl<T> std::ops::IndexMut<Idx<T>> for Arena<T> {
    fn index_mut(&mut self, id: Idx<T>) -> &mut T {
        &mut self.inner[id]
    }
}

#[cfg(test)]
#[path = "arena_test.rs"]
mod tests;
