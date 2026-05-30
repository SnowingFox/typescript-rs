//! Port of Go `internal/transformers/estransforms/async.go`: down-levels
//! `async`/`await` to a generator driven by the `__awaiter` helper.
//!
//! # Scope (round 6d-3)
//!
//! Lowers a top-level **async function declaration** to the `__awaiter` wrapper:
//! `async function f() { await g(); }` →
//! `function f() { return __awaiter(this, void 0, void 0, function* () { yield g(); }); }`,
//! converting `await X` → `yield X` in the (direct) function body and requesting
//! the `__awaiter` helper (whose definition is emitted in the module prologue).
//!
//! Deferred (DEFER(P5), see `estransforms/mod.rs`): async **methods**/
//! **accessors**/**arrow functions**, async **generators** (`__asyncGenerator`),
//! `super`/lexical-`this`/`arguments` capture, parameter-list transformation
//! (default/rest params), top-level `await`, and `await using`. blocked-by: the
//! `EmitContext` super-capture + parameter/variable-environment machinery and
//! the async-generator helpers are not yet ported.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{
    Kind, ModifierFlags, ModifierList, NodeArena, NodeData, NodeFlags, NodeId, NodeList,
    TokenFlags, VisitOptions,
};
use tsgo_printer::emithelpers::AWAITER_HELPER;
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that lowers `async`/`await`, sharing the pipeline's
/// emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::r#async::new_async_transformer, TransformOptions};
/// let _tx = new_async_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/async.go:newAsyncTransformer
pub fn new_async_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| async_visit(ec, node)),
        opt.context.clone(),
    )
}

/// Emit-context-threaded visit: lowers async function declarations and, at the
/// source-file boundary, attaches requested helpers.
///
/// Side effects: may push rebuilt nodes; may request/attach emit helpers.
// Go: internal/transformers/estransforms/async.go:asyncTransformer.visit
fn async_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::SourceFile => visit_source_file(ec, node),
        Kind::FunctionDeclaration if is_async(ec.arena(), node) => {
            visit_async_function_declaration(ec, node)
        }
        _ => {
            // Deeper nodes are not on the tracer's async path; recurse
            // structurally (arena-only) to preserve them.
            let opts = VisitOptions {
                synthetic_location: false,
                clone_lists: false,
            };
            ec.arena_mut().visit_each_child(node, opts, &mut |_, c| c)
        }
    }
}

/// Visits the source file's statements, then attaches the helpers requested
/// during the visit so the printer emits them in the prologue.
///
/// Side effects: rebuilds the source file; attaches emit helpers.
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
    let visited: Vec<NodeId> = statements
        .nodes
        .iter()
        .copied()
        .map(|s| async_visit(ec, s))
        .collect();
    let new_source_file = ec.arena_mut().new_source_file(
        &file_name,
        script_kind,
        language_variant,
        NodeList::new(visited),
        end_of_file_token,
    );
    for helper in ec.read_emit_helpers() {
        ec.add_emit_helper(new_source_file, helper);
    }
    new_source_file
}

/// Lowers `async function f(...) { ... }` to a non-async function whose body is
/// `return __awaiter(this, void 0, void 0, function* () { ... });`.
///
/// Side effects: pushes rebuilt nodes; requests the `__awaiter` helper.
// Go: internal/transformers/estransforms/async.go:asyncTransformer.visitFunctionDeclaration
fn visit_async_function_declaration(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (modifiers, name, parameters, body) = match ec.arena().data(node) {
        NodeData::FunctionDeclaration(d) => {
            (d.modifiers.clone(), d.name, d.parameters.clone(), d.body)
        }
        _ => unreachable!("kind/data mismatch"),
    };
    let modifiers = strip_async_modifier(ec.arena(), modifiers);
    let awaiter = build_awaiter_wrapper_body(ec, body);
    ec.arena_mut().new_function_declaration(
        modifiers,
        None, // asterisk_token
        name,
        None, // type_parameters
        parameters,
        None, // type_node
        None, // full_signature
        Some(awaiter),
    )
}

/// Builds the `{ return __awaiter(this, void 0, void 0, function* () { <body> }); }`
/// block for a lowered async function. `await X` is converted to `yield X` in
/// the body.
///
/// Side effects: pushes rebuilt nodes; requests the `__awaiter` helper.
// Go: internal/transformers/estransforms/async.go:transformAsyncFunctionBody
fn build_awaiter_wrapper_body(ec: &mut EmitContext, body: Option<NodeId>) -> NodeId {
    let body_statements = match body.map(|b| ec.arena().data(b)) {
        Some(NodeData::Block(d)) => d.list.nodes.clone(),
        _ => Vec::new(),
    };
    let generator_statements: Vec<NodeId> = body_statements
        .iter()
        .copied()
        .map(|s| convert_await_to_yield(ec.arena_mut(), s))
        .collect();
    let generator_body = ec
        .arena_mut()
        .new_block(NodeList::new(generator_statements));
    let asterisk = ec.arena_mut().new_token(Kind::AsteriskToken);
    let generator = ec.arena_mut().new_function_expression(
        None,
        Some(asterisk),
        None,
        None,
        NodeList::new(vec![]),
        None,
        None,
        Some(generator_body),
    );
    ec.request_emit_helper(&AWAITER_HELPER);
    let awaiter = ec.factory().new_unscoped_helper_name("__awaiter");
    let this = ec.arena_mut().new_keyword_expression(Kind::ThisKeyword);
    let void_zero_arguments = make_void_zero(ec.arena_mut());
    let void_zero_generators = make_void_zero(ec.arena_mut());
    let call = ec.arena_mut().new_call_expression(
        awaiter,
        None,
        None,
        NodeList::new(vec![
            this,
            void_zero_arguments,
            void_zero_generators,
            generator,
        ]),
        NodeFlags::NONE,
    );
    let return_statement = ec.arena_mut().new_return_statement(Some(call));
    ec.arena_mut()
        .new_block(NodeList::new(vec![return_statement]))
}

/// Recursively rewrites `await X` → `yield X` within a function body, without
/// descending into nested function-like scopes (which have their own async
/// context).
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/estransforms/async.go:visitAwaitExpression
fn convert_await_to_yield(arena: &mut NodeArena, node: NodeId) -> NodeId {
    match arena.kind(node) {
        Kind::AwaitExpression => {
            let expression = match arena.data(node) {
                NodeData::AwaitExpression(d) => d.expression,
                _ => unreachable!("kind checked above"),
            };
            let expression = convert_await_to_yield(arena, expression);
            arena.new_yield_expression(None, Some(expression))
        }
        // A nested function-like scope is its own `this`/async boundary; leave it.
        Kind::FunctionDeclaration
        | Kind::FunctionExpression
        | Kind::ArrowFunction
        | Kind::MethodDeclaration
        | Kind::GetAccessor
        | Kind::SetAccessor
        | Kind::Constructor
        | Kind::ClassDeclaration
        | Kind::ClassExpression => node,
        _ => {
            let opts = VisitOptions {
                synthetic_location: false,
                clone_lists: false,
            };
            arena.visit_each_child(node, opts, &mut |a, c| convert_await_to_yield(a, c))
        }
    }
}

/// Reports whether a function declaration is an `async` (non-generator)
/// function. Async **generators** (`async function*`) need the
/// `__asyncGenerator` helper and are deferred, so they are excluded here and
/// pass through unchanged.
///
/// Side effects: none (reads the arena).
fn is_async(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.data(node),
        NodeData::FunctionDeclaration(d)
            if d.asterisk_token.is_none()
                && d.modifiers.as_ref().is_some_and(|m| m.modifier_flags.contains(ModifierFlags::ASYNC))
    )
}

/// Drops the `async` keyword from a modifier list, returning `None` when no
/// modifiers remain.
///
/// Side effects: none (reads the arena; builds a value).
fn strip_async_modifier(
    arena: &NodeArena,
    modifiers: Option<ModifierList>,
) -> Option<ModifierList> {
    let modifiers = modifiers?;
    let kept: Vec<NodeId> = modifiers
        .list
        .nodes
        .iter()
        .copied()
        .filter(|&n| arena.kind(n) != Kind::AsyncKeyword)
        .collect();
    if kept.is_empty() {
        return None;
    }
    Some(ModifierList {
        list: NodeList::new(kept),
        modifier_flags: modifiers.modifier_flags & !ModifierFlags::ASYNC,
    })
}

/// Builds the `void 0` expression.
///
/// Side effects: pushes the literal/void nodes onto the arena.
fn make_void_zero(arena: &mut NodeArena) -> NodeId {
    let zero = arena.new_numeric_literal("0", TokenFlags::NONE);
    arena.new_void_expression(zero)
}

#[cfg(test)]
#[path = "async_test.rs"]
mod tests;
