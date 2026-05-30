//! Port of Go `internal/transformers/estransforms/optionalchain.go`: lowers
//! ES2020 optional chains (`a?.b`, `a?.[x]`, `a?.()`) to conditional
//! expressions guarded by a not-null check.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, TokenFlags, VisitOptions};
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that lowers optional chains, sharing the pipeline's
/// emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::optionalchain::new_optional_chain_transformer, TransformOptions};
/// let _tx = new_optional_chain_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/optionalchain.go:newOptionalChainTransformer
pub fn new_optional_chain_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| optional_chain_visit(ec.arena_mut(), node)),
        opt.context.clone(),
    )
}

/// Lowers optional chains in the subtree rooted at `node`.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/optionalchain.go:optionalChainTransformer.visit
fn optional_chain_visit(arena: &mut NodeArena, node: NodeId) -> NodeId {
    if arena.flags(node).contains(NodeFlags::OPTIONAL_CHAIN)
        && matches!(
            arena.kind(node),
            Kind::PropertyAccessExpression | Kind::ElementAccessExpression | Kind::CallExpression
        )
    {
        if let Some(lowered) = try_lower_optional_expression(arena, node) {
            return lowered;
        }
        // Unsupported chain shape (multi-`?.`, non-simple receiver needing a
        // temp, this-capture, delete) -> leave verbatim (DEFER). Do not recurse
        // into it, which would partially lower and break the chain semantics.
        return node;
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| optional_chain_visit(a, c))
}

/// Lowers an optional chain `recv?.…` into a not-null-guarded conditional
/// `recv === null || recv === void 0 ? void 0 : recv.…`, flattening trailing
/// non-optional segments (`a?.b()`, `a?.b.c`) into one guard on the receiver.
///
/// Returns `None` for shapes outside the reachable subset (a receiver that is
/// not a simple-copiable expression and so needs a hoisted temp, a nested
/// optional chain — i.e. multiple `?.` — `this`-capture for parenthesized
/// calls, tagged templates, or `delete`); those are DEFER'd, see
/// `estransforms/mod.rs`.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression
fn try_lower_optional_expression(arena: &mut NodeArena, node: NodeId) -> Option<NodeId> {
    let (receiver, chain) = flatten_chain(arena, node);
    // Multiple `?.` (the flattened base is itself an optional chain) needs a
    // hoisted temp -> deferred.
    if arena.flags(receiver).contains(NodeFlags::OPTIONAL_CHAIN) {
        return None;
    }
    let captured = optional_chain_visit(arena, receiver);
    // A non-simple receiver would be evaluated twice (guard + access) without a
    // hoisted temp -> deferred.
    if !is_simple_copiable(arena, captured) {
        return None;
    }
    let mut right = captured;
    for &segment in &chain {
        right = match arena.kind(segment) {
            Kind::PropertyAccessExpression => {
                let name = match arena.data(segment) {
                    NodeData::PropertyAccessExpression(d) => d.name,
                    _ => return None,
                };
                arena.new_property_access_expression(right, None, name)
            }
            Kind::ElementAccessExpression => {
                let argument = match arena.data(segment) {
                    NodeData::ElementAccessExpression(d) => d.argument_expression,
                    _ => return None,
                };
                let argument = optional_chain_visit(arena, argument);
                arena.new_element_access_expression(right, None, argument)
            }
            Kind::CallExpression => {
                let arguments = match arena.data(segment) {
                    NodeData::CallExpression(d) => d.arguments.clone(),
                    _ => return None,
                };
                let arguments = visit_argument_list(arena, &arguments);
                arena.new_call_expression(right, None, None, arguments, NodeFlags::NONE)
            }
            // Tagged templates / other trailing segments -> deferred.
            _ => return None,
        };
    }
    let condition = create_not_null_condition(arena, captured, captured);
    let void_zero = make_void_zero(arena);
    let question = arena.new_token(Kind::QuestionToken);
    let colon = arena.new_token(Kind::ColonToken);
    Some(arena.new_conditional_expression(condition, question, void_zero, colon, right))
}

/// Flattens an optional chain into its receiver expression (the operand before
/// the first `?.`) and the ordered list of access/call segments above it.
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/estransforms/optionalchain.go:flattenChain
fn flatten_chain(arena: &NodeArena, node: NodeId) -> (NodeId, Vec<NodeId>) {
    let mut chain = node;
    let mut links = vec![chain];
    while question_dot_token(arena, chain).is_none() {
        let Some(expression) = segment_expression(arena, chain) else {
            break;
        };
        chain = expression;
        links.insert(0, chain);
    }
    let receiver = segment_expression(arena, chain).unwrap_or(chain);
    (receiver, links)
}

/// Returns the `?.` token of an access/call segment, if present.
///
/// Side effects: none (reads the arena).
fn question_dot_token(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::PropertyAccessExpression(d) => d.question_dot_token,
        NodeData::ElementAccessExpression(d) => d.question_dot_token,
        NodeData::CallExpression(d) => d.question_dot_token,
        _ => None,
    }
}

/// Returns the operand expression of an access/call segment.
///
/// Side effects: none (reads the arena).
fn segment_expression(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::PropertyAccessExpression(d) => Some(d.expression),
        NodeData::ElementAccessExpression(d) => Some(d.expression),
        NodeData::CallExpression(d) => Some(d.expression),
        _ => None,
    }
}

/// Reports whether `node` is a simple, side-effect-free expression that can be
/// duplicated (guard + access) without a temp.
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/utilities.go:IsSimpleCopiableExpression
fn is_simple_copiable(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.kind(node),
        Kind::Identifier
            | Kind::NumericLiteral
            | Kind::StringLiteral
            | Kind::BigIntLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::ThisKeyword
            | Kind::SuperKeyword
            | Kind::TrueKeyword
            | Kind::FalseKeyword
            | Kind::NullKeyword
    )
}

/// Visits each argument of a call segment, lowering nested optional chains.
///
/// Side effects: may push rebuilt nodes onto the arena.
fn visit_argument_list(
    arena: &mut NodeArena,
    arguments: &tsgo_ast::NodeList,
) -> tsgo_ast::NodeList {
    let visited = arguments
        .nodes
        .iter()
        .map(|&arg| optional_chain_visit(arena, arg))
        .collect();
    tsgo_ast::NodeList::new(visited)
}

/// Builds `left === null || right === void 0` — the inverted not-null guard used
/// by an optional chain's conditional. (Port of the `invert == true` arm of
/// Go's `createNotNullCondition`; the non-inverted form is not yet needed.)
///
/// Side effects: pushes the comparison/keyword/literal nodes onto the arena.
// Go: internal/transformers/estransforms/utilities.go:createNotNullCondition
fn create_not_null_condition(arena: &mut NodeArena, left: NodeId, right: NodeId) -> NodeId {
    let null_keyword = arena.new_keyword_expression(Kind::NullKeyword);
    let eq_null = arena.new_token(Kind::EqualsEqualsEqualsToken);
    let left_check = arena.new_binary_expression(left, eq_null, null_keyword);
    let void_zero = make_void_zero(arena);
    let eq_void = arena.new_token(Kind::EqualsEqualsEqualsToken);
    let right_check = arena.new_binary_expression(right, eq_void, void_zero);
    let or = arena.new_token(Kind::BarBarToken);
    arena.new_binary_expression(left_check, or, right_check)
}

/// Builds the `void 0` expression.
///
/// Side effects: pushes the literal/void nodes onto the arena.
// Go: internal/printer/factory.go:NodeFactory.NewVoidZeroExpression
fn make_void_zero(arena: &mut NodeArena) -> NodeId {
    let zero = arena.new_numeric_literal("0", TokenFlags::NONE);
    arena.new_void_expression(zero)
}

#[cfg(test)]
#[path = "optionalchain_test.rs"]
mod tests;
