//! Port of Go `internal/transformers/declarations`: the declaration (`.d.ts`)
//! emit transform.
//!
//! The declaration transformer turns a checked TypeScript source file into its
//! ambient declaration shape — implementation bodies and initializers removed,
//! `declare`/`export` modifiers normalized, type annotations preserved. Each Go
//! file maps to a sibling `.rs`:
//!
//! - `transform.go` → [`transform`] (the `DeclarationsTransformer` driver +
//!   per-declaration-kind handlers).
//! - `util.go` → [`util`] (the pure modifier-flag / structural helpers).
//!
//! Round D-F1 ports the *annotated-declaration core* (see [`transform`]'s
//! module docs for the covered behaviors and the DEFER list). The
//! `SymbolTracker` (`tracker.go`) and the declaration-emit diagnostics
//! (`diagnostics.go`) are deferred to D-F3 (visibility / accessibility error
//! tracking + isolatedDeclarations), and the inferred-type node synthesis is
//! deferred to D-F2.

pub mod transform;
pub mod util;

pub use transform::{new_declarations_transformer, DeclarationsTransformer};
