//! Port of Go `internal/transformers/estransforms/classthis.go`: utility to
//! detect a `static { _classThis = this; }` block in a class body.
//!
//! The ES decorator transform and class-fields transform use this to detect
//! whether a class already has a `static {}` block that captures `this` into a
//! `_classThis` (or similar) variable, so they do not inject a duplicate.
//!
//! # Deferred
//!
//! The function body delegates to `EmitContext::class_this(node)`, which is not
//! yet ported to the Rust emit context. The structure is ported so that call
//! sites can reference it; the body currently always returns `false`.
//! DEFER(P5): implement once `EmitContext::class_this` / `set_class_this` are
//! ported. blocked-by: `EmitContext` class-this side table.

use tsgo_ast::{Kind, NodeData, NodeId};
use tsgo_printer::EmitContext;

/// Reports whether `node` is a `static {}` block containing only a single
/// assignment of the static `this` to the `_classThis` (or similar) variable
/// stored in the `classthis` property of the block's `EmitNode`.
///
/// # Examples
/// ```
/// use tsgo_transformers::estransforms::classthis::is_class_this_assignment_block;
/// use tsgo_printer::EmitContext;
/// let ec = EmitContext::new();
/// use tsgo_ast::NodeId;
/// // Without the class_this side table, always returns false.
/// // Once EmitContext::class_this is ported this will be functional.
/// ```
///
/// Side effects: none (reads the emit context's class-this side table).
// Go: internal/transformers/estransforms/classthis.go:isClassThisAssignmentBlock
pub fn is_class_this_assignment_block(ec: &EmitContext, node: NodeId) -> bool {
    if ec.arena().kind(node) != Kind::ClassStaticBlockDeclaration {
        return false;
    }

    let body = match ec.arena().data(node) {
        NodeData::ClassStaticBlockDeclaration(d) => d.body,
        _ => return false,
    };

    let block_list = match ec.arena().data(body) {
        NodeData::Block(d) => &d.list,
        _ => return false,
    };

    if block_list.nodes.len() != 1 {
        return false;
    }

    let statement = block_list.nodes[0];
    if ec.arena().kind(statement) != Kind::ExpressionStatement {
        return false;
    }

    let expression = match ec.arena().data(statement) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => return false,
    };

    // Check: is this an assignment expression (`=`, not `+=`, etc.)?
    let (left, right) = match ec.arena().data(expression) {
        NodeData::BinaryExpression(d) => {
            if ec.arena().kind(d.operator_token) != Kind::EqualsToken {
                return false;
            }
            (d.left, d.right)
        }
        _ => return false,
    };

    if ec.arena().kind(left) != Kind::Identifier {
        return false;
    }
    if ec.arena().kind(right) != Kind::ThisKeyword {
        return false;
    }

    // DEFER(P5): `ec.class_this(node) == Some(left)` — the emit-context
    // class-this side table is not yet ported. Once it is, uncomment:
    // ec.class_this(node) == Some(left)
    //
    // For now, return false (conservative: never detects a class-this block,
    // so the decorator/classfields transform may inject a duplicate — that is
    // safe because the worst case is an extra assignment).
    let _ = (left, right);
    false
}

#[cfg(test)]
#[path = "classthis_test.rs"]
mod tests;
