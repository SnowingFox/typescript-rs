//! Port of Go `internal/transformers/estransforms/optionalchain.go`: lowers
//! ES2020 optional chains (`a?.b`, `a?.[x]`, `a?.()`) to conditional
//! expressions guarded by a not-null check.
//!
//! # Scope (rounds 6d + 6h)
//!
//! 6d lowered single-`?.` chains with a *simple-copiable* receiver
//! (`a?.b` / `a?.[x]` / `a?.()` / `a?.b()` / `a?.b.c`). 6h deepens this with
//! **receiver temp-hoisting** (`f()?.b` → `var _a; (_a = f()) === null ||
//! _a === void 0 ? void 0 : _a.b`) and **multiple `?.` in a chain**
//! (`a?.b?.c`), reusing the [`EmitContext`](tsgo_printer::EmitContext) variable
//! environment established in round 6c-3.
//!
//! Like `exponentiation.rs`, the emit-context is threaded only through the
//! top-level statement path (`SourceFile` → `ExpressionStatement`) so hoisted
//! temporaries land in a leading `var ...;`; descent into other nodes is
//! arena-only and defers temp-hoisting chains nested in non-top-level scopes
//! (no variable environment is active there yet). See `estransforms/mod.rs` for
//! the full DEFER list.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, NodeList, TokenFlags, VisitOptions};
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
        Box::new(|ec: &mut EmitContext, node: NodeId| optional_chain_visit(ec, node)),
        opt.context.clone(),
    )
}

/// Emit-context-threaded visit: lowers optional chains, hoisting receiver temps
/// into the enclosing variable environment when a chain on the top-level
/// statement path needs them.
///
/// Side effects: may push rebuilt nodes; may hoist `var` declarations.
// Go: internal/transformers/estransforms/optionalchain.go:optionalChainTransformer.visit
fn optional_chain_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::SourceFile => visit_source_file(ec, node),
        Kind::ExpressionStatement => {
            let expression = match ec.arena().data(node) {
                NodeData::ExpressionStatement(d) => d.expression,
                _ => unreachable!("kind/data mismatch"),
            };
            let expression = optional_chain_visit(ec, expression);
            ec.arena_mut().new_expression_statement(expression)
        }
        Kind::PropertyAccessExpression | Kind::ElementAccessExpression | Kind::CallExpression
            if ec.arena().flags(node).contains(NodeFlags::OPTIONAL_CHAIN) =>
        {
            // Unsupported chain shapes (see `lower_optional_expression`) are left
            // verbatim (DEFER); recursing into them would partially lower and
            // break the chain semantics.
            lower_optional_expression(ec, node).unwrap_or(node)
        }
        _ => {
            // Descent into non-top-level nodes is arena-only: simple chains are
            // still lowered, but temp-hoisting chains nested here are deferred
            // (no variable environment is active to receive their `var`).
            let opts = VisitOptions {
                synthetic_location: false,
                clone_lists: false,
            };
            ec.arena_mut()
                .visit_each_child(node, opts, &mut |a, c| optional_chain_visit_arena(a, c))
        }
    }
}

/// Wraps the source file's statements in a variable environment so hoisted
/// receiver temporaries are emitted as a leading `var ...;` statement.
///
/// Side effects: pushes/pops a variable environment; rebuilds the source file.
// Go: internal/printer/emitcontext.go:EmitContext.VisitVariableEnvironment (top-level statements)
fn visit_source_file(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (file_name, script_kind, language_variant, statements, end_of_file_token) =
        match ec.arena().data(node) {
            NodeData::SourceFile(d) => (
                d.file_name.clone(),
                d.script_kind,
                d.language_variant,
                d.statements.clone(),
                d.end_of_file_token,
            ),
            _ => unreachable!("kind/data mismatch"),
        };
    ec.start_variable_environment();
    let mut visited = Vec::with_capacity(statements.nodes.len());
    for &statement in &statements.nodes {
        visited.push(optional_chain_visit(ec, statement));
    }
    let mut all = ec.end_variable_environment();
    all.extend(visited);
    ec.arena_mut().new_source_file(
        &file_name,
        script_kind,
        language_variant,
        NodeList::new(all),
        end_of_file_token,
    )
}

/// Lowers an optional chain `recv?.…` into a not-null-guarded conditional,
/// flattening trailing non-optional segments (`a?.b()`, `a?.b.c`) into one
/// guard on the receiver. A receiver that is not a simple-copiable expression
/// is evaluated once into a hoisted temp (`f()?.b` →
/// `(_a = f()) === null || _a === void 0 ? void 0 : _a.b`).
///
/// Returns `None` for shapes outside the reachable subset (a nested optional
/// chain — i.e. multiple `?.` — `this`-capture for parenthesized calls, tagged
/// templates, or `delete`); those are DEFER'd, see `estransforms/mod.rs`.
///
/// Side effects: may push rebuilt nodes; may hoist a `var` declaration.
// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression
fn lower_optional_expression(ec: &mut EmitContext, node: NodeId) -> Option<NodeId> {
    let (receiver, chain) = flatten_chain(ec.arena(), node);
    // Lower the receiver first. When it is itself an optional chain (multiple
    // `?.`, e.g. `a?.b?.c`), this recursion lowers the inner chain into a
    // conditional, which is then hoisted into a temp below.
    let captured = optional_chain_visit(ec, receiver);
    // A non-simple receiver would be evaluated twice (guard + access); hoist it
    // into a temp so it is evaluated once: the guard tests the assignment
    // `(_a = recv)`, the access reads the temp `_a`.
    let (left_expression, captured_left) = if is_simple_copiable(ec.arena(), captured) {
        (captured, captured)
    } else {
        let temp = ec.factory().new_temp_variable();
        ec.add_variable_declaration(temp);
        let equals = ec.arena_mut().new_token(Kind::EqualsToken);
        let assignment = ec.arena_mut().new_binary_expression(temp, equals, captured);
        (assignment, temp)
    };
    let right = build_chain_segments(ec.arena_mut(), captured_left, &chain)?;
    let condition = create_not_null_condition(ec.arena_mut(), left_expression, captured_left);
    let void_zero = make_void_zero(ec.arena_mut());
    let question = ec.arena_mut().new_token(Kind::QuestionToken);
    let colon = ec.arena_mut().new_token(Kind::ColonToken);
    Some(
        ec.arena_mut()
            .new_conditional_expression(condition, question, void_zero, colon, right),
    )
}

/// Arena-only lowering used for descent into nested scopes: handles simple
/// chains, but defers any chain whose receiver needs a hoisted temp (no
/// variable environment is active to receive its `var`).
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/optionalchain.go:optionalChainTransformer.visit
fn optional_chain_visit_arena(arena: &mut NodeArena, node: NodeId) -> NodeId {
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
    arena.visit_each_child(node, opts, &mut |a, c| optional_chain_visit_arena(a, c))
}

/// Arena-only optional-chain lowering (no temp hoisting): lowers a chain only
/// when its receiver is a simple-copiable expression, returning `None` for any
/// shape that would need a hoisted temp or is otherwise out of the reachable
/// subset.
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
    let captured = optional_chain_visit_arena(arena, receiver);
    // A non-simple receiver would be evaluated twice (guard + access) without a
    // hoisted temp -> deferred (the temp-hoisting path needs the emit context).
    if !is_simple_copiable(arena, captured) {
        return None;
    }
    let right = build_chain_segments(arena, captured, &chain)?;
    let condition = create_not_null_condition(arena, captured, captured);
    let void_zero = make_void_zero(arena);
    let question = arena.new_token(Kind::QuestionToken);
    let colon = arena.new_token(Kind::ColonToken);
    Some(arena.new_conditional_expression(condition, question, void_zero, colon, right))
}

/// Builds the right-hand access/call sequence of a lowered optional chain on
/// top of `base` (the simple receiver or its hoisted temp), lowering nested
/// optional chains in element-access arguments / call arguments.
///
/// Returns `None` for a segment kind outside the reachable subset (e.g. tagged
/// templates) -> DEFER.
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression (segment loop)
fn build_chain_segments(arena: &mut NodeArena, base: NodeId, chain: &[NodeId]) -> Option<NodeId> {
    let mut right = base;
    for &segment in chain {
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
                let argument = optional_chain_visit_arena(arena, argument);
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
    Some(right)
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
        .map(|&arg| optional_chain_visit_arena(arena, arg))
        .collect();
    tsgo_ast::NodeList::new(visited)
}

/// Builds `left === null || right === void 0` — the inverted not-null guard used
/// by an optional chain's conditional. (Port of the `invert == true` arm of
/// Go's `createNotNullCondition`; the non-inverted form is not yet needed.)
///
/// `left` is the (possibly temp-assigning) receiver expression and `right` is
/// the value re-read by the access — the same node when the receiver is simple,
/// the temp when it was hoisted.
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
