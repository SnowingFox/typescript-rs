//! Port of Go `internal/transformers/estransforms/logicalassignment.go`: lowers
//! logical assignment operators (`&&=`, `||=`, `??=`) for pre-ES2021 targets.
//!
//! `a &&= b` → `a && (a = b)`
//! `a ||= b` → `a || (a = b)`
//! `a ??= b` → `a ?? (a = b)`
//!
//! For access expressions (`a.x ??= b`), the receiver is cached in a temp to
//! avoid double evaluation.
//!
//! # Deferred
//!
//! The `SubtreeFacts::CONTAINS_LOGICAL_ASSIGNMENTS` short-circuit gate is
//! omitted: the Rust parser does not yet compute subtree facts.
//! DEFER(P5): add subtree-facts gate once the parser computes them.

use crate::{new_transformer, TransformOptions, Transformer};
use rustc_hash::FxHashMap;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, VisitOptions};
use tsgo_printer::EmitContext;

use super::nullishcoalescing::is_simple_copiable;

/// Builds a [`Transformer`] that lowers logical assignment operators,
/// sharing the pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::logicalassignment::new_logical_assignment_transformer, TransformOptions};
/// let _tx = new_logical_assignment_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/logicalassignment.go:newLogicalAssignmentTransformer
pub fn new_logical_assignment_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| logical_visit(ec, node)),
        opt.context.clone(),
    )
}

/// Visits a node, dispatching binary expressions for logical assignment lowering.
///
/// Side effects: see [`new_logical_assignment_transformer`].
// Go: internal/transformers/estransforms/logicalassignment.go:logicalAssignmentTransformer.visit
fn logical_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::BinaryExpression => visit_binary_expression(ec, node),
        _ => visit_each_child_ec(ec, node),
    }
}

/// EC-threaded recursive descent.
fn visit_each_child_ec(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let mut children = Vec::new();
    ec.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    let mut replacements: FxHashMap<NodeId, NodeId> = FxHashMap::default();
    for child in children {
        let transformed = logical_visit(ec, child);
        if transformed != child {
            replacements.insert(child, transformed);
        }
    }
    if replacements.is_empty() {
        return node;
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    ec.arena_mut()
        .visit_each_child(node, opts, &mut |_, child| {
            replacements.get(&child).copied().unwrap_or(child)
        })
}

/// Lowers `a &&= b` → `a && (a = b)`, `a ||= b` → `a || (a = b)`,
/// `a ??= b` → `a ?? (a = b)`.
///
/// Side effects: may allocate temp variables and new nodes.
// Go: internal/transformers/estransforms/logicalassignment.go:logicalAssignmentTransformer.visitBinaryExpression
fn visit_binary_expression(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (left, op_token, right) = match ec.arena().data(node) {
        NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
        _ => unreachable!("kind checked by caller"),
    };

    let op_kind = ec.arena().kind(op_token);
    let non_assignment_operator = match op_kind {
        Kind::BarBarEqualsToken => Kind::BarBarToken,
        Kind::AmpersandAmpersandEqualsToken => Kind::AmpersandAmpersandToken,
        Kind::QuestionQuestionEqualsToken => Kind::QuestionQuestionToken,
        _ => return visit_each_child_ec(ec, node),
    };

    let visited_left = logical_visit(ec, left);
    let visited_left = skip_parentheses(ec.arena(), visited_left);
    let mut assignment_target = visited_left;
    let visited_right = logical_visit(ec, right);
    let visited_right = skip_parentheses(ec.arena(), visited_right);

    let mut result_left = visited_left;

    if is_access_expression(ec.arena(), visited_left) {
        let expr = get_expression(ec.arena(), visited_left);
        let simple = is_simple_copiable(ec.arena(), expr);

        let (target_expr, target_assign) = if simple {
            (expr, expr)
        } else {
            let temp = ec.factory().new_temp_variable();
            ec.add_variable_declaration(temp);
            let assign = make_assignment(ec.arena_mut(), temp, expr);
            (temp, assign)
        };

        if ec.arena().kind(visited_left) == Kind::PropertyAccessExpression {
            let name = get_name(ec.arena(), visited_left);
            assignment_target =
                ec.arena_mut()
                    .new_property_access_expression(target_expr, None, name);
            result_left = ec
                .arena_mut()
                .new_property_access_expression(target_assign, None, name);
        } else {
            let arg = get_argument(ec.arena(), visited_left);
            let arg_simple = is_simple_copiable(ec.arena(), arg);

            let (arg_target, arg_expr) = if arg_simple {
                (arg, arg)
            } else {
                let temp = ec.factory().new_temp_variable();
                ec.add_variable_declaration(temp);
                let assign = make_assignment(ec.arena_mut(), temp, arg);
                (temp, assign)
            };

            assignment_target =
                ec.arena_mut()
                    .new_element_access_expression(target_expr, None, arg_target);
            result_left =
                ec.arena_mut()
                    .new_element_access_expression(target_assign, None, arg_expr);
        }
    }

    let assign = make_assignment(ec.arena_mut(), assignment_target, visited_right);
    let paren = ec.arena_mut().new_parenthesized_expression(assign);
    let op = ec.arena_mut().new_token(non_assignment_operator);
    ec.arena_mut().new_binary_expression(result_left, op, paren)
}

/// Skips parentheses around an expression.
fn skip_parentheses(arena: &NodeArena, mut node: NodeId) -> NodeId {
    while arena.kind(node) == Kind::ParenthesizedExpression {
        node = match arena.data(node) {
            NodeData::ParenthesizedExpression(d) => d.expression,
            _ => break,
        };
    }
    node
}

/// Reports whether `node` is a property-access or element-access expression.
fn is_access_expression(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.kind(node),
        Kind::PropertyAccessExpression | Kind::ElementAccessExpression
    )
}

/// Gets the `.expression` (receiver) of a property-access or element-access.
fn get_expression(arena: &NodeArena, node: NodeId) -> NodeId {
    match arena.data(node) {
        NodeData::PropertyAccessExpression(d) => d.expression,
        NodeData::ElementAccessExpression(d) => d.expression,
        _ => panic!("expected access expression"),
    }
}

/// Gets the `.name` of a property-access expression.
fn get_name(arena: &NodeArena, node: NodeId) -> NodeId {
    match arena.data(node) {
        NodeData::PropertyAccessExpression(d) => d.name,
        _ => panic!("expected PropertyAccessExpression"),
    }
}

/// Gets the argument of an element-access expression.
fn get_argument(arena: &NodeArena, node: NodeId) -> NodeId {
    match arena.data(node) {
        NodeData::ElementAccessExpression(d) => d.argument_expression,
        _ => panic!("expected ElementAccessExpression"),
    }
}

/// Builds `target = value` as an assignment expression.
fn make_assignment(arena: &mut NodeArena, target: NodeId, value: NodeId) -> NodeId {
    let eq = arena.new_token(Kind::EqualsToken);
    arena.new_binary_expression(target, eq, value)
}

#[cfg(test)]
#[path = "logicalassignment_test.rs"]
mod tests;
