//! 1:1 port of Go `internal/project/dirty/interfaces.go`.
//!
//! Defines the two abstractions the dirty-tracking containers are built on:
//! [`Cloneable`] (the copy-on-write deep clone constraint) and [`Value`] (the
//! mutable, dirty-trackable view over a single tracked value).

/// A value that can produce an independent copy of itself for copy-on-write.
///
/// This mirrors Go's `Cloneable[T] interface { Clone() T }`. The dirty
/// containers call [`clone_cow`](Cloneable::clone_cow) the first time a base
/// value is about to be mutated, so that the mutation never leaks back into the
/// shared base. The returned value must be fully independent of `self`.
///
/// `clone_cow` is intentionally distinct from [`Clone`] so that an
/// implementation can deep-copy on write even when the surrounding code uses a
/// cheap structural [`Clone`] elsewhere.
///
/// # Examples
/// ```
/// use tsgo_project_dirty::Cloneable;
///
/// #[derive(Clone, PartialEq, Debug)]
/// struct Doc {
///     text: String,
/// }
/// impl Cloneable for Doc {
///     fn clone_cow(&self) -> Self {
///         self.clone()
///     }
/// }
///
/// let a = Doc { text: "hi".into() };
/// let mut b = a.clone_cow();
/// b.text.push('!');
/// assert_eq!(a.text, "hi");
/// assert_eq!(b.text, "hi!");
/// ```
pub trait Cloneable {
    /// Returns a fully independent copy of `self` for copy-on-write.
    ///
    /// Side effects: none (pure).
    fn clone_cow(&self) -> Self;
}

/// A mutable, dirty-trackable view over a single tracked value.
///
/// This mirrors Go's `Value[T]` interface. It is the common surface exposed by
/// [`Box`](crate::Box), the entries of [`Map`](crate::Map), and the locked
/// entries of [`SyncMap`](crate::SyncMap). It is the object passed to the
/// callback of [`locked`](Value::locked), allowing a caller to read and mutate a
/// tracked value through a uniform interface.
///
/// All mutators use interior mutability (`&self`) because the same underlying
/// value may be shared between a container and an outstanding entry handle.
///
/// `T` is the tracked value type and must implement [`Cloneable`].
pub trait Value<T> {
    /// Returns the current value, or `T::default()` if the entry is marked for
    /// deletion (mirroring Go's zero-value return for a deleted entry).
    ///
    /// Side effects: none (pure).
    fn value(&self) -> T;

    /// Returns the original (pre-change) value the view was created from.
    ///
    /// Side effects: none (pure).
    fn original(&self) -> T;

    /// Reports whether the value has been changed since it was loaded.
    ///
    /// Side effects: none (pure).
    fn dirty(&self) -> bool;

    /// Applies `apply` to a mutable, owned-for-write copy of the value.
    ///
    /// On the first change the value is cloned via [`Cloneable::clone_cow`] so
    /// the change does not affect the shared base; subsequent changes reuse the
    /// already-dirty value.
    ///
    /// Side effects: mutates the tracked value and marks it dirty.
    fn change(&self, apply: &mut dyn FnMut(&mut T));

    /// Applies `apply` only when `cond` returns true for the current value.
    ///
    /// Returns whether a change was applied.
    ///
    /// Side effects: may mutate the tracked value and mark it dirty.
    fn change_if(&self, cond: &mut dyn FnMut(&T) -> bool, apply: &mut dyn FnMut(&mut T)) -> bool;

    /// Marks the value for deletion.
    ///
    /// Side effects: marks the entry deleted.
    fn delete(&self);

    /// Invokes `f` with a view that is valid for the duration of the call.
    ///
    /// For concurrent containers this holds the entry lock across `f`, giving
    /// the callback atomic read/modify access.
    ///
    /// Side effects: invokes `f`; any mutations performed through the view take
    /// effect on the tracked value.
    fn locked(&self, f: &mut dyn FnMut(&dyn Value<T>));
}
