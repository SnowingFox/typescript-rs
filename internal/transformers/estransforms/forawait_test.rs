use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the ES2018 async-generator transformer over `input` and asserts the
// emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_for_await_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// Go: internal/transformers/estransforms/forawait.go:visitFunctionDeclaration
// (FunctionFlagsAsync && FunctionFlagsGenerator branch ->
// transformAsyncGeneratorFunctionBody).
//
// Tracer bullet (verified against `tsc --target es2017`): an async generator
// function declaration lowers to a plain function returning
// `__asyncGenerator(this, arguments, function* g_1() { ... })`; inside the inner
// generator `await x` -> `yield __await(x)` and `yield y` -> `yield yield
// __await(y)` (NOTE: the round briefing's "yield x stays yield x" is wrong; Go
// and tsc both await-then-yield the yielded value). The `__await` and
// `__asyncGenerator` helper definitions are emitted in the module prologue.
#[test]
fn async_generator_function_lowers_to_async_generator_wrapper() {
    let expected = format!(
        "{}\n{}\nfunction g() {{ return __asyncGenerator(this, arguments, function* g_1() {{ yield __await(x); yield yield __await(y); }}); }}",
        AWAIT_HELPER.text, ASYNC_GENERATOR_HELPER.text,
    );
    check_downlevel("async function* g() { await x; yield y; }", &expected);
}

// Go: internal/transformers/estransforms/forawait.go:visitYieldExpression
// (asteriskToken != nil branch).
//
// `yield* e` delegates to another iterator: it becomes
// `yield __await(yield* __asyncDelegator(__asyncValues(e)))`. Prologue helper
// order (verified against tsc): __asyncValues, __await, __asyncDelegator,
// __asyncGenerator.
#[test]
fn async_generator_yield_delegate_uses_async_delegator() {
    let expected = format!(
        "{}\n{}\n{}\n{}\nfunction a() {{ return __asyncGenerator(this, arguments, function* a_1() {{ yield __await(yield* __asyncDelegator(__asyncValues(y))); }}); }}",
        ASYNC_VALUES_HELPER.text, AWAIT_HELPER.text, ASYNC_DELEGATOR_HELPER.text, ASYNC_GENERATOR_HELPER.text,
    );
    check_downlevel("async function* a() { yield* y; }", &expected);
}

// Go: internal/transformers/estransforms/forawait.go:visitReturnStatement
// A `return e` inside an async generator awaits the returned value:
// `return yield __await(e)`.
#[test]
fn async_generator_return_awaits_value() {
    let expected = format!(
        "{}\n{}\nfunction b() {{ return __asyncGenerator(this, arguments, function* b_1() {{ return yield __await(y); }}); }}",
        AWAIT_HELPER.text, ASYNC_GENERATOR_HELPER.text,
    );
    check_downlevel("async function* b() { return y; }", &expected);
}

// Go: internal/transformers/estransforms/forawait.go:visitYieldExpression
// (node.Expression == nil -> NewVoidZeroExpression()).
// A bare `yield` yields `void 0` after a down-level await:
// `yield yield __await(void 0)`.
#[test]
fn async_generator_bare_yield_uses_void_zero() {
    let expected = format!(
        "{}\n{}\nfunction c() {{ return __asyncGenerator(this, arguments, function* c_1() {{ yield yield __await(void 0); }}); }}",
        AWAIT_HELPER.text, ASYNC_GENERATOR_HELPER.text,
    );
    check_downlevel("async function* c() { yield; }", &expected);
}

// Go: internal/transformers/estransforms/forawait.go:transformForAwaitOfStatement
// (+ convertForOfStatementHead).
//
// 6z tracer (verified against `tsc --target es2017`, where `async`/`await` stay
// native and only the ES2018 `for await` downlevels): `for await (const x of
// gen()) {}` inside a plain async function lowers to the full async-iteration
// scaffold — an `__asyncValues(gen())` iterator temp (`_e`), a `result` temp
// (`_f`), the C-style `for` whose condition is `_f = await _e.next(), _a =
// _f.done, !_a`, the loop variable bound from `_f.value`, and the
// `try/catch/finally` with the `iterator.return` cleanup (down-level `await`,
// since the enclosing function is async but not a generator). Only the
// `__asyncValues` helper is requested. NON-IDENTIFIER source `gen()` so the
// iterator/result temps are clean `NewTempVariable` names (`_e`/`_f`); an
// identifier source needs the printer's resolving `getTextOfNode` for the
// nested generated-name and is DEFER'd (see worklog).
//
// Go/tsc emit the catch variable as `e_1_1` (`NewGeneratedNameForNode` of the
// `e_1` errorRecord); the Rust printer's `generate_name_for_node` reads the raw
// `arena().text()` ("e") rather than the resolving `getTextOfNode` ("e_1"), so
// it lands on `e_2`. The name is a fresh binding, so this is a cosmetic printer
// divergence only (documented in the 6z worklog).
#[test]
fn for_await_of_lowers_to_async_iteration_scaffold() {
    let expected = format!(
        "{}\nasync function f() {{\n    var _a, e_1, _b, _c;\n    try {{\n        for (var _d = true, _e = __asyncValues(gen()), _f; _f = await _e.next(), _a = _f.done, !_a; _d = true) {{\n            _c = _f.value;\n            _d = false;\n            const x = _c;\n        }}\n    }}\n    catch (e_2) {{ e_1 = {{ error: e_2 }}; }}\n    finally {{\n        try {{\n            if (!_d && !_a && (_b = _e.return)) await _b.call(_e);\n        }}\n        finally {{ if (e_1) throw e_1.error; }}\n    }}\n}}",
        ASYNC_VALUES_HELPER.text,
    );
    check_downlevel(
        "async function f() { for await (const x of gen()) {} }",
        &expected,
    );
}

// Go: internal/transformers/estransforms/forawait.go:convertForOfStatementHead
// (`statements = append(statements, statement.Statements()...)` for a block
// body). The loop body's statements are spliced after the value binding, so
// `use(x)` references the bound `const x` (verified against `tsc --target
// es2017`).
#[test]
fn for_await_of_body_statements_follow_the_binding() {
    let expected = format!(
        "{}\nasync function f() {{\n    var _a, e_1, _b, _c;\n    try {{\n        for (var _d = true, _e = __asyncValues(gen()), _f; _f = await _e.next(), _a = _f.done, !_a; _d = true) {{\n            _c = _f.value;\n            _d = false;\n            const x = _c;\n            use(x);\n        }}\n    }}\n    catch (e_2) {{ e_1 = {{ error: e_2 }}; }}\n    finally {{\n        try {{\n            if (!_d && !_a && (_b = _e.return)) await _b.call(_e);\n        }}\n        finally {{ if (e_1) throw e_1.error; }}\n    }}\n}}",
        ASYNC_VALUES_HELPER.text,
    );
    check_downlevel(
        "async function f() { for await (const x of gen()) { use(x); } }",
        &expected,
    );
}

// Go: internal/transformers/estransforms/forawait.go:convertForOfStatementHead
// via NodeFactory.CreateForOfBindingStatement (non-VariableDeclarationList
// branch). An existing-variable target (`for await (x of ...)`, no `const`/
// `let`) binds with a plain assignment `x = _c;` rather than a declaration
// (verified against `tsc --target es2017`).
#[test]
fn for_await_of_existing_variable_target_binds_with_assignment() {
    let expected = format!(
        "{}\nasync function f() {{\n    var _a, e_1, _b, _c;\n    try {{\n        for (var _d = true, _e = __asyncValues(gen()), _f; _f = await _e.next(), _a = _f.done, !_a; _d = true) {{\n            _c = _f.value;\n            _d = false;\n            x = _c;\n        }}\n    }}\n    catch (e_2) {{ e_1 = {{ error: e_2 }}; }}\n    finally {{\n        try {{\n            if (!_d && !_a && (_b = _e.return)) await _b.call(_e);\n        }}\n        finally {{ if (e_1) throw e_1.error; }}\n    }}\n}}",
        ASYNC_VALUES_HELPER.text,
    );
    check_downlevel(
        "async function f() { for await (x of gen()) {} }",
        &expected,
    );
}

// Go: internal/transformers/estransforms/forawait.go:visitMethodDeclaration
// (async-generator branch) — DEFER(6y): async-generator *methods* need the
// `_super` binding + hierarchy-facts threading, so this round handles only
// function *declarations*. Guard: an async-generator method is left unchanged
// (not mis-lowered) by this stage.
#[test]
fn async_generator_method_is_left_unchanged() {
    // The class is re-emitted in the printer's canonical multiline form, but the
    // method (and its `await`/`yield` body) is left untouched by this stage.
    check_downlevel(
        "class C { async *m() { await x; } }",
        "class C {\n    async *m() { await x; }\n}",
    );
}
