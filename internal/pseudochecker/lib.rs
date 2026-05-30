//! `tsgo_pseudochecker` — 1:1 Rust port of Go `internal/pseudochecker`.
//!
//! A limited "checker" that returns pseudo-"types" of expressions: mostly those
//! which trivially have type nodes. It powers `isolatedDeclarations` (ID), where
//! a `.d.ts` must be produced from a single file's syntax alone, without running
//! the full checker.
//!
//! # Scope of this port
//!
//! The data model ([`PseudoType`] and friends, Go `type.go`) and the
//! [`PseudoChecker`] entry point (Go `checker.go`) are ported in full. The
//! `lookup.go` derivation logic depends heavily on AST surface that the current
//! `tsgo_ast` representative port does not yet expose (declaration/accessor/
//! object-literal node data, `Symbol`, `FindAncestor`, `GetAllAccessorDeclarations`,
//! `IsPrimitiveLiteralValue`, ...). Only the expression core
//! ([`PseudoChecker::get_type_of_expression`]) is ported here; the remainder is
//! deferred (see `lookup.rs` `DEFER`/`blocked-by` markers and the package docs).

pub mod lookup;
pub mod ptype;

pub use ptype::{
    PseudoObjectElement, PseudoObjectElementKind, PseudoParameter, PseudoType, PseudoTypeKind,
};

// TODO: Late binding/symbol merging?
// In strada, `expressionToTypeNode` used many `resolver` methods whose net effect was just
// calling `Checker.GetMergedSymbol` on a symbol when dealing with accessors. Right now those
// just use Node.Symbol, which will fail to pair up late-bound symbols. In theory, this is actually
// fine, since ID can't possibly know if `set [q1()](a){}` and `get [q2()](): T {}` are connected
// without performing real type checking, regardless, so it shouldn't matter. If anything, it might be
// OK to add a "dumb" late binder that can merge multiple `[a.b.c]: T` together, but not anything else.
// This is an area of active ~~feature-creep~~ development in ID output, prerequisite refactoring would include
// extracting the `mergeSymbol` core checker logic into a reusable component.

/// A limited "checker" that derives pseudo-types from syntax for
/// `isolatedDeclarations`, holding only the two flags its derivation depends on.
///
/// # Examples
/// ```
/// use tsgo_pseudochecker::PseudoChecker;
/// let ch = PseudoChecker::new(true, false);
/// assert!(ch.strict_null_checks());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/pseudochecker/checker.go:PseudoChecker
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PseudoChecker {
    strict_null_checks: bool,
    exact_optional_property_types: bool,
}

impl PseudoChecker {
    /// Creates a [`PseudoChecker`] from the two flags it observes.
    ///
    /// # Examples
    /// ```
    /// use tsgo_pseudochecker::PseudoChecker;
    /// let ch = PseudoChecker::new(false, true);
    /// assert!(ch.exact_optional_property_types());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/checker.go:NewPseudoChecker
    pub fn new(strict_null_checks: bool, exact_optional_property_types: bool) -> PseudoChecker {
        PseudoChecker {
            strict_null_checks,
            exact_optional_property_types,
        }
    }

    /// Returns whether `strictNullChecks` is enabled.
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/checker.go:PseudoChecker.strictNullChecks (field access)
    pub fn strict_null_checks(&self) -> bool {
        self.strict_null_checks
    }

    /// Returns whether `exactOptionalPropertyTypes` is enabled.
    ///
    /// Side effects: none (pure).
    // Go: internal/pseudochecker/checker.go:PseudoChecker.exactOptionalPropertyTypes (field access)
    pub fn exact_optional_property_types(&self) -> bool {
        self.exact_optional_property_types
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
