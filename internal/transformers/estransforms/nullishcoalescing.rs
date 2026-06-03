//! Port of Go `internal/transformers/estransforms/nullishcoalescing.go`: lowers
//! `a ?? b` to a ternary `a !== null && a !== void 0 ? a : b` for pre-ES2020
//! targets.
//!
//! When the left-hand side is not a simple copiable expression (identifier or
//! literal), a temp variable is introduced: `(temp = a) !== null && temp !==
//! void 0 ? temp : b`.
//!
//! # Deferred
//!
//! The `SubtreeFacts::CONTAINS_NULLISH_COALESCING` short-circuit gate (Go
//! checks `node.SubtreeFacts()`) is omitted: the Rust parser does not yet
//! compute subtree facts. The gate is a pure performance optimization.
//! DEFER(P5): add subtree-facts gate once the parser computes them.

use crate::{new_transformer, TransformOptions, Transformer};
use rustc_hash::FxHashMap;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, TokenFlags, VisitOptions};
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that lowers `??` to conditional expressions,
/// sharing the pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::nullishcoalescing::new_nullish_coalescing_transformer, TransformOptions};
/// let _tx = new_nullish_coalescing_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/nullishcoalescing.go:newNullishCoalescingTransformer
pub fn new_nullish_coalescing_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| nullish_visit(ec, node)),
        opt.context.clone(),
    )
}

/// Visits a node, dispatching binary expressions for `??` lowering.
///
/// Side effects: see [`new_nullish_coalescing_transformer`].
// Go: internal/transformers/estransforms/nullishcoalescing.go:nullishCoalescingTransformer.visit
fn nullish_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::BinaryExpression => visit_binary_expression(ec, node),
        _ => visit_each_child_ec(ec, node),
    }
}

/// EC-threaded recursive descent: collects children, transforms each through
/// [`nullish_visit`], replays through the arena's `visit_each_child` with a
/// replacement map.
fn visit_each_child_ec(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let mut children = Vec::new();
    ec.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    let mut replacements: FxHashMap<NodeId, NodeId> = FxHashMap::default();
    for child in children {
        let transformed = nullish_visit(ec, child);
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

/// Lowers `a ?? b` to `a !== null && a !== void 0 ? a : b`.
///
/// Side effects: may allocate temp variables and new nodes.
// Go: internal/transformers/estransforms/nullishcoalescing.go:nullishCoalescingTransformer.visitBinaryExpression
fn visit_binary_expression(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (left, op_token, right) = match ec.arena().data(node) {
        NodeData::BinaryExpression(d) => (d.left, d.operator_token, d.right),
        _ => unreachable!("kind checked by caller"),
    };

    if ec.arena().kind(op_token) != Kind::QuestionQuestionToken {
        return visit_each_child_ec(ec, node);
    }

    let visited_left = nullish_visit(ec, left);
    let visited_right = nullish_visit(ec, right);

    let (condition_left, condition_right) = if is_simple_copiable(ec.arena(), visited_left) {
        (visited_left, visited_left)
    } else {
        let temp = ec.factory().new_temp_variable();
        ec.add_variable_declaration(temp);
        let assign = make_assignment(ec.arena_mut(), temp, visited_left);
        (assign, temp)
    };

    let condition =
        create_not_null_condition(ec.arena_mut(), condition_left, condition_right, false);
    let question = ec.arena_mut().new_token(Kind::QuestionToken);
    let colon = ec.arena_mut().new_token(Kind::ColonToken);
    ec.arena_mut().new_conditional_expression(
        condition,
        question,
        condition_right,
        colon,
        visited_right,
    )
}

/// Reports whether `expr` is a simple expression safe to duplicate without side effects.
///
/// Side effects: none (pure).
// Go: internal/transformers/utilities.go:IsSimpleCopiableExpression
pub(crate) fn is_simple_copiable(arena: &NodeArena, expr: NodeId) -> bool {
    let kind = arena.kind(expr);
    kind == Kind::Identifier
        || kind == Kind::StringLiteral
        || kind == Kind::NoSubstitutionTemplateLiteral
        || kind == Kind::NumericLiteral
        || kind == Kind::BigIntLiteral
        || tsgo_ast::utilities::is_keyword_kind(kind)
}

/// Builds `left !== null && right !== void 0` (or the inverted form when
/// `invert` is true: `left === null || right === void 0`).
///
/// Side effects: pushes nodes.
// Go: internal/transformers/estransforms/utilities.go:createNotNullCondition
pub(crate) fn create_not_null_condition(
    arena: &mut NodeArena,
    left: NodeId,
    right: NodeId,
    invert: bool,
) -> NodeId {
    let (token_kind, op_kind) = if invert {
        (Kind::EqualsEqualsEqualsToken, Kind::BarBarToken)
    } else {
        (
            Kind::ExclamationEqualsEqualsToken,
            Kind::AmpersandAmpersandToken,
        )
    };

    let null_kw = arena.new_keyword_expression(Kind::NullKeyword);
    let token1 = arena.new_token(token_kind);
    let null_check = arena.new_binary_expression(left, token1, null_kw);

    let zero = arena.new_numeric_literal("0", TokenFlags::NONE);
    let void_zero = arena.new_void_expression(zero);
    let token2 = arena.new_token(token_kind);
    let void_check = arena.new_binary_expression(right, token2, void_zero);

    let op = arena.new_token(op_kind);
    arena.new_binary_expression(null_check, op, void_check)
}

/// Builds `target = value` as an assignment expression.
fn make_assignment(arena: &mut NodeArena, target: NodeId, value: NodeId) -> NodeId {
    let eq = arena.new_token(Kind::EqualsToken);
    arena.new_binary_expression(target, eq, value)
}

#[cfg(test)]
#[path = "nullishcoalescing_test.rs"]
mod tests;
