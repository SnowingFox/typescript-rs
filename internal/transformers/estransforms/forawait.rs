//! Port of Go `internal/transformers/estransforms/forawait.go`: the ES2018
//! down-leveling stage that lowers **async generators** (and, in Go, `for await`
//! loops). This stage runs before the ES2017 `async` stage; it rewrites an
//! `async function*` into a plain function returning an `__asyncGenerator(this,
//! arguments, function* () { ... })` call, and inside that generator converts
//! `await x` → `yield __await(x)`, `yield e` → `yield yield __await(e)`,
//! `yield* e` → `yield __await(yield* __asyncDelegator(__asyncValues(e)))`, and
//! `return e` → `return yield __await(e)`.
//!
//! # Scope (rounds 6y + 6z)
//!
//! 6y lands the **async-generator function declaration** lowering: the
//! `__asyncGenerator`/`__await`/`__asyncDelegator`/`__asyncValues` helper
//! definitions (defined here because the `tsgo_printer` crate is out of this
//! round's edit scope) plus the function-declaration lowering verified against
//! Go and `tsc --target es2017`.
//!
//! 6z lands the **`for await (x of y)` downlevel** inside an async (non-
//! generator) function: the full async-iteration scaffold — an
//! `__asyncValues(<expr>)` iterator temp, a `result` temp, a C-style `for` whose
//! condition is `result = await iterator.next(), done = result.done, !done`, the
//! loop variable bound from `result.value`, and the `try/catch/finally` with the
//! `iterator.return` cleanup (down-level `await`, since the enclosing function is
//! async but not a generator). The `done`/`errorRecord`/`returnMethod`/`value`
//! temporaries hoist into the enclosing function body via its variable
//! environment. Verified against `tsc --target es2017` (where `async`/`await`
//! stay native and only the ES2018 `for await` downlevels).
//!
//! Deferred (DEFER, see `estransforms/mod.rs`): `for await` with an **identifier
//! source** (`for await (const x of y)`), which derives the iterator/result
//! names from the source identifier and needs the printer's resolving
//! `getTextOfNode` for the nested generated name (not ported); `for await`
//! inside an **async generator** (needs `createDownlevelAwait`'s generator form
//! `yield __await(...)`, i.e. enclosing-function-flags threading); **destructuring**
//! loop variables, **top-level** `for await`, **labeled**/`continue`/`break`
//! interplay, and the **nested-loop `errorRecord` reset**; async-generator
//! **methods** / **function expressions**, `super`/lexical-`this` threading,
//! non-simple parameter lists, and the variable-environment merge. blocked-by:
//! the printer's resolving name generation, enclosing-function-flags threading,
//! the destructuring flattener, and the `EmitContext` super-capture + parameter
//! machinery.

use crate::{new_transformer, TransformOptions, Transformer};
use rustc_hash::FxHashMap;
use tsgo_ast::{
    Kind, ModifierFlags, ModifierList, NodeArena, NodeData, NodeFlags, NodeId, NodeList,
    TokenFlags, VisitOptions,
};
use tsgo_printer::emithelpers::EmitHelper;
use tsgo_printer::{EmitContext, EmitFlags};

/// ES2018 `__await` — wraps an awaited value so `__asyncGenerator` can suspend
/// on it. Defined here (not in `tsgo_printer`) because the printer crate is out
/// of this round's edit scope.
// Go: internal/printer/helpers.go:awaitHelper
pub static AWAIT_HELPER: EmitHelper = EmitHelper {
    name: "typescript:await",
    import_name: "__await",
    scoped: false,
    priority: None,
    dependencies: &[],
    text: r#"var __await = (this && this.__await) || function (v) { return this instanceof __await ? (this.v = v, this) : new __await(v); }"#,
};

/// ES2018 `__asyncGenerator` — drives an async generator's inner generator
/// function, distinguishing `__await`-wrapped suspensions from yielded values.
// Go: internal/printer/helpers.go:asyncGeneratorHelper
pub static ASYNC_GENERATOR_HELPER: EmitHelper = EmitHelper {
    name: "typescript:asyncGenerator",
    import_name: "__asyncGenerator",
    scoped: false,
    priority: None,
    dependencies: &[&AWAIT_HELPER],
    text: r#"var __asyncGenerator = (this && this.__asyncGenerator) || function (thisArg, _arguments, generator) {
    if (!Symbol.asyncIterator) throw new TypeError("Symbol.asyncIterator is not defined.");
    var g = generator.apply(thisArg, _arguments || []), i, q = [];
    return i = Object.create((typeof AsyncIterator === "function" ? AsyncIterator : Object).prototype), verb("next"), verb("throw"), verb("return", awaitReturn), i[Symbol.asyncIterator] = function () { return this; }, i;
    function awaitReturn(f) { return function (v) { return Promise.resolve(v).then(f, reject); }; }
    function verb(n, f) { if (g[n]) { i[n] = function (v) { return new Promise(function (a, b) { q.push([n, v, a, b]) > 1 || resume(n, v); }); }; if (f) i[n] = f(i[n]); } }
    function resume(n, v) { try { step(g[n](v)); } catch (e) { settle(q[0][3], e); } }
    function step(r) { r.value instanceof __await ? Promise.resolve(r.value.v).then(fulfill, reject) : settle(q[0][2], r); }
    function fulfill(value) { resume("next", value); }
    function reject(value) { resume("throw", value); }
    function settle(f, v) { if (f(v), q.shift(), q.length) resume(q[0][0], q[0][1]); }
};"#,
};

/// ES2018 `__asyncDelegator` — adapts a `yield*` delegate so an async generator
/// can delegate to another (sync or async) iterator.
// Go: internal/printer/helpers.go:asyncDelegatorHelper
pub static ASYNC_DELEGATOR_HELPER: EmitHelper = EmitHelper {
    name: "typescript:asyncDelegator",
    import_name: "__asyncDelegator",
    scoped: false,
    priority: None,
    dependencies: &[&AWAIT_HELPER],
    text: r#"var __asyncDelegator = (this && this.__asyncDelegator) || function (o) {
    var i, p;
    return i = {}, verb("next"), verb("throw", function (e) { throw e; }), verb("return"), i[Symbol.iterator] = function () { return this; }, i;
    function verb(n, f) { i[n] = o[n] ? function (v) { return (p = !p) ? { value: __await(o[n](v)), done: false } : f ? f(v) : v; } : f; }
};"#,
};

/// ES2018 `__asyncValues` — obtains an async iterator for a `for await` /
/// `yield*` source, falling back to a sync iterator wrapped per-step.
// Go: internal/printer/helpers.go:asyncValuesHelper
pub static ASYNC_VALUES_HELPER: EmitHelper = EmitHelper {
    name: "typescript:asyncValues",
    import_name: "__asyncValues",
    scoped: false,
    priority: None,
    dependencies: &[],
    text: r#"var __asyncValues = (this && this.__asyncValues) || function (o) {
    if (!Symbol.asyncIterator) throw new TypeError("Symbol.asyncIterator is not defined.");
    var m = o[Symbol.asyncIterator], i;
    return m ? m.call(o) : (o = typeof __values === "function" ? __values(o) : o[Symbol.iterator](), i = {}, verb("next"), verb("throw"), verb("return"), i[Symbol.asyncIterator] = function () { return this; }, i);
    function verb(n) { i[n] = o[n] && function (v) { return new Promise(function (resolve, reject) { v = o[n](v), settle(resolve, reject, v.done, v.value); }); }; }
    function settle(resolve, reject, d, v) { Promise.resolve(v).then(function(v) { resolve({ value: v, done: d }); }, reject); }
};"#,
};

/// Builds a [`Transformer`] that lowers ES2018 async generators, sharing the
/// pipeline's emit context.
///
/// # Examples
/// ```
/// use tsgo_transformers::{estransforms::forawait::new_for_await_transformer, TransformOptions};
/// let _tx = new_for_await_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/estransforms/forawait.go:newforawaitTransformer
pub fn new_for_await_transformer(opt: &TransformOptions) -> Transformer {
    new_transformer(
        Box::new(|ec: &mut EmitContext, node: NodeId| for_await_visit(ec, node)),
        opt.context.clone(),
    )
}

/// Emit-context-threaded visit (skeleton): recurses through container nodes via
/// [`visit_each_child_ec`]. The async-generator lowering arms are added per
/// TDD slice.
// Go: internal/transformers/estransforms/forawait.go:forawaitTransformer.visit
fn for_await_visit(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::SourceFile => visit_source_file(ec, node),
        Kind::FunctionDeclaration if is_async_generator_function_declaration(ec.arena(), node) => {
            visit_async_generator_function_declaration(ec, node)
        }
        // A plain function declaration: thread the emit context through its body
        // inside a fresh variable environment so a `for await` lowered there
        // hoists its `var` temporaries into *that* function's body (mirrors the
        // optionalchain stage's 6i function-body handling).
        Kind::FunctionDeclaration => visit_function_declaration(ec, node),
        // `for await (x of y) ...`: lower to the async-iteration scaffold.
        Kind::ForOfStatement if for_of_has_await_modifier(ec.arena(), node) => {
            transform_for_await_of_statement(ec, node)
        }
        _ => visit_each_child_ec(ec, node),
    }
}

/// Reports whether a `for-of` statement carries the `await` modifier
/// (`for await (...)`).
///
/// Side effects: none (reads the arena).
fn for_of_has_await_modifier(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.data(node),
        NodeData::ForInOrOfStatement(d) if d.await_modifier.is_some()
    )
}

/// Emit-context-threaded `VisitEachChild`: recursively runs [`for_await_visit`]
/// over every child, then rebuilds the node with the transformed children. The
/// node is returned unchanged when no child changed.
///
/// Side effects: may push rebuilt nodes; may request/attach emit helpers.
fn visit_each_child_ec(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let mut children = Vec::new();
    ec.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    let mut replacements: FxHashMap<NodeId, NodeId> = FxHashMap::default();
    for child in children {
        let transformed = for_await_visit(ec, child);
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
        .map(|s| for_await_visit(ec, s))
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

/// Rebuilds a plain function declaration, visiting its body inside its own
/// variable environment so a `for await` lowered in the body hoists its `var`
/// temporaries into that function's body (round 6z), not at module top.
///
/// Side effects: pushes/pops a variable environment; rebuilds the function.
// Go: internal/transformers/estransforms/forawait.go:visitFunctionDeclaration
//     (non-async-generator path) + EmitContext.VisitFunctionBody
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
    let body = visit_for_await_function_body(ec, body);
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

/// Visits a function-like block body within a fresh variable environment, then
/// prepends the hoisted `var ...;` declarations (the `for await` scaffold's
/// `done`/`errorRecord`/`returnMethod`/`value` temporaries) collected during
/// the visit. A non-block (overload signature) body is returned unchanged.
///
/// Side effects: pushes/pops a variable environment; may rebuild the body.
// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody
fn visit_for_await_function_body(ec: &mut EmitContext, body: Option<NodeId>) -> Option<NodeId> {
    let body = body?;
    if let NodeData::Block(d) = ec.arena().data(body) {
        let statements = d.list.nodes.clone();
        ec.start_variable_environment();
        let mut changed = false;
        let mut visited = Vec::with_capacity(statements.len());
        for statement in statements {
            let v = for_await_visit(ec, statement);
            changed |= v != statement;
            visited.push(v);
        }
        let hoisted = ec.end_variable_environment();
        // Nothing was lowered in this body: keep the original block so its
        // source multi-line layout (and identity) is preserved.
        if hoisted.is_empty() && !changed {
            return Some(body);
        }
        // A `for await` lowered here hoists its `var` temporaries; Go preserves
        // the original block's `MultiLine` flag via `UpdateBlock`. The Rust AST
        // carries that as the `MULTI_LINE` emit flag, so force it on the rebuilt
        // body to match the multi-line scaffold tsc emits.
        let force_multi_line = !hoisted.is_empty();
        let mut all = hoisted;
        all.extend(visited);
        let block = ec.arena_mut().new_block(NodeList::new(all));
        if force_multi_line {
            ec.set_emit_flags(block, EmitFlags::MULTI_LINE);
        }
        return Some(block);
    }
    Some(body)
}

/// Lowers `for await (<initializer> of <expression>) <statement>` (inside an
/// async, non-generator context) to the ES2018 down-level async-iteration
/// scaffold:
///
/// ```text
/// try {
///     for (var _nuc = true, _it = __asyncValues(<expression>), _res;
///          _res = await _it.next(), _done = _res.done, !_done;
///          _nuc = true) {
///         _val = _res.value;
///         _nuc = false;
///         <bound loop variable>;
///         <body statements>
///     }
/// }
/// catch (_e) { _err = { error: _e }; }
/// finally {
///     try { if (!_nuc && !_done && (_ret = _it.return)) await _ret.call(_it); }
///     finally { if (_err) throw _err.error; }
/// }
/// ```
///
/// The `_err`/`_done`/`_ret`/`_val` temporaries are hoisted into the enclosing
/// function body via the active variable environment; `_nuc`/`_it`/`_res` are
/// declared by the `for` loop itself. Requests the `__asyncValues` helper.
///
/// Side effects: pushes rebuilt nodes; hoists `var` declarations; requests the
/// `__asyncValues` helper; sets single-line emit flags on the cleanup blocks.
// Go: internal/transformers/estransforms/forawait.go:transformForAwaitOfStatement
fn transform_for_await_of_statement(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (initializer, expression, statement) = match ec.arena().data(node) {
        NodeData::ForInOrOfStatement(d) => (d.initializer, d.expression, d.statement),
        _ => unreachable!("kind/data mismatch"),
    };
    let expression = for_await_visit(ec, expression);

    // For an identifier source, Go derives the iterator/result names from the
    // source identifier; that needs the printer's resolving `getTextOfNode` for
    // the nested generated name (DEFER, see worklog), so this round only handles
    // a non-identifier source where both are clean `NewTempVariable` temps.
    let is_identifier = ec.arena().kind(expression) == Kind::Identifier;
    let iterator = if is_identifier {
        ec.factory().new_generated_name_for_node(expression)
    } else {
        ec.factory().new_temp_variable()
    };
    let result = if is_identifier {
        ec.factory().new_generated_name_for_node(iterator)
    } else {
        ec.factory().new_temp_variable()
    };
    let non_user_code = ec.factory().new_temp_variable();
    let done = ec.factory().new_temp_variable();
    ec.add_variable_declaration(done);
    let error_record = ec.factory().new_unique_name("e");
    let catch_variable = ec.factory().new_generated_name_for_node(error_record);
    let return_method = ec.factory().new_temp_variable();

    let call_values = build_async_values_call(ec, expression);

    // `iterator.next()`
    let next_name = ec.arena_mut().new_identifier("next");
    let next_access = ec
        .arena_mut()
        .new_property_access_expression(iterator, None, next_name);
    let call_next = ec.arena_mut().new_call_expression(
        next_access,
        None,
        None,
        NodeList::new(vec![]),
        NodeFlags::NONE,
    );
    // `result.done` / `result.value`
    let done_name = ec.arena_mut().new_identifier("done");
    let get_done = ec
        .arena_mut()
        .new_property_access_expression(result, None, done_name);
    let value_name = ec.arena_mut().new_identifier("value");
    let get_value = ec
        .arena_mut()
        .new_property_access_expression(result, None, value_name);
    // `returnMethod.call(iterator)`
    let call_return = new_function_call_call(ec, return_method, iterator);

    ec.add_variable_declaration(error_record);
    ec.add_variable_declaration(return_method);

    // `var _nuc = true, _it = __asyncValues(<expr>), _res`
    // (DEFER the iteration-container `errorRecord` reset for nested loops.)
    let true_kw = ec.arena_mut().new_keyword_expression(Kind::TrueKeyword);
    let decl_non_user =
        ec.arena_mut()
            .new_variable_declaration(non_user_code, None, None, Some(true_kw));
    let decl_iterator =
        ec.arena_mut()
            .new_variable_declaration(iterator, None, None, Some(call_values));
    let decl_result = ec
        .arena_mut()
        .new_variable_declaration(result, None, None, None);
    let var_decl_list = ec
        .arena_mut()
        .new_variable_declaration_list(NodeList::new(vec![
            decl_non_user,
            decl_iterator,
            decl_result,
        ]));

    // `_res = await _it.next(), _done = _res.done, !_done`
    let downlevel_next = create_downlevel_await_value(ec, call_next);
    let assign_result = new_assignment(ec, result, downlevel_next);
    let assign_done = new_assignment(ec, done, get_done);
    let not_done = ec
        .arena_mut()
        .new_prefix_unary_expression(Kind::ExclamationToken, done);
    let condition = inline_expressions_ec(ec, vec![assign_result, assign_done, not_done]);

    // `_nuc = true`
    let true_kw2 = ec.arena_mut().new_keyword_expression(Kind::TrueKeyword);
    let incrementor = new_assignment(ec, non_user_code, true_kw2);

    let for_body =
        convert_for_of_statement_head(ec, initializer, statement, get_value, non_user_code);
    let for_statement = ec.arena_mut().new_for_statement(
        Some(var_decl_list),
        Some(condition),
        Some(incrementor),
        for_body,
    );

    let try_block = ec.arena_mut().new_block(NodeList::new(vec![for_statement]));

    // catch (_e) { _err = { error: _e }; }   (single line)
    let error_key = ec.arena_mut().new_identifier("error");
    let error_prop =
        ec.arena_mut()
            .new_property_assignment(None, error_key, None, None, Some(catch_variable));
    let error_object = ec
        .arena_mut()
        .new_object_literal_expression(NodeList::new(vec![error_prop]));
    let assign_error = new_assignment(ec, error_record, error_object);
    let assign_error_stmt = ec.arena_mut().new_expression_statement(assign_error);
    let catch_body = ec
        .arena_mut()
        .new_block(NodeList::new(vec![assign_error_stmt]));
    ec.set_emit_flags(catch_body, EmitFlags::SINGLE_LINE);
    let catch_var_decl = ec
        .arena_mut()
        .new_variable_declaration(catch_variable, None, None, None);
    let catch_clause = ec
        .arena_mut()
        .new_catch_clause(Some(catch_var_decl), catch_body);

    // finally { try { if (!_nuc && !_done && (_ret = _it.return)) await _ret.call(_it); }
    //           finally { if (_err) throw _err.error; } }
    let not_non_user = ec
        .arena_mut()
        .new_prefix_unary_expression(Kind::ExclamationToken, non_user_code);
    let not_done2 = ec
        .arena_mut()
        .new_prefix_unary_expression(Kind::ExclamationToken, done);
    let and1 = ec.arena_mut().new_token(Kind::AmpersandAmpersandToken);
    let left_and = ec
        .arena_mut()
        .new_binary_expression(not_non_user, and1, not_done2);
    let return_name = ec.arena_mut().new_identifier("return");
    let iter_return = ec
        .arena_mut()
        .new_property_access_expression(iterator, None, return_name);
    let assign_return = new_assignment(ec, return_method, iter_return);
    let and2 = ec.arena_mut().new_token(Kind::AmpersandAmpersandToken);
    let inner_if_condition = ec
        .arena_mut()
        .new_binary_expression(left_and, and2, assign_return);
    let downlevel_return = create_downlevel_await_value(ec, call_return);
    let return_stmt = ec.arena_mut().new_expression_statement(downlevel_return);
    let inner_if = ec
        .arena_mut()
        .new_if_statement(inner_if_condition, return_stmt, None);
    ec.set_emit_flags(inner_if, EmitFlags::SINGLE_LINE);
    let inner_try_block = ec.arena_mut().new_block(NodeList::new(vec![inner_if]));

    let error_key2 = ec.arena_mut().new_identifier("error");
    let throw_access =
        ec.arena_mut()
            .new_property_access_expression(error_record, None, error_key2);
    let throw_stmt = ec.arena_mut().new_throw_statement(throw_access);
    let inner_finally_if = ec
        .arena_mut()
        .new_if_statement(error_record, throw_stmt, None);
    ec.set_emit_flags(inner_finally_if, EmitFlags::SINGLE_LINE);
    let inner_finally_block = ec
        .arena_mut()
        .new_block(NodeList::new(vec![inner_finally_if]));
    ec.set_emit_flags(inner_finally_block, EmitFlags::SINGLE_LINE);
    let inner_try =
        ec.arena_mut()
            .new_try_statement(inner_try_block, None, Some(inner_finally_block));
    let finally_block = ec.arena_mut().new_block(NodeList::new(vec![inner_try]));

    ec.arena_mut()
        .new_try_statement(try_block, Some(catch_clause), Some(finally_block))
}

/// Builds the `for await` loop body: assigns the iteration value into a hoisted
/// temp (`_val = result.value`), clears the non-user-code flag (`_nuc =
/// false`), binds the loop variable from the temp (`const x = _val`, or a plain
/// assignment for an existing-variable target), then appends the (visited) body
/// statements.
///
/// Side effects: pushes rebuilt nodes; hoists the value `var` declaration.
// Go: internal/transformers/estransforms/forawait.go:convertForOfStatementHead
fn convert_for_of_statement_head(
    ec: &mut EmitContext,
    initializer: NodeId,
    statement: NodeId,
    bound_value: NodeId,
    non_user_code: NodeId,
) -> NodeId {
    let value = ec.factory().new_temp_variable();
    ec.add_variable_declaration(value);
    let assign_value = new_assignment(ec, value, bound_value);
    let value_stmt = ec.arena_mut().new_expression_statement(assign_value);
    let false_kw = ec.arena_mut().new_keyword_expression(Kind::FalseKeyword);
    let exit_assign = new_assignment(ec, non_user_code, false_kw);
    let exit_stmt = ec.arena_mut().new_expression_statement(exit_assign);
    let mut statements = vec![value_stmt, exit_stmt];

    let binding = create_for_of_binding_statement(ec, initializer, value);
    let binding = for_await_visit(ec, binding);
    statements.push(binding);

    let visited_statement = for_await_visit(ec, statement);
    if let NodeData::Block(d) = ec.arena().data(visited_statement) {
        statements.extend(d.list.nodes.iter().copied());
    } else {
        statements.push(visited_statement);
    }
    ec.arena_mut().new_block(NodeList::new(statements))
}

/// Builds the statement that binds the iteration value: for a declaration list
/// (`const x` / `let x` / `var x`) → `const x = <value>;` (preserving the
/// declaration kind); for an existing-variable target → `<target> = <value>;`.
///
/// Side effects: pushes rebuilt nodes.
// Go: internal/printer/factory.go:NodeFactory.CreateForOfBindingStatement
fn create_for_of_binding_statement(
    ec: &mut EmitContext,
    initializer: NodeId,
    bound_value: NodeId,
) -> NodeId {
    if ec.arena().kind(initializer) == Kind::VariableDeclarationList {
        let (first_declaration, flags) = match ec.arena().data(initializer) {
            NodeData::VariableDeclarationList(d) => {
                (d.declarations.nodes[0], ec.arena().flags(initializer))
            }
            _ => unreachable!("kind checked above"),
        };
        let name = match ec.arena().data(first_declaration) {
            NodeData::VariableDeclaration(d) => d.name,
            _ => unreachable!("declaration-list element is a variable declaration"),
        };
        let updated_declaration =
            ec.arena_mut()
                .new_variable_declaration(name, None, None, Some(bound_value));
        let declaration_list = ec
            .arena_mut()
            .new_variable_declaration_list(NodeList::new(vec![updated_declaration]));
        // Preserve `let`/`const`/`using` (block-scoped) flags.
        ec.arena_mut()
            .add_flags(declaration_list, flags & NodeFlags::BLOCK_SCOPED);
        return ec
            .arena_mut()
            .new_variable_statement(None, declaration_list);
    }
    let assignment = new_assignment(ec, initializer, bound_value);
    ec.arena_mut().new_expression_statement(assignment)
}

/// Within an async, non-generator context a down-level `await` is a plain
/// `await <expression>` (the ES2017 `async` stage lowers it later). The
/// async-generator (`yield __await(...)`) form is handled by
/// [`create_downlevel_await`] and is DEFER'd for `for await` until enclosing
/// function flags are threaded.
///
/// Side effects: pushes the await node onto the arena.
// Go: internal/transformers/estransforms/forawait.go:createDownlevelAwait
//     (non-Generator branch)
fn create_downlevel_await_value(ec: &mut EmitContext, expression: NodeId) -> NodeId {
    ec.arena_mut().new_await_expression(expression)
}

/// Builds an assignment expression `left = right`.
///
/// Side effects: pushes the `=` token and binary-expression nodes.
// Go: internal/printer/factory.go:NodeFactory.NewAssignmentExpression
fn new_assignment(ec: &mut EmitContext, left: NodeId, right: NodeId) -> NodeId {
    let equals = ec.arena_mut().new_token(Kind::EqualsToken);
    ec.arena_mut().new_binary_expression(left, equals, right)
}

/// Reduces a non-empty list of expressions left-to-right with the comma
/// operator (`a, b, c`). Panics on an empty list (callers never pass one).
///
/// Side effects: pushes comma-token / binary-expression nodes.
// Go: internal/printer/factory.go:NodeFactory.InlineExpressions
fn inline_expressions_ec(ec: &mut EmitContext, exprs: Vec<NodeId>) -> NodeId {
    let mut iter = exprs.into_iter();
    let mut acc = iter
        .next()
        .expect("inline_expressions_ec requires a non-empty list");
    for next in iter {
        let comma = ec.arena_mut().new_token(Kind::CommaToken);
        acc = ec.arena_mut().new_binary_expression(acc, comma, next);
    }
    acc
}

/// Builds `target.call(thisArg)` — the `iterator.return`-cleanup call shape.
///
/// Side effects: pushes the property-access/call nodes onto the arena.
// Go: internal/printer/factory.go:NodeFactory.NewFunctionCallCall
fn new_function_call_call(ec: &mut EmitContext, target: NodeId, this_arg: NodeId) -> NodeId {
    let call_name = ec.arena_mut().new_identifier("call");
    let callee = ec
        .arena_mut()
        .new_property_access_expression(target, None, call_name);
    ec.arena_mut().new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![this_arg]),
        NodeFlags::NONE,
    )
}

/// Lowers `async function* g(...) { ... }` to a plain function whose body is
/// `{ return __asyncGenerator(this, arguments, function* g_1() { ... }); }`.
/// The `async` modifier and the outer `*` are dropped; the inner generator
/// function gets a generated name derived from the declaration name (`g` →
/// `g_1`). Inside the inner generator, `await`/`yield`/`return` are rewritten by
/// [`convert_async_generator_body_node`].
///
/// Side effects: pushes rebuilt nodes; requests the async-generator helpers.
// Go: internal/transformers/estransforms/forawait.go:visitFunctionDeclaration
//     (+ transformAsyncGeneratorFunctionBody)
fn visit_async_generator_function_declaration(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let (modifiers, name, parameters, body) = match ec.arena().data(node) {
        NodeData::FunctionDeclaration(d) => {
            (d.modifiers.clone(), d.name, d.parameters.clone(), d.body)
        }
        _ => unreachable!("kind/data mismatch"),
    };
    let modifiers = strip_async_modifier(ec.arena(), modifiers);
    let outer_body = build_async_generator_wrapper_body(ec, name, body);
    ec.arena_mut().new_function_declaration(
        modifiers,
        None, // asterisk_token (the outer function is no longer a generator)
        name,
        None, // type_parameters
        parameters,
        None, // type_node
        None, // full_signature
        Some(outer_body),
    )
}

/// Builds the `{ return __asyncGenerator(this, arguments, function* <name_1>() {
/// <converted body> }); }` block for a lowered async generator. The original
/// `name` seeds the inner generator's generated name; a top-level function
/// declaration always has its own `this`, so `this` is threaded as the first
/// argument.
///
/// Side effects: pushes rebuilt nodes; requests the async-generator helpers.
// Go: internal/transformers/estransforms/forawait.go:transformAsyncGeneratorFunctionBody
fn build_async_generator_wrapper_body(
    ec: &mut EmitContext,
    name: Option<NodeId>,
    body: Option<NodeId>,
) -> NodeId {
    let body_statements = match body.map(|b| ec.arena().data(b)) {
        Some(NodeData::Block(d)) => d.list.nodes.clone(),
        _ => Vec::new(),
    };
    let generator_statements: Vec<NodeId> = body_statements
        .iter()
        .copied()
        .map(|s| convert_async_generator_body_node(ec, s))
        .collect();
    let generator_body = ec
        .arena_mut()
        .new_block(NodeList::new(generator_statements));
    let asterisk = ec.arena_mut().new_token(Kind::AsteriskToken);
    let inner_name = name.map(|n| ec.factory().new_generated_name_for_node(n));
    let generator = ec.arena_mut().new_function_expression(
        None,
        Some(asterisk),
        inner_name,
        None,
        NodeList::new(vec![]),
        None,
        None,
        Some(generator_body),
    );
    // A top-level function declaration has its own lexical `this`.
    let call = build_async_generator_call(ec, generator, true);
    let return_statement = ec.arena_mut().new_return_statement(Some(call));
    ec.arena_mut()
        .new_block(NodeList::new(vec![return_statement]))
}

/// Builds the `__asyncGenerator(<thisArg>, arguments, <generator>)` call,
/// requesting the `__await` and `__asyncGenerator` helpers.
///
/// Side effects: pushes rebuilt nodes; requests emit helpers.
// Go: internal/printer/factory.go:NodeFactory.NewAsyncGeneratorHelper
fn build_async_generator_call(
    ec: &mut EmitContext,
    generator: NodeId,
    has_lexical_this: bool,
) -> NodeId {
    ec.request_emit_helper(&AWAIT_HELPER);
    ec.request_emit_helper(&ASYNC_GENERATOR_HELPER);
    let helper_name = ec.factory().new_unscoped_helper_name("__asyncGenerator");
    let this_arg = if has_lexical_this {
        ec.arena_mut().new_keyword_expression(Kind::ThisKeyword)
    } else {
        make_void_zero(ec)
    };
    let arguments_ident = ec.arena_mut().new_identifier("arguments");
    ec.arena_mut().new_call_expression(
        helper_name,
        None,
        None,
        NodeList::new(vec![this_arg, arguments_ident, generator]),
        NodeFlags::NONE,
    )
}

/// Recursively rewrites an async generator body: `await x` → `yield __await(x)`,
/// `yield e` → `yield yield __await(e)`. (Other arms — `yield*`, `return`, bare
/// `yield` — are added in later TDD slices.) Stops at nested function-like
/// scopes, which carry their own async context.
///
/// Side effects: may push rebuilt nodes; requests emit helpers.
// Go: internal/transformers/estransforms/forawait.go:visitAwaitExpression / visitYieldExpression
fn convert_async_generator_body_node(ec: &mut EmitContext, node: NodeId) -> NodeId {
    match ec.arena().kind(node) {
        Kind::AwaitExpression => {
            let expression = match ec.arena().data(node) {
                NodeData::AwaitExpression(d) => d.expression,
                _ => unreachable!("kind checked above"),
            };
            let expression = convert_async_generator_body_node(ec, expression);
            let awaited = build_await_call(ec, expression);
            ec.arena_mut().new_yield_expression(None, Some(awaited))
        }
        Kind::YieldExpression => {
            let (asterisk_token, expression) = match ec.arena().data(node) {
                NodeData::YieldExpression(d) => (d.asterisk_token, d.expression),
                _ => unreachable!("kind checked above"),
            };
            if let Some(asterisk_token) = asterisk_token {
                // `yield* e`: `yield __await(yield* __asyncDelegator(__asyncValues(e)))`.
                let expression = expression.expect("yield* requires an expression");
                let expression = convert_async_generator_body_node(ec, expression);
                let async_values = build_async_values_call(ec, expression);
                let async_delegator = build_async_delegator_call(ec, async_values);
                let inner_yield = ec
                    .arena_mut()
                    .new_yield_expression(Some(asterisk_token), Some(async_delegator));
                let awaited = build_await_call(ec, inner_yield);
                return ec.arena_mut().new_yield_expression(None, Some(awaited));
            }
            // `yield e` / bare `yield` (no `*`): `yield (yield __await(e | void 0))`.
            let inner = match expression {
                Some(e) => convert_async_generator_body_node(ec, e),
                None => make_void_zero(ec),
            };
            let downlevel = create_downlevel_await(ec, inner);
            ec.arena_mut().new_yield_expression(None, Some(downlevel))
        }
        Kind::ReturnStatement => {
            let expression = match ec.arena().data(node) {
                NodeData::ReturnStatement(d) => d.expression,
                _ => unreachable!("kind checked above"),
            };
            // `return e` -> `return yield __await(e)`.
            let inner = match expression {
                Some(e) => convert_async_generator_body_node(ec, e),
                None => make_void_zero(ec),
            };
            let downlevel = create_downlevel_await(ec, inner);
            ec.arena_mut().new_return_statement(Some(downlevel))
        }
        // A nested function-like scope is its own async boundary; leave it.
        Kind::FunctionDeclaration
        | Kind::FunctionExpression
        | Kind::ArrowFunction
        | Kind::MethodDeclaration
        | Kind::GetAccessor
        | Kind::SetAccessor
        | Kind::Constructor
        | Kind::ClassDeclaration
        | Kind::ClassExpression => node,
        _ => visit_each_child_converting(ec, node),
    }
}

/// `VisitEachChild` that recurses with [`convert_async_generator_body_node`],
/// rebuilding the node only when a child changed.
///
/// Side effects: may push rebuilt nodes; requests emit helpers.
fn visit_each_child_converting(ec: &mut EmitContext, node: NodeId) -> NodeId {
    let mut children = Vec::new();
    ec.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    let mut replacements: FxHashMap<NodeId, NodeId> = FxHashMap::default();
    for child in children {
        let transformed = convert_async_generator_body_node(ec, child);
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

/// Builds `__await(<expression>)`, requesting the `__await` helper.
///
/// Side effects: pushes rebuilt nodes; requests the `__await` helper.
// Go: internal/printer/factory.go:NodeFactory.NewAwaitHelper
fn build_await_call(ec: &mut EmitContext, expression: NodeId) -> NodeId {
    ec.request_emit_helper(&AWAIT_HELPER);
    let helper_name = ec.factory().new_unscoped_helper_name("__await");
    ec.arena_mut().new_call_expression(
        helper_name,
        None,
        None,
        NodeList::new(vec![expression]),
        NodeFlags::NONE,
    )
}

/// Builds `__asyncValues(<expression>)`, requesting the `__asyncValues` helper.
///
/// Side effects: pushes rebuilt nodes; requests the `__asyncValues` helper.
// Go: internal/printer/factory.go:NodeFactory.NewAsyncValuesHelper
fn build_async_values_call(ec: &mut EmitContext, expression: NodeId) -> NodeId {
    ec.request_emit_helper(&ASYNC_VALUES_HELPER);
    let helper_name = ec.factory().new_unscoped_helper_name("__asyncValues");
    ec.arena_mut().new_call_expression(
        helper_name,
        None,
        None,
        NodeList::new(vec![expression]),
        NodeFlags::NONE,
    )
}

/// Builds `__asyncDelegator(<expression>)`, requesting the `__await` and
/// `__asyncDelegator` helpers (matching Go's request order).
///
/// Side effects: pushes rebuilt nodes; requests emit helpers.
// Go: internal/printer/factory.go:NodeFactory.NewAsyncDelegatorHelper
fn build_async_delegator_call(ec: &mut EmitContext, expression: NodeId) -> NodeId {
    ec.request_emit_helper(&AWAIT_HELPER);
    ec.request_emit_helper(&ASYNC_DELEGATOR_HELPER);
    let helper_name = ec.factory().new_unscoped_helper_name("__asyncDelegator");
    ec.arena_mut().new_call_expression(
        helper_name,
        None,
        None,
        NodeList::new(vec![expression]),
        NodeFlags::NONE,
    )
}

/// Within an async generator (`enclosingFunctionFlags & Generator`), a
/// down-level `await` is `yield __await(<expression>)`.
///
/// Side effects: pushes rebuilt nodes; requests the `__await` helper.
// Go: internal/transformers/estransforms/forawait.go:createDownlevelAwait
fn create_downlevel_await(ec: &mut EmitContext, expression: NodeId) -> NodeId {
    let awaited = build_await_call(ec, expression);
    ec.arena_mut().new_yield_expression(None, Some(awaited))
}

/// Reports whether a function declaration is an `async function*`
/// (async generator).
///
/// Side effects: none (reads the arena).
fn is_async_generator_function_declaration(arena: &NodeArena, node: NodeId) -> bool {
    matches!(
        arena.data(node),
        NodeData::FunctionDeclaration(d)
            if d.asterisk_token.is_some() && has_async_modifier(&d.modifiers)
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
fn make_void_zero(ec: &mut EmitContext) -> NodeId {
    let zero = ec.arena_mut().new_numeric_literal("0", TokenFlags::NONE);
    ec.arena_mut().new_void_expression(zero)
}

#[cfg(test)]
#[path = "forawait_test.rs"]
mod tests;
