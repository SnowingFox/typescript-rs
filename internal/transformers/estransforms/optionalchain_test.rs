use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the optional-chain transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_optional_chain_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression
// Tracer bullet: a single optional property access `a?.b` lowers to a
// not-null-guarded conditional `a === null || a === void 0 ? void 0 : a.b`.
#[test]
fn optional_property_access_lowered() {
    check_downlevel("a?.b;", "a === null || a === void 0 ? void 0 : a.b;");
}

// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression (element-access segment)
// `a?.[x]` lowers to the same guard with an element access on the right.
#[test]
fn optional_element_access_lowered() {
    check_downlevel("a?.[x];", "a === null || a === void 0 ? void 0 : a[x];");
}

// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression (call segment)
// `a?.()` (optional call directly on `a`) lowers to a guarded call.
#[test]
fn optional_call_lowered() {
    check_downlevel("a?.();", "a === null || a === void 0 ? void 0 : a();");
}

// Go: internal/transformers/estransforms/optionalchain.go:flattenChain (single `?.` + trailing segments)
// `a?.b()` is one chain: the `?.` is on `.b` and the call is a trailing
// non-optional segment, so the whole thing is guarded once on `a`.
#[test]
fn optional_method_call_lowered() {
    check_downlevel("a?.b();", "a === null || a === void 0 ? void 0 : a.b();");
}

// Go: internal/transformers/estransforms/optionalchain.go:flattenChain (trailing property segment)
// `a?.b.c` guards once on `a`, with `.c` a trailing non-optional segment.
#[test]
fn optional_chain_trailing_property_lowered() {
    check_downlevel("a?.b.c;", "a === null || a === void 0 ? void 0 : a.b.c;");
}

// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression (non-simple receiver)
// A receiver that is not a simple-copiable expression (`f()`) must be evaluated
// once: it is assigned into a hoisted temp `_a`, the guard tests the assignment
// and the access reads the temp.
#[test]
fn non_simple_receiver_hoists_temp() {
    check_downlevel(
        "f()?.b;",
        "var _a;\n(_a = f()) === null || _a === void 0 ? void 0 : _a.b;",
    );
}

// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression (nested chain)
// `a?.b?.c` is two chains: the inner `a?.b` lowers first, and because that
// lowered conditional is not simple-copiable it is hoisted into a temp `_a`
// which the outer guard tests and the `.c` access reads.
#[test]
fn multiple_optional_links_nest_guards() {
    check_downlevel(
        "a?.b?.c;",
        "var _a;\n(_a = a === null || a === void 0 ? void 0 : a.b) === null || _a === void 0 ? void 0 : _a.c;",
    );
}

// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression
// Generalization (directly green): both deepenings compose — a non-simple
// receiver feeding a nested chain hoists one temp per link (`_a` for `f()`,
// `_b` for the inner conditional), allocated inner-first.
#[test]
fn non_simple_receiver_in_nested_chain_hoists_two_temps() {
    check_downlevel(
        "f()?.b?.c;",
        "var _a, _b;\n(_b = (_a = f()) === null || _a === void 0 ? void 0 : _a.b) === null || _b === void 0 ? void 0 : _b.c;",
    );
}

// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody (per-scope variable environment)
// 6i tracer: a temp-hoisting optional chain inside a function body must hoist
// its `var _a;` INTO that function's body (its own variable environment), not
// at module top. Before 6i this was DEFER'd (left verbatim) because no
// variable environment was active inside non-top-level scopes.
//
// The body prints single-line because the synthesized `Block` does not carry
// Go's `Block.MultiLine` flag (a known printer divergence, see 6c-1); the
// behavior under test is that `var _a;` lands inside `f`'s braces, not at top.
#[test]
fn non_simple_receiver_inside_function_body_hoists_into_body() {
    check_downlevel(
        "function f() { return g()?.b; }",
        "function f() { var _a; return (_a = g()) === null || _a === void 0 ? void 0 : _a.b; }",
    );
}

// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody (concise arrow body)
// 6i: an arrow function's concise expression body is its own variable
// environment. When a chain there hoists a temp, the concise body becomes a
// block `{ var _a; return <lowered>; }` (mirrors Go's VisitFunctionBody, which
// wraps a non-block body in `return` once declarations are hoisted). The outer
// `f` keeps an empty environment (no stray `var` leaks outward).
#[test]
fn temp_in_arrow_concise_body_wraps_into_block() {
    check_downlevel(
        "function f() { return () => g()?.b; }",
        "function f() { return () => { var _a; return (_a = g()) === null || _a === void 0 ? void 0 : _a.b; }; }",
    );
}

// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody (nested scopes)
// 6i: a chain hoisting a temp in an outer function body and another in a nested
// inner function body each target the NEAREST enclosing variable environment.
// The two temps are independent: each function scope resets the temp-name
// counter, so both print `_a` but live in their own body's leading `var`.
#[test]
fn nested_function_bodies_hoist_into_their_own_scopes() {
    check_downlevel(
        "function outer() { g()?.b; function inner() { return h()?.c; } }",
        "function outer() { var _a; (_a = g()) === null || _a === void 0 ? void 0 : _a.b; function inner() { var _a; return (_a = h()) === null || _a === void 0 ? void 0 : _a.c; } }",
    );
}

// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody (function expression)
// 6i: a function *expression* body is also its own variable environment. A
// temp-hoisting chain in a nested function expression lands inside that
// expression's body, not in the enclosing function or at module top.
#[test]
fn temp_inside_function_expression_body_hoists_into_body() {
    check_downlevel(
        "function f() { return function () { return g()?.b; }; }",
        "function f() { return function () { var _a; return (_a = g()) === null || _a === void 0 ? void 0 : _a.b; }; }",
    );
}

// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody (method body)
// 6i: a method body is its own variable environment. Reaching it threads the
// emit context through the enclosing class declaration's members; the chain's
// temp hoists into the method body, not at module top.
#[test]
fn temp_inside_method_body_hoists_into_method() {
    check_downlevel(
        "class C { m() { return g()?.b; } }",
        "class C {\n    m() { var _a; return (_a = g()) === null || _a === void 0 ? void 0 : _a.b; }\n}",
    );
}

// Go: internal/transformers/estransforms/optionalchain.go:visitCallExpression (parenthesized
// optional chain in call position) + NewSyntheticReferenceExpression / NewFunctionCallCall.
// 6s: a parenthesized optional chain called as a function `(a?.b)()` must
// preserve `this` — the receiver `a` is captured as the call's `this` argument
// and the call is lowered to `<lowered access>.call(a)` (not a bare call, which
// would lose `this`). `a` is simple-copiable so no temp is hoisted.
// Baseline: conformance/expressions/optionalChaining/callChain/parentheses.ts.
#[test]
fn parenthesized_optional_call_captures_this() {
    check_downlevel(
        "(a?.b)();",
        "(a === null || a === void 0 ? void 0 : a.b).call(a);",
    );
}

// Go: internal/transformers/estransforms/optionalchain.go:visitDeleteExpression
// 6s: `delete a?.b` lowers to a guarded conditional whose "present" branch is a
// `delete` of the (non-optional) access and whose "absent" branch is `true`
// (deleting a nullish base is a no-op that yields `true`). `a` is
// simple-copiable so no temp is hoisted.
// Baseline: conformance/expressions/optionalChaining/delete/deleteChain.ts.
#[test]
fn delete_optional_access_lowered() {
    check_downlevel(
        "delete a?.b;",
        "a === null || a === void 0 ? true : delete a.b;",
    );
}

// Go: internal/transformers/estransforms/optionalchain.go:visitParenthesizedExpression (isDelete)
// Generalization (directly green): a parenthesized deleted optional chain
// `delete (a?.b)` keeps the original parentheses around the lowered guard, with
// the `delete` still pushed into the present-branch.
// Baseline: deleteChain.ts line `delete (o1?.b);`.
#[test]
fn delete_parenthesized_optional_access_keeps_parens() {
    check_downlevel(
        "delete (a?.b);",
        "(a === null || a === void 0 ? true : delete a.b);",
    );
}

// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression (leftThisArg
// threading for the first call segment) + NewFunctionCallCall.
// 6t: `a?.b?.()` is two chains — the inner optional member `a?.b` is lowered
// with `this`-capture (its receiver `a` becomes the call's `this`), the lowered
// conditional is hoisted into a temp `_a`, and the outer optional call segment
// threads the captured `this` via `_a.call(a)` (a bare `_a()` would lose
// `this`). `a` is simple-copiable so only one temp is hoisted.
// Baseline: conformance/.../callChain/callChain.3.ts (`a?.m?.({x:12})`).
#[test]
fn optional_member_then_optional_call_threads_this() {
    check_downlevel(
        "a?.b?.();",
        "var _a;\n(_a = a === null || a === void 0 ? void 0 : a.b) === null || _a === void 0 ? void 0 : _a.call(a);",
    );
}

// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression (leftThisArg
// captured as a hoisted temp when the receiver of the final access is non-simple).
// Generalization (directly green): exercises the auto-generated-temp branch of
// the `leftThisArg` threading. `a?.b.c?.()` — the inner chain `a?.b.c` ends with
// a trailing `.c` whose receiver `a.b` is non-simple, so capturing `this` for
// the call hoists `_a` (`(_a = a.b).c`). The lowered inner conditional is
// hoisted into `_b`, and the outer optional call threads the captured temp
// `this`: `_b.call(_a)`. Because `_a` is an auto-generated temp it is reused
// as-is (not cloned, unlike the identifier `this` of `a?.b?.()`).
#[test]
fn nested_optional_member_then_optional_call_threads_temp_this() {
    check_downlevel(
        "a?.b.c?.();",
        "var _a, _b;\n(_b = a === null || a === void 0 ? void 0 : (_a = a.b).c) === null || _b === void 0 ? void 0 : _b.call(_a);",
    );
}

// Go: internal/transformers/estransforms/optionalchain.go:visitPropertyOrElementAccessExpression
// (non-optional access in `this`-capturing position) + leftThisArg threading.
// 6t: `a.b?.()` — the receiver `a.b` is a *non-optional* member access, but the
// call segment is optional, so it is visited in `this`-capturing position: the
// simple base `a` becomes the call's `this` (`leftThisArg`), the access `a.b` is
// hoisted into `_a`, and the optional call lowers to `_a.call(a)`.
// Baseline: conformance/.../callChain/callChain.js (`o3.b?.().c`).
#[test]
fn non_optional_member_then_optional_call_threads_this() {
    check_downlevel(
        "a.b?.();",
        "var _a;\n(_a = a.b) === null || _a === void 0 ? void 0 : _a.call(a);",
    );
}

// ───────────────────────────────────────────────────────────────────────
// T2-8 integration tests: optional chaining verification
// ───────────────────────────────────────────────────────────────────────

// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression
// A simple identifier with optional property access — the canonical `a?.b`
// lowering from the spec tests (ES2019 target).
#[test]
fn simple_optional_property_canonical() {
    check_downlevel(
        "var r = a?.b;",
        "var r = a === null || a === void 0 ? void 0 : a.b;",
    );
}

// Go: internal/transformers/estransforms/optionalchain.go:visitOptionalExpression
// Optional chaining on a string literal property name with bracket notation
// `a?.["prop"]` lowers to an element access guard.
#[test]
fn optional_string_element_access_lowered() {
    check_downlevel(
        "a?.[\"prop\"];",
        "a === null || a === void 0 ? void 0 : a[\"prop\"];",
    );
}
