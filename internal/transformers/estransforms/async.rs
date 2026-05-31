//! Port of Go `internal/transformers/estransforms/async.go`: down-levels
//! `async`/`await` to a generator driven by the `__awaiter` helper.
//!
//! # Scope (rounds 6d-3 + 6m)
//!
//! 6d-3 lowered a top-level **async function declaration** to the `__awaiter`
//! wrapper: `async function f() { await g(); }` →
//! `function f() { return __awaiter(this, void 0, void 0, function* () { yield g(); }); }`,
//! converting `await X` → `yield X` in the (direct) function body and requesting
//! the `__awaiter` helper (whose definition is emitted in the module prologue).
//!
//! 6m extends that lowering, via an emit-context-threaded `VisitEachChild`
//! ([`visit_each_child_ec`]) that reaches functions nested through container
//! nodes, to:
//!
//! - **async function expressions** (`const f = async function () { await x; };`)
//!   — same `{ return __awaiter(this, …) }` block; the function-expression scope
//!   has lexical `this`.
//! - **async methods** (`class C { async m() { await x; } }`) — method body
//!   becomes the same wrapper; the method scope has lexical `this`.
//! - **async arrows** (`const f = async () => { await x; };`) — a concise-body
//!   arrow returning the `__awaiter(…)` call directly. An arrow's `this` is
//!   lexical; at module top there is no lexical `this`, so the first argument is
//!   `void 0` (Go's arrow case does not set `asyncContextHasLexicalThis`).
//!
//! Deferred (DEFER(P5), see `estransforms/mod.rs`): async **accessors**, async
//! **generators** (`__asyncGenerator`), `super` in async methods (needs a
//! `_super` binding), threading `asyncContextHasLexicalThis` through nested
//! scopes (an arrow inside an async method should thread `this`),
//! lexical-`arguments`/`_this` capture, parameter-list transformation
//! (default/rest params), `for await`, top-level `await`, and `await using`.
//! blocked-by: the `EmitContext` super-capture + parameter machinery and the
//! async-generator helpers are not yet ported.

use crate::{new_transformer, TransformOptions, Transformer};
use rustc_hash::FxHashMap;
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

/// Emit-context-threaded visit: lowers async functions (declarations,
/// expressions, methods, arrows), recursing through container nodes via
/// [`visit_each_child_ec`], and at the source-file boundary attaches requested
/// helpers.
///
/// Side effects: may push rebuilt nodes; may request/attach emit helpers.
// Go: internal/transformers/estransforms/async.go:asyncTransformer.visit
fn async_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::SourceFile => visit_source_file(ec, node),
        Kind::FunctionDeclaration if is_async_function_declaration(ec.arena(), node) => {
            visit_async_function_declaration(ec, node)
        }
        Kind::FunctionExpression if is_async_function_expression(ec.arena(), node) => {
            visit_async_function_expression(ec, node)
        }
        Kind::MethodDeclaration if is_async_method(ec.arena(), node) => {
            visit_async_method_declaration(ec, node)
        }
        Kind::ArrowFunction if is_async_arrow(ec.arena(), node) => {
            visit_async_arrow_function(ec, node)
        }
        _ => visit_each_child_ec(ec, node),
    }
}

/// Emit-context-threaded `VisitEachChild`: recursively runs [`async_visit`] over
/// every child (so a nested async function reachable through container nodes —
/// `const f = async function () {…}`, a class body, etc. — is lowered and can
/// request the `__awaiter` helper), then rebuilds the node with the transformed
/// children. The node is returned unchanged when no child changed, preserving
/// its original source positions.
///
/// Side effects: may push rebuilt nodes; may request/attach emit helpers
/// through the recursive visits.
// Go: internal/transformers/estransforms/async.go:asyncTransformer.visit (default: VisitEachChild)
fn visit_each_child_ec(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let mut children = Vec::new();
    ec.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    let mut replacements: FxHashMap<NodeId, NodeId> = FxHashMap::default();
    for child in children {
        let transformed = async_visit(ec, child);
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

/// Lowers `async function (...) { ... }` (a function *expression*) to a
/// non-async function expression whose body is
/// `{ return __awaiter(this, void 0, void 0, function* () { ... }); }`. A
/// function expression has its own `this`, so it is threaded as the first
/// `__awaiter` argument (same shape as a declaration).
///
/// Side effects: pushes rebuilt nodes; requests the `__awaiter` helper.
// Go: internal/transformers/estransforms/async.go:asyncTransformer.visitFunctionExpression
fn visit_async_function_expression(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (modifiers, name, parameters, body) = match ec.arena().data(node) {
        NodeData::FunctionExpression(d) => {
            (d.modifiers.clone(), d.name, d.parameters.clone(), d.body)
        }
        _ => unreachable!("kind/data mismatch"),
    };
    let modifiers = strip_async_modifier(ec.arena(), modifiers);
    let awaiter = build_awaiter_wrapper_body(ec, body);
    ec.arena_mut().new_function_expression(
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

/// Lowers an `async m() { ... }` class method to a method whose body is
/// `{ return __awaiter(this, void 0, void 0, function* () { ... }); }`. A method
/// has lexical `this`, threaded as the first `__awaiter` argument.
///
/// Side effects: pushes rebuilt nodes; requests the `__awaiter` helper.
// Go: internal/transformers/estransforms/async.go:asyncTransformer.visitMethodDeclaration
fn visit_async_method_declaration(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (modifiers, name, postfix_token, parameters, body) = match ec.arena().data(node) {
        NodeData::MethodDeclaration(d) => (
            d.modifiers.clone(),
            d.name,
            d.postfix_token,
            d.parameters.clone(),
            d.body,
        ),
        _ => unreachable!("kind/data mismatch"),
    };
    let modifiers = strip_async_modifier(ec.arena(), modifiers);
    let awaiter = build_awaiter_wrapper_body(ec, body);
    ec.arena_mut().new_method_declaration(
        modifiers,
        None, // asterisk_token
        name,
        postfix_token,
        None, // type_parameters
        parameters,
        None, // type_node
        None, // full_signature
        Some(awaiter),
    )
}

/// Lowers `async (...) => { ... }` to a concise-body arrow whose body is the
/// `__awaiter(<thisArg>, void 0, void 0, function* () { ... })` call directly
/// (no `{ return ...; }` wrapper).
///
/// An arrow's `this` is lexical, so the first `__awaiter` argument is the
/// enclosing scope's `this`. At module top there is no lexical `this`, so it is
/// `void 0` (mirroring Go, where the arrow case does not set
/// `asyncContextHasLexicalThis` and thus inherits the top-level `false`).
///
/// Side effects: pushes rebuilt nodes; requests the `__awaiter` helper.
// Go: internal/transformers/estransforms/async.go:asyncTransformer.visitArrowFunction
fn visit_async_arrow_function(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (modifiers, parameters, equals_greater_than_token, body) = match ec.arena().data(node) {
        NodeData::ArrowFunction(d) => (
            d.modifiers.clone(),
            d.parameters.clone(),
            d.equals_greater_than_token,
            d.body,
        ),
        _ => unreachable!("kind/data mismatch"),
    };
    let modifiers = strip_async_modifier(ec.arena(), modifiers);
    // DEFER: threading `asyncContextHasLexicalThis` through enclosing non-arrow
    // async scopes (an arrow nested in an async method would thread `this`).
    // The reachable top-level subset has no lexical `this` -> `void 0`.
    let awaiter_call = build_awaiter_call(ec, Some(body), false);
    ec.arena_mut().new_arrow_function(
        modifiers,
        None, // type_parameters
        parameters,
        None, // type_node
        None, // full_signature
        equals_greater_than_token,
        awaiter_call,
    )
}

/// Builds the `{ return __awaiter(this, void 0, void 0, function* () { <body> }); }`
/// block used as the lowered body of a non-arrow async function
/// (declaration / expression / method). These scopes have lexical `this`, so it
/// is threaded as the first `__awaiter` argument.
///
/// Side effects: pushes rebuilt nodes; requests the `__awaiter` helper.
// Go: internal/transformers/estransforms/async.go:transformAsyncFunctionBody (non-arrow branch)
fn build_awaiter_wrapper_body(ec: &mut EmitContext, body: Option<NodeId>) -> NodeId {
    let call = build_awaiter_call(ec, body, true);
    let return_statement = ec.arena_mut().new_return_statement(Some(call));
    ec.arena_mut()
        .new_block(NodeList::new(vec![return_statement]))
}

/// Builds the `__awaiter(<thisArg>, void 0, void 0, function* () { <body> })`
/// call expression for a lowered async function. `await X` is converted to
/// `yield X` in the generator body. `has_lexical_this` selects the first
/// argument: `this` for scopes with their own `this` (functions / methods),
/// `void 0` for an arrow whose enclosing scope has no lexical `this`.
///
/// Side effects: pushes rebuilt nodes; requests the `__awaiter` helper.
// Go: internal/printer/factory.go:NodeFactory.NewAwaiterHelper
fn build_awaiter_call(
    ec: &mut EmitContext,
    body: Option<NodeId>,
    has_lexical_this: bool,
) -> NodeId {
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
    let this_arg = if has_lexical_this {
        ec.arena_mut().new_keyword_expression(Kind::ThisKeyword)
    } else {
        make_void_zero(ec.arena_mut())
    };
    let void_zero_arguments = make_void_zero(ec.arena_mut());
    let void_zero_generators = make_void_zero(ec.arena_mut());
    ec.arena_mut().new_call_expression(
        awaiter,
        None,
        None,
        NodeList::new(vec![
            this_arg,
            void_zero_arguments,
            void_zero_generators,
            generator,
        ]),
        NodeFlags::NONE,
    )
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
fn is_async_function_declaration(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.data(node),
        NodeData::FunctionDeclaration(d)
            if d.asterisk_token.is_none() && has_async_modifier(&d.modifiers)
    )
}

/// Reports whether a function *expression* is an `async` (non-generator)
/// function, with the same async-generator guard as
/// [`is_async_function_declaration`].
///
/// Side effects: none (reads the arena).
fn is_async_function_expression(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.data(node),
        NodeData::FunctionExpression(d)
            if d.asterisk_token.is_none() && has_async_modifier(&d.modifiers)
    )
}

/// Reports whether a method declaration is an `async` (non-generator) method,
/// with the same async-generator guard as [`is_async_function_declaration`].
///
/// Side effects: none (reads the arena).
fn is_async_method(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.data(node),
        NodeData::MethodDeclaration(d)
            if d.asterisk_token.is_none() && has_async_modifier(&d.modifiers)
    )
}

/// Reports whether an arrow function is `async`. Arrows are never generators, so
/// no asterisk guard is needed.
///
/// Side effects: none (reads the arena).
fn is_async_arrow(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.data(node),
        NodeData::ArrowFunction(d) if has_async_modifier(&d.modifiers)
    )
}

/// Reports whether a modifier list carries the `async` modifier.
///
/// Side effects: none (pure).
fn has_async_modifier(modifiers: &Option<ModifierList>) -> bool {
    modifiers
        .as_ref()
        .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::ASYNC))
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
