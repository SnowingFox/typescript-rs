//! Port of Go `internal/transformers/estransforms/optionalchain.go`: lowers
//! ES2020 optional chains (`a?.b`, `a?.[x]`, `a?.()`) to conditional
//! expressions guarded by a not-null check.
//!
//! # Scope (rounds 6d + 6h + 6i)
//!
//! 6d lowered single-`?.` chains with a *simple-copiable* receiver
//! (`a?.b` / `a?.[x]` / `a?.()` / `a?.b()` / `a?.b.c`). 6h deepened this with
//! **receiver temp-hoisting** (`f()?.b` → `var _a; (_a = f()) === null ||
//! _a === void 0 ? void 0 : _a.b`) and **multiple `?.` in a chain**
//! (`a?.b?.c`), reusing the [`EmitContext`](tsgo_printer::EmitContext) variable
//! environment established in round 6c-3.
//!
//! 6i wires **per-scope variable environments**: the emit context is now also
//! threaded through function-like bodies (function declarations / expressions,
//! arrow bodies, class methods), each opening its own variable environment
//! around the body (mirroring Go's `VisitFunctionBody`). A temp-hoisting chain
//! inside such a body now lands in *that scope's* leading `var ...;`, not at
//! module top — and a concise arrow body is wrapped into a block once a temp is
//! hoisted.
//!
//! Descent into nodes still *not* on a threaded path (e.g. chains nested inside
//! control-flow statement bodies, `switch` cases, or object-literal method
//! shorthands) remains arena-only and continues to defer temp-hoisting there.
//! See `estransforms/mod.rs` for the remaining DEFER list.

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
        Kind::FunctionDeclaration => visit_function_declaration(ec, node),
        Kind::FunctionExpression => visit_function_expression(ec, node),
        Kind::ArrowFunction => visit_arrow_function(ec, node),
        Kind::ClassDeclaration => visit_class_declaration(ec, node),
        Kind::MethodDeclaration => visit_method_declaration(ec, node),
        Kind::ExpressionStatement => {
            let expression = match ec.arena().data(node) {
                NodeData::ExpressionStatement(d) => d.expression,
                _ => unreachable!("kind/data mismatch"),
            };
            let expression = optional_chain_visit(ec, expression);
            ec.arena_mut().new_expression_statement(expression)
        }
        Kind::ReturnStatement => {
            let expression = match ec.arena().data(node) {
                NodeData::ReturnStatement(d) => d.expression,
                _ => unreachable!("kind/data mismatch"),
            };
            let expression = expression.map(|e| optional_chain_visit(ec, e));
            ec.arena_mut().new_return_statement(expression)
        }
        Kind::PropertyAccessExpression | Kind::ElementAccessExpression | Kind::CallExpression
            if ec.arena().flags(node).contains(NodeFlags::OPTIONAL_CHAIN) =>
        {
            // Unsupported chain shapes (see `lower_optional_expression`) are left
            // verbatim (DEFER); recursing into them would partially lower and
            // break the chain semantics.
            lower_optional_expression(ec, node, false, false).unwrap_or(node)
        }
        Kind::CallExpression => visit_call_expression(ec, node),
        Kind::DeleteExpression => visit_delete_expression(ec, node),
        _ => {
            // Nodes not on a threaded path (control-flow statement bodies,
            // object-literal method shorthands, ...) recurse arena-only: simple
            // chains are still lowered, but temp-hoisting chains nested here are
            // deferred (no variable environment is active to receive their
            // `var`). Function-like bodies are handled by the arms above.
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

/// Rebuilds a function declaration, visiting its body inside its own variable
/// environment so chains lowered in the body hoist their receiver temporaries
/// into that scope (round 6i), not at module top.
///
/// Side effects: pushes/pops a variable environment; rebuilds the function.
// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody
fn visit_function_declaration(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (
        modifiers,
        asterisk_token,
        name,
        type_parameters,
        parameters,
        type_node,
        full_signature,
        body,
    ) = match ec.arena().data(node) {
        NodeData::FunctionDeclaration(d) => (
            d.modifiers.clone(),
            d.asterisk_token,
            d.name,
            d.type_parameters.clone(),
            d.parameters.clone(),
            d.type_node,
            d.full_signature,
            d.body,
        ),
        _ => unreachable!("kind/data mismatch"),
    };
    let body = visit_function_body(ec, body);
    ec.arena_mut().new_function_declaration(
        modifiers,
        asterisk_token,
        name,
        type_parameters,
        parameters,
        type_node,
        full_signature,
        body,
    )
}

/// Rebuilds a function expression, visiting its body inside its own variable
/// environment (round 6i).
///
/// Side effects: pushes/pops a variable environment; rebuilds the function.
// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody
fn visit_function_expression(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (
        modifiers,
        asterisk_token,
        name,
        type_parameters,
        parameters,
        type_node,
        full_signature,
        body,
    ) = match ec.arena().data(node) {
        NodeData::FunctionExpression(d) => (
            d.modifiers.clone(),
            d.asterisk_token,
            d.name,
            d.type_parameters.clone(),
            d.parameters.clone(),
            d.type_node,
            d.full_signature,
            d.body,
        ),
        _ => unreachable!("kind/data mismatch"),
    };
    let body = visit_function_body(ec, body);
    ec.arena_mut().new_function_expression(
        modifiers,
        asterisk_token,
        name,
        type_parameters,
        parameters,
        type_node,
        full_signature,
        body,
    )
}

/// Rebuilds an arrow function, visiting its body inside its own variable
/// environment. A concise expression body that hoists a temp is wrapped into a
/// block (`{ var _a; return <lowered>; }`), mirroring Go's `VisitFunctionBody`.
///
/// Side effects: pushes/pops a variable environment; rebuilds the arrow.
// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody
fn visit_arrow_function(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (
        modifiers,
        type_parameters,
        parameters,
        type_node,
        full_signature,
        equals_greater_than_token,
        body,
    ) = match ec.arena().data(node) {
        NodeData::ArrowFunction(d) => (
            d.modifiers.clone(),
            d.type_parameters.clone(),
            d.parameters.clone(),
            d.type_node,
            d.full_signature,
            d.equals_greater_than_token,
            d.body,
        ),
        _ => unreachable!("kind/data mismatch"),
    };
    let body = visit_function_body(ec, Some(body)).expect("arrow function always has a body");
    ec.arena_mut().new_arrow_function(
        modifiers,
        type_parameters,
        parameters,
        type_node,
        full_signature,
        equals_greater_than_token,
        body,
    )
}

/// Rebuilds a class declaration, threading the emit context through its members
/// so a method body's chain temps hoist into that method's own scope (round
/// 6i). Non-method members recurse arena-only via the fallthrough visit.
///
/// Side effects: rebuilds the class; may push/pop per-method variable
/// environments.
// Go: internal/printer/emitcontext.go:EmitContext.NewNodeVisitor (VisitEachChild over members)
fn visit_class_declaration(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (modifiers, name, type_parameters, heritage_clauses, members) = match ec.arena().data(node)
    {
        NodeData::ClassDeclaration(d) => (
            d.modifiers.clone(),
            d.name,
            d.type_parameters.clone(),
            d.heritage_clauses.clone(),
            d.members.clone(),
        ),
        _ => unreachable!("kind/data mismatch"),
    };
    let visited: Vec<NodeId> = members
        .nodes
        .iter()
        .copied()
        .map(|member| optional_chain_visit(ec, member))
        .collect();
    ec.arena_mut().new_class_like(
        Kind::ClassDeclaration,
        modifiers,
        name,
        type_parameters,
        heritage_clauses,
        NodeList::new(visited),
    )
}

/// Rebuilds a method declaration, visiting its body inside its own variable
/// environment (round 6i).
///
/// Side effects: pushes/pops a variable environment; rebuilds the method.
// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody
fn visit_method_declaration(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (
        modifiers,
        asterisk_token,
        name,
        postfix_token,
        type_parameters,
        parameters,
        type_node,
        full_signature,
        body,
    ) = match ec.arena().data(node) {
        NodeData::MethodDeclaration(d) => (
            d.modifiers.clone(),
            d.asterisk_token,
            d.name,
            d.postfix_token,
            d.type_parameters.clone(),
            d.parameters.clone(),
            d.type_node,
            d.full_signature,
            d.body,
        ),
        _ => unreachable!("kind/data mismatch"),
    };
    let body = visit_function_body(ec, body);
    ec.arena_mut().new_method_declaration(
        modifiers,
        asterisk_token,
        name,
        postfix_token,
        type_parameters,
        parameters,
        type_node,
        full_signature,
        body,
    )
}

/// Visits a function-like body within a fresh variable environment, then
/// prepends the hoisted `var ...;` declarations collected during the visit.
///
/// A block body keeps its block shape (declarations prepended to its statement
/// list). A concise arrow expression body is returned unchanged when nothing was
/// hoisted, otherwise wrapped into a block whose final statement returns the
/// lowered expression. Returns `None` for an absent body (e.g. an overload
/// signature).
///
/// Side effects: pushes/pops a variable environment; may rebuild the body.
// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody
fn visit_function_body(ec: &mut EmitContext, body: Option<NodeId>) -> Option<NodeId> {
    let body = body?;
    if let NodeData::Block(d) = ec.arena().data(body) {
        let statements = d.list.nodes.clone();
        ec.start_variable_environment();
        let mut visited = Vec::with_capacity(statements.len());
        for statement in statements {
            visited.push(optional_chain_visit(ec, statement));
        }
        let mut all = ec.end_variable_environment();
        all.extend(visited);
        return Some(ec.arena_mut().new_block(NodeList::new(all)));
    }
    // Concise arrow expression body: lower it inside the environment, then wrap
    // into a block `{ <hoisted>; return <expr>; }` only when a temp was hoisted.
    ec.start_variable_environment();
    let visited = optional_chain_visit(ec, body);
    let mut declarations = ec.end_variable_environment();
    if declarations.is_empty() {
        return Some(visited);
    }
    let return_statement = ec.arena_mut().new_return_statement(Some(visited));
    declarations.push(return_statement);
    Some(ec.arena_mut().new_block(NodeList::new(declarations)))
}

/// Lowers an optional chain `recv?.…` into a not-null-guarded conditional,
/// flattening trailing non-optional segments (`a?.b()`, `a?.b.c`) into one
/// guard on the receiver. A receiver that is not a simple-copiable expression
/// is evaluated once into a hoisted temp (`f()?.b` →
/// `(_a = f()) === null || _a === void 0 ? void 0 : _a.b`).
///
/// When `capture_this_arg` is set (a parenthesized optional call `(a?.b)()`),
/// the final access's receiver is captured and the result is wrapped in a
/// [`Kind::SyntheticReferenceExpression`] carrying that `this` argument so the
/// enclosing call can lower to `<access>.call(thisArg, ...)`. When `is_delete`
/// is set (`delete a?.b`), the guard's "true" branch is `true` and the access
/// branch is `delete <access>`.
///
/// Returns `None` for shapes outside the reachable subset (tagged templates);
/// those are DEFER'd, see `estransforms/mod.rs`.
///
/// Side effects: may push rebuilt nodes; may hoist `var` declarations.
// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression
fn lower_optional_expression(
    ec: &mut EmitContext,
    node: NodeId,
    capture_this_arg: bool,
    is_delete: bool,
) -> Option<NodeId> {
    let (receiver, chain) = flatten_chain(ec.arena(), node);
    // Lower the receiver first. When the first chain segment is a call
    // (`<receiver>?.()`), the receiver is visited in `this`-capturing position
    // so its own receiver becomes the call's `this` (`leftThisArg`). When the
    // receiver is itself an optional chain (`a?.b?.()`), this recursion lowers
    // the inner chain into a conditional carrying the captured `this`, which is
    // then hoisted into a temp below.
    let capture_left_this = is_call_chain(ec.arena(), chain[0]);
    let left = visit_non_optional_expression(ec, receiver, capture_left_this, false);
    // When the receiver captured a `this` argument it returns a
    // `SyntheticReferenceExpression { expression, this_arg }`; unwrap it so the
    // bundled expression flows on and `left_this_arg` threads into the first
    // call segment below.
    let (left_this_arg, captured) = match ec.arena().data(left) {
        NodeData::SyntheticReferenceExpression(d) => (Some(d.this_arg), d.expression),
        _ => (None, left),
    };
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
    let (right, this_arg) =
        build_chain_segments_capturing(ec, captured_left, &chain, capture_this_arg, left_this_arg)?;
    let condition = create_not_null_condition(ec.arena_mut(), left_expression, captured_left);
    let question = ec.arena_mut().new_token(Kind::QuestionToken);
    let colon = ec.arena_mut().new_token(Kind::ColonToken);
    let target = if is_delete {
        // `delete a?.b` -> `... ? true : delete a.b`.
        let when_true = ec.arena_mut().new_keyword_expression(Kind::TrueKeyword);
        let delete = ec.arena_mut().new_delete_expression(right);
        ec.arena_mut()
            .new_conditional_expression(condition, question, when_true, colon, delete)
    } else {
        let void_zero = make_void_zero(ec.arena_mut());
        ec.arena_mut()
            .new_conditional_expression(condition, question, void_zero, colon, right)
    };
    match this_arg {
        Some(this_arg) => Some(
            ec.arena_mut()
                .new_synthetic_reference_expression(target, this_arg),
        ),
        None => Some(target),
    }
}

/// Builds the right-hand access/call sequence of a lowered optional chain on
/// top of `base`, in the emit-context path. When `capture_this_arg` is set, the
/// receiver of the final access segment is captured as the call's `this`
/// argument (hoisting a temp when it is not simple-copiable) and returned
/// alongside the built expression. When `left_this_arg` is `Some` and the first
/// segment is a call (`<receiver>?.()`), that captured `this` is threaded into
/// the call as `<callee>.call(thisArg, ...)` so `this` is preserved. Mirrors
/// the segment loop of Go's `visitOptionalExpression`.
///
/// Returns `None` for a segment kind outside the reachable subset (tagged
/// templates) or a `super` `this` argument -> DEFER.
///
/// Side effects: may push rebuilt nodes; may hoist a `var` declaration.
// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression (segment loop)
fn build_chain_segments_capturing(
    ec: &mut EmitContext,
    base: NodeId,
    chain: &[NodeId],
    capture_this_arg: bool,
    left_this_arg: Option<NodeId>,
) -> Option<(NodeId, Option<NodeId>)> {
    let mut right = base;
    let mut this_arg = None;
    let last = chain.len().checked_sub(1)?;
    for (i, &segment) in chain.iter().enumerate() {
        let kind = ec.arena().kind(segment);
        right = match kind {
            Kind::PropertyAccessExpression | Kind::ElementAccessExpression => {
                if i == last && capture_this_arg {
                    if is_simple_copiable(ec.arena(), right) {
                        this_arg = Some(right);
                    } else {
                        let temp = ec.factory().new_temp_variable();
                        ec.add_variable_declaration(temp);
                        let equals = ec.arena_mut().new_token(Kind::EqualsToken);
                        let assignment = ec.arena_mut().new_binary_expression(temp, equals, right);
                        this_arg = Some(temp);
                        right = assignment;
                    }
                }
                if kind == Kind::PropertyAccessExpression {
                    let name = match ec.arena().data(segment) {
                        NodeData::PropertyAccessExpression(d) => d.name,
                        _ => return None,
                    };
                    ec.arena_mut()
                        .new_property_access_expression(right, None, name)
                } else {
                    let argument = match ec.arena().data(segment) {
                        NodeData::ElementAccessExpression(d) => d.argument_expression,
                        _ => return None,
                    };
                    let argument = optional_chain_visit_arena(ec.arena_mut(), argument);
                    ec.arena_mut()
                        .new_element_access_expression(right, None, argument)
                }
            }
            Kind::CallExpression => {
                let arguments = match ec.arena().data(segment) {
                    NodeData::CallExpression(d) => d.arguments.clone(),
                    _ => return None,
                };
                let arguments = visit_argument_list(ec.arena_mut(), &arguments);
                match left_this_arg {
                    // The first segment is a call whose callee captured a `this`
                    // argument: lower to `<callee>.call(thisArg, ...)`.
                    Some(this) if i == 0 => {
                        let call_this = prepare_call_this_arg(ec, this)?;
                        new_function_call_call(ec, right, call_this, &arguments)
                    }
                    _ => ec.arena_mut().new_call_expression(
                        right,
                        None,
                        None,
                        arguments,
                        NodeFlags::NONE,
                    ),
                }
            }
            // Tagged templates / other trailing segments -> deferred.
            _ => return None,
        };
    }
    Some((right, this_arg))
}

/// Prepares a captured `leftThisArg` for use as a call's `this` argument: a
/// non-auto-generated node is cloned and flagged `NoComments` (so reusing it as
/// `this` does not duplicate the receiver's comments); a generated temp is used
/// as-is. A `super` receiver is DEFER'd (`None`) — it needs the `super` -> `this`
/// rewrite that is not yet ported.
///
/// Side effects: may clone a node and set its emit flags.
// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression (leftThisArg clone)
fn prepare_call_this_arg(ec: &mut EmitContext, this_arg: NodeId) -> Option<NodeId> {
    // `super` as the captured `this` needs `super` -> `this` rewriting (DEFER).
    if ec.arena().kind(this_arg) == Kind::SuperKeyword {
        return None;
    }
    if ec.has_auto_generate_info(this_arg) {
        return Some(this_arg);
    }
    let cloned = ec.arena_mut().clone_node(this_arg);
    let flags = ec.emit_flags(cloned) | tsgo_printer::emitflags::EmitFlags::NO_COMMENTS;
    ec.set_emit_flags(cloned, flags);
    Some(cloned)
}

/// Lowers a non-optional call whose callee is a parenthesized optional chain
/// (`(a?.b)()`): the optional chain is lowered with `this`-capture, and the call
/// becomes `<lowered access>.call(thisArg, ...args)` so `this` is preserved.
/// Other calls fall back to the arena child-visit.
///
/// Side effects: may push rebuilt nodes; may hoist `var` declarations.
// Go: internal/transformers/estransforms/optionalchain.go:visitCallExpression
fn visit_call_expression(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (expression, arguments) = match ec.arena().data(node) {
        NodeData::CallExpression(d) => (d.expression, d.arguments.clone()),
        _ => unreachable!("kind/data mismatch"),
    };
    if ec.arena().kind(expression) == Kind::ParenthesizedExpression
        && skip_parentheses_is_optional_chain(ec.arena(), expression)
    {
        let lowered = visit_parenthesized_expression(ec, expression, true, false);
        let args = visit_argument_list(ec.arena_mut(), &arguments);
        if let NodeData::SyntheticReferenceExpression(d) = ec.arena().data(lowered) {
            let (target, this_arg) = (d.expression, d.this_arg);
            return new_function_call_call(ec, target, this_arg, &args);
        }
        return ec
            .arena_mut()
            .new_call_expression(lowered, None, None, args, NodeFlags::NONE);
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    ec.arena_mut()
        .visit_each_child(node, opts, &mut |a, c| optional_chain_visit_arena(a, c))
}

/// Lowers `delete <optional-chain>`: when the deleted operand is (through any
/// parentheses) an optional chain, the `delete` is pushed into the chain's
/// guard so a nullish base yields `true` and a present base runs the real
/// `delete` (`delete a?.b` -> `a === null || a === void 0 ? true : delete a.b`).
/// Other `delete` operands recurse via the arena child-visit.
///
/// Side effects: may push rebuilt nodes; may hoist `var` declarations.
// Go: internal/transformers/estransforms/optionalchain.go:visitDeleteExpression
fn visit_delete_expression(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let expression = match ec.arena().data(node) {
        NodeData::DeleteExpression(d) => d.expression,
        _ => unreachable!("kind/data mismatch"),
    };
    if skip_parentheses_is_optional_chain(ec.arena(), expression) {
        return visit_non_optional_expression(ec, expression, false, true);
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    ec.arena_mut()
        .visit_each_child(node, opts, &mut |a, c| optional_chain_visit_arena(a, c))
}

/// Visits a parenthesized expression in `this`-capturing / `delete` position,
/// propagating a [`Kind::SyntheticReferenceExpression`] result by re-wrapping
/// its inner expression in parentheses while threading the captured `this`
/// argument.
///
/// Side effects: may push rebuilt nodes; may hoist `var` declarations.
// Go: internal/transformers/estransforms/optionalchain.go:visitParenthesizedExpression
fn visit_parenthesized_expression(
    ec: &mut EmitContext,
    node: NodeId,
    capture_this_arg: bool,
    is_delete: bool,
) -> NodeId {
    let inner = match ec.arena().data(node) {
        NodeData::ParenthesizedExpression(d) => d.expression,
        _ => unreachable!("kind/data mismatch"),
    };
    let visited = visit_non_optional_expression(ec, inner, capture_this_arg, is_delete);
    if let NodeData::SyntheticReferenceExpression(d) = ec.arena().data(visited) {
        let (expr, this_arg) = (d.expression, d.this_arg);
        let paren = ec.arena_mut().new_parenthesized_expression(expr);
        return ec
            .arena_mut()
            .new_synthetic_reference_expression(paren, this_arg);
    }
    ec.arena_mut().new_parenthesized_expression(visited)
}

/// Dispatches a non-optional expression in `this`-capturing / `delete` position
/// to the right handler. An optional access (`a?.b`) is lowered (with
/// `this`-capture and/or `delete` semantics) via [`lower_optional_expression`];
/// other shapes recurse through the normal visit. Only the reachable subset
/// (parenthesized optional access) is wired; other forms fall back to the
/// standard visit.
///
/// Side effects: may push rebuilt nodes; may hoist `var` declarations.
// Go: internal/transformers/estransforms/optionalchain.go:visitNonOptionalExpression
fn visit_non_optional_expression(
    ec: &mut EmitContext,
    node: NodeId,
    capture_this_arg: bool,
    is_delete: bool,
) -> NodeId {
    match ec.arena().kind(node) {
        Kind::ParenthesizedExpression => {
            visit_parenthesized_expression(ec, node, capture_this_arg, is_delete)
        }
        Kind::PropertyAccessExpression | Kind::ElementAccessExpression
            if ec.arena().flags(node).contains(NodeFlags::OPTIONAL_CHAIN) =>
        {
            lower_optional_expression(ec, node, capture_this_arg, is_delete).unwrap_or(node)
        }
        Kind::PropertyAccessExpression | Kind::ElementAccessExpression if capture_this_arg => {
            visit_access_capturing_this(ec, node)
        }
        _ => optional_chain_visit(ec, node),
    }
}

/// Visits a *non-optional* property/element access in `this`-capturing position
/// (the receiver of an optional call, `a.b?.()`): the access's own receiver
/// becomes the call's `this` (hoisted into a temp when not simple-copiable), and
/// the rebuilt access is wrapped in a [`Kind::SyntheticReferenceExpression`]
/// carrying that `this` so the enclosing call can lower to
/// `<access>.call(thisArg, ...)`.
///
/// Side effects: may push rebuilt nodes; may hoist a `var` declaration.
// Go: internal/transformers/estransforms/optionalchain.go:visitPropertyOrElementAccessExpression
// (non-optional branch with captureThisArg)
fn visit_access_capturing_this(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let kind = ec.arena().kind(node);
    let inner = segment_expression(ec.arena(), node).expect("access has a receiver");
    let expression = optional_chain_visit(ec, inner);
    let (expression, this_arg) = if is_simple_copiable(ec.arena(), expression) {
        (expression, expression)
    } else {
        let temp = ec.factory().new_temp_variable();
        ec.add_variable_declaration(temp);
        let equals = ec.arena_mut().new_token(Kind::EqualsToken);
        let assignment = ec
            .arena_mut()
            .new_binary_expression(temp, equals, expression);
        (assignment, temp)
    };
    let rebuilt = if kind == Kind::PropertyAccessExpression {
        let name = match ec.arena().data(node) {
            NodeData::PropertyAccessExpression(d) => d.name,
            _ => unreachable!("kind/data mismatch"),
        };
        ec.arena_mut()
            .new_property_access_expression(expression, None, name)
    } else {
        let argument = match ec.arena().data(node) {
            NodeData::ElementAccessExpression(d) => d.argument_expression,
            _ => unreachable!("kind/data mismatch"),
        };
        let argument = optional_chain_visit_arena(ec.arena_mut(), argument);
        ec.arena_mut()
            .new_element_access_expression(expression, None, argument)
    };
    ec.arena_mut()
        .new_synthetic_reference_expression(rebuilt, this_arg)
}

/// Builds `target.call(thisArg, ...args)` — the `this`-preserving lowering of a
/// call whose callee captured a `this` argument.
///
/// Side effects: pushes the property-access/call nodes onto the arena.
// Go: internal/printer/factory.go:NodeFactory.NewFunctionCallCall
fn new_function_call_call(
    ec: &mut EmitContext,
    target: NodeId,
    this_arg: NodeId,
    args: &NodeList,
) -> NodeId {
    let call_name = ec.arena_mut().new_identifier("call");
    let callee = ec
        .arena_mut()
        .new_property_access_expression(target, None, call_name);
    let mut call_args = Vec::with_capacity(args.nodes.len() + 1);
    call_args.push(this_arg);
    call_args.extend(args.nodes.iter().copied());
    ec.arena_mut().new_call_expression(
        callee,
        None,
        None,
        NodeList::new(call_args),
        NodeFlags::NONE,
    )
}

/// Reports whether `node` is a parenthesized expression wrapping (through any
/// nested parentheses) an optional chain.
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:SkipParentheses + NodeFlagsOptionalChain
fn skip_parentheses_is_optional_chain(arena: &NodeArena, node: NodeId) -> bool {
    let mut current = node;
    while let NodeData::ParenthesizedExpression(d) = arena.data(current) {
        current = d.expression;
    }
    arena.flags(current).contains(NodeFlags::OPTIONAL_CHAIN)
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

/// Reports whether `node` is an optional call segment (a `CallExpression` that
/// is part of an optional chain). Used to decide whether the receiver should be
/// visited in `this`-capturing position (`<receiver>?.()`).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/estransforms/optionalchain.go:isCallChain
fn is_call_chain(arena: &NodeArena, node: NodeId) -> bool {
    arena.kind(node) == Kind::CallExpression
        && arena.flags(node).contains(NodeFlags::OPTIONAL_CHAIN)
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
