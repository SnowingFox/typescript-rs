//! The checker's per-checker trace recorder.
//!
//! Go's `Tracer` (1) injects the owning checker's index into every trace
//! event's `args` under the key `checkerId`, and (2) records the types a
//! checker creates so they can be dumped to `types_<n>.json`. Round 4a ports
//! the deterministic arg-injection core; the event/type recording is deferred.
//!
//! # Deferred
//!
//! - `RecordType` / `wrapType` / the `tracedTypeAdapter` need the full `Type`
//!   graph (unions, conditionals, references, ...) and `TypeToString`, none of
//!   which exist before sub-phases 4b..4j.
//! - `Push` for the `separateBeginAndEnd` case relies on Go's `map[string]any`
//!   being a shared reference, so mutations made by the caller between `Push`
//!   and the returned `pop()` are observed by the end event (this is exactly
//!   what `TestTracerPushPreservesEndArgMutations` checks). The ported
//!   `tsgo_tracing::Tracing::push` takes `args` by value and snapshots them, so
//!   faithfully reproducing that aliasing would require either changing
//!   `tsgo_tracing` (out of this crate's scope) or a shared-mutable args design
//!   in the checker. It is therefore deferred to a later checker round.

use tsgo_tracing::{ArgValue, Args};

/// Records types and trace events on behalf of one checker.
///
/// A checker's index is woven into every trace event so a trace viewer can
/// attribute work to the right checker thread.
///
/// # Examples
/// ```
/// use tsgo_checker::Tracer;
/// let tracer = Tracer::new(7);
/// assert_eq!(tracer.checker_index(), 7);
/// ```
///
/// Side effects: none (pure value type in 4a; event/type recording is deferred).
// Go: internal/checker/tracer.go:Tracer
#[derive(Clone, Debug)]
pub struct Tracer {
    checker_index: i32,
}

impl Tracer {
    /// Creates a tracer for the given checker index.
    ///
    /// In Go this also creates the per-checker type recorder via
    /// `tr.NewTypeTracer(checkerIndex)`; that wiring is deferred until type
    /// recording is ported, so 4a stores only the index.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Tracer;
    /// let tracer = Tracer::new(0);
    /// assert_eq!(tracer.checker_index(), 0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/tracer.go:NewTracer
    pub fn new(checker_index: i32) -> Self {
        Tracer { checker_index }
    }

    /// Returns this tracer's checker index.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/tracer.go:Tracer.checkerIndex
    pub fn checker_index(&self) -> i32 {
        self.checker_index
    }

    /// Returns a copy of `args` with this tracer's `checkerId` added.
    ///
    /// The input is never mutated (Go allocates a fresh map of capacity
    /// `len+1`), so a caller's args map is never polluted with `checkerId` —
    /// the invariant `TestTracerPushPreservesEndArgMutations` asserts on the
    /// caller side. Any pre-existing `checkerId` is overwritten.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Tracer;
    /// use tsgo_tracing::{ArgValue, Args};
    /// let tracer = Tracer::new(7);
    /// let mut args = Args::new();
    /// args.insert("id".to_string(), ArgValue::Int(1));
    /// let with_id = tracer.copy_with_checker_index(&args);
    /// assert_eq!(with_id.get("checkerId"), Some(&ArgValue::Int(7)));
    /// assert_eq!(with_id.get("id"), Some(&ArgValue::Int(1)));
    /// // The caller's map is untouched.
    /// assert!(!args.contains_key("checkerId"));
    /// ```
    ///
    /// Side effects: none (pure); returns a new map.
    // Go: internal/checker/tracer.go:Tracer.copyWithCheckerIndex
    pub fn copy_with_checker_index(&self, args: &Args) -> Args {
        let mut with_checker_index = args.clone();
        with_checker_index.insert(
            "checkerId".to_string(),
            ArgValue::Int(self.checker_index as i64),
        );
        with_checker_index
    }
}

#[cfg(test)]
#[path = "tracer_test.rs"]
mod tests;
