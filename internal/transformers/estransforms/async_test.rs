use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// The `__awaiter` helper definition emitted in the module prologue whenever the
// stage requests it. Shared by the cases below to keep expectations readable.
const AWAITER_PROLOGUE: &str = "var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {\n    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }\n    return new (P || (P = Promise))(function (resolve, reject) {\n        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }\n        function rejected(value) { try { step(generator[\"throw\"](value)); } catch (e) { reject(e); } }\n        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }\n        step((generator = generator.apply(thisArg, _arguments || [])).next());\n    });\n};\n";

// Runs the async transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_async_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// Go: internal/transformers/estransforms/async.go:visitFunctionDeclaration
// Tracer bullet: an async function lowers to the `__awaiter` wrapper, `await`
// becomes `yield`, and the `__awaiter` helper definition is emitted in the
// module prologue.
#[test]
fn async_function_lowers_to_awaiter_wrapper() {
    check_downlevel(
        "async function f() { await g(); }",
        "var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {\n    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }\n    return new (P || (P = Promise))(function (resolve, reject) {\n        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }\n        function rejected(value) { try { step(generator[\"throw\"](value)); } catch (e) { reject(e); } }\n        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }\n        step((generator = generator.apply(thisArg, _arguments || [])).next());\n    });\n};\nfunction f() { return __awaiter(this, void 0, void 0, function* () { yield g(); }); }",
    );
}

// Go: internal/transformers/estransforms/async.go (FunctionFlagsGenerator guard)
// An async *generator* needs the `__asyncGenerator` helper (deferred), so it is
// left unchanged by this stage rather than mis-lowered as a plain async function.
#[test]
fn async_generator_is_left_unchanged() {
    check_downlevel(
        "async function* g() { yield 1; }",
        "async function* g() { yield 1; }",
    );
}

// Go: internal/transformers/estransforms/async.go:visitFunctionDeclaration
// An async function with no `await` is still wrapped (the transform triggers on
// the `async` modifier); the generator body is empty.
#[test]
fn async_function_without_await_still_wraps() {
    check_downlevel(
        "async function f() { g(); }",
        "var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {\n    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }\n    return new (P || (P = Promise))(function (resolve, reject) {\n        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }\n        function rejected(value) { try { step(generator[\"throw\"](value)); } catch (e) { reject(e); } }\n        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }\n        step((generator = generator.apply(thisArg, _arguments || [])).next());\n    });\n};\nfunction f() { return __awaiter(this, void 0, void 0, function* () { g(); }); }",
    );
}

// Go: internal/transformers/estransforms/async.go:visitFunctionExpression
// Round 6m tracer: an async *function expression* lowers to the same
// `__awaiter` wrapper as a declaration — its own `this` is threaded as the
// first argument (the function-expression scope has lexical `this`).
#[test]
fn async_function_expression_lowers_to_awaiter_wrapper() {
    check_downlevel(
        "const f = async function () { await x; };",
        &format!(
            "{AWAITER_PROLOGUE}const f = function () {{ return __awaiter(this, void 0, void 0, function* () {{ yield x; }}); }};"
        ),
    );
}

// Go: internal/transformers/estransforms/async.go:visitMethodDeclaration
// Round 6m: an async *method* lowers to a method whose body is the `__awaiter`
// wrapper; the method scope has lexical `this`, threaded as the first argument.
#[test]
fn async_method_lowers_to_awaiter_wrapper() {
    check_downlevel(
        "class C { async m() { await x; } }",
        &format!(
            "{AWAITER_PROLOGUE}class C {{\n    m() {{ return __awaiter(this, void 0, void 0, function* () {{ yield x; }}); }}\n}}"
        ),
    );
}

// Go: internal/transformers/estransforms/async.go:visitArrowFunction
// Round 6m: an async *arrow* lowers to a concise-body arrow returning the
// `__awaiter(...)` call directly (no `{ return ...; }` wrapper). An arrow's
// `this` is lexical: at module top there is no lexical `this`, so the Go
// transform threads `void 0` as the first argument (the arrow case does not set
// `asyncContextHasLexicalThis`).
#[test]
fn async_arrow_lowers_to_awaiter_wrapper_with_lexical_this() {
    check_downlevel(
        "const f = async () => { await x; };",
        &format!(
            "{AWAITER_PROLOGUE}const f = () => __awaiter(void 0, void 0, void 0, function* () {{ yield x; }});"
        ),
    );
}
