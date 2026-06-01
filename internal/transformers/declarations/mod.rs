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
//! - `tracker.go` → [`tracker`] (the accessibility / inference-fallback
//!   diagnostic recorder, Go's `SymbolTrackerImpl`).
//! - `diagnostics.go` → [`diagnostics`] (the declaration-emit diagnostic
//!   message-selection tables).
//!
//! Round D-F1 ports the *annotated-declaration core*, D-F2 the inferred-type
//! node synthesis, and D-F3 (this round) the visibility / accessibility gating
//! (module-vs-script elision, import/export `.d.ts` handling), the
//! `SymbolTracker` private-name family, and the `--isolatedDeclarations`
//! explicit-annotation diagnostics — see [`transform`]'s module docs for the
//! covered behaviors and the DEFER list.

pub mod diagnostics;
pub mod tracker;
pub mod transform;
pub mod util;

pub use transform::{
    new_declarations_transformer, new_declarations_transformer_with_diagnostics,
    DeclarationsTransformer,
};
