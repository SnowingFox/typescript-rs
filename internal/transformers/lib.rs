//! Port of Go package `internal/transformers`.
//!
//! The transform pipeline lowers a checked TypeScript AST toward emittable
//! JavaScript: type erasure, downleveling (ES decorators, class fields, async),
//! module transforms, JSX, and `.d.ts` declaration emit. Each Go subpackage
//! (`tstransforms`, `estransforms`, `moduletransforms`, `jsxtransforms`,
//! `declarations`) is ported as a submodule of this crate, with `.rs` files
//! living next to their `.go` counterparts.
//!
//! Ported incrementally under strict TDD (see `docs/rust-rewrite/references/tdd.md`
//! and the `/tdd` skill). Round 6a establishes the shared transform
//! infrastructure (the `Transformer` driver, chaining, modifier visiting, and
//! shared utilities); later rounds fill in the individual transforms.

use std::cell::RefCell;
use std::rc::Rc;
use tsgo_printer::EmitContext;

pub mod chain;
pub mod modifiervisitor;
pub mod transformer;
pub mod tstransforms;
pub mod utilities;

pub use chain::{chain, TransformOptions, TransformerFactory};
pub use modifiervisitor::extract_modifiers;
pub use transformer::{new_transformer, Transformer, VisitFn};

/// A shared, mutable [`EmitContext`] handle.
///
/// Go threads a single `*printer.EmitContext` pointer through every transformer
/// in a pipeline; the Rust port shares it as `Rc<RefCell<EmitContext>>` so the
/// chained transformers all append to one arena (PORTING.md §3).
///
/// Side effects: none (a type alias).
pub type SharedEmitContext = Rc<RefCell<EmitContext>>;

#[cfg(test)]
#[path = "test_support.rs"]
pub(crate) mod test_support;
