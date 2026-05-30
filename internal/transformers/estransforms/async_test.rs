use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the async transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_async_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
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
