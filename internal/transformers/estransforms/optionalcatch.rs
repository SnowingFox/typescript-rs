//! Port of Go `internal/transformers/estransforms/optionalcatch.go`: lowers
//! `catch {}` (no binding) to `catch (_e) {}` for pre-ES2019 targets.
//!
//! The ES2019 spec allows omitting the catch-clause binding variable
//! (`try { } catch { }`). This transform adds a synthesized temp variable
//! for targets that require it.
//!
//! # Deferred
//!
//! The `SubtreeFacts::CONTAINS_MISSING_CATCH_CLAUSE_VARIABLE` short-circuit
//! gate (Go checks `node.SubtreeFacts()`) is omitted: the Rust parser does
//! not yet compute subtree facts, so we visit all nodes. The gate is a pure
//! performance optimization and does not affect correctness.
//! DEFER(P5): add subtree-facts gate once the parser computes them.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeData, NodeId, VisitOptions};
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that adds a binding variable to bare `catch` clauses,
/// sharing the pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::optionalcatch::new_optional_catch_transformer, TransformOptions};
/// let _tx = new_optional_catch_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/optionalcatch.go:newOptionalCatchTransformer
pub fn new_optional_catch_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| optional_catch_visit(ec, node)),
        opt.context.clone(),
    )
}

/// Visits a node, dispatching catch clauses to the optional-catch transform.
///
/// Side effects: see [`new_optional_catch_transformer`].
// Go: internal/transformers/estransforms/optionalcatch.go:optionalCatchTransformer.visit
fn optional_catch_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::CatchClause => visit_catch_clause(ec, node),
        _ => {
            let opts = VisitOptions {
                synthetic_location: false,
                clone_lists: false,
            };
            ec.arena_mut()
                .visit_each_child(node, opts, &mut |a, c| optional_catch_visit_arena(a, c))
        }
    }
}

/// Arena-only recursive visitor (no emit context threading needed for this
/// simple transform).
fn optional_catch_visit_arena(arena: &mut tsgo_ast::NodeArena, node: NodeId) -> NodeId {
    match arena.kind(node) {
        Kind::CatchClause => visit_catch_clause_arena(arena, node),
        _ => {
            let opts = VisitOptions {
                synthetic_location: false,
                clone_lists: false,
            };
            arena.visit_each_child(node, opts, &mut |a, c| optional_catch_visit_arena(a, c))
        }
    }
}

/// If the catch clause has no variable declaration, add a temp variable.
///
/// Side effects: may allocate new nodes in the emit context's arena.
// Go: internal/transformers/estransforms/optionalcatch.go:optionalCatchTransformer.visitCatchClause
fn visit_catch_clause(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (variable_declaration, block) = match ec.arena().data(node) {
        NodeData::CatchClause(d) => (d.variable_declaration, d.block),
        _ => unreachable!("kind checked by caller"),
    };

    if variable_declaration.is_none() {
        let temp = ec.factory().new_temp_variable();
        let var_decl = ec
            .arena_mut()
            .new_variable_declaration(temp, None, None, None);
        let block = {
            let opts = VisitOptions {
                synthetic_location: false,
                clone_lists: false,
            };
            ec.arena_mut()
                .visit_each_child(block, opts, &mut |a, c| optional_catch_visit_arena(a, c))
        };
        return ec.arena_mut().new_catch_clause(Some(var_decl), block);
    }

    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    ec.arena_mut()
        .visit_each_child(node, opts, &mut |a, c| optional_catch_visit_arena(a, c))
}

/// Arena-only catch clause visitor (for recursion without emit context).
fn visit_catch_clause_arena(arena: &mut tsgo_ast::NodeArena, node: NodeId) -> NodeId {
    let (variable_declaration, block) = match arena.data(node) {
        NodeData::CatchClause(d) => (d.variable_declaration, d.block),
        _ => unreachable!("kind checked by caller"),
    };

    if variable_declaration.is_none() {
        let temp = arena.new_identifier("_a");
        let var_decl = arena.new_variable_declaration(temp, None, None, None);
        let block = {
            let opts = VisitOptions {
                synthetic_location: false,
                clone_lists: false,
            };
            arena.visit_each_child(block, opts, &mut |a, c| optional_catch_visit_arena(a, c))
        };
        return arena.new_catch_clause(Some(var_decl), block);
    }

    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| optional_catch_visit_arena(a, c))
}

#[cfg(test)]
#[path = "optionalcatch_test.rs"]
mod tests;
