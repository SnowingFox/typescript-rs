use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the named-evaluation transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_named_evaluation_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// Go: internal/transformers/estransforms/namedevaluation.go:transformNamedEvaluationOfVariableDeclaration
// Validation tracer for the 6d-2 emit-helper infrastructure: an anonymous
// function bound to `f` is wrapped in `__setFunctionName(..., "f")`, and the
// helper's definition is emitted once in the module prologue.
#[test]
fn anonymous_function_binding_gets_set_function_name() {
    check_downlevel(
        "var f = function () {};",
        "var __setFunctionName = (this && this.__setFunctionName) || function (f, name, prefix) {\n    if (typeof name === \"symbol\") name = name.description ? \"[\".concat(name.description, \"]\") : \"\";\n    return Object.defineProperty(f, \"name\", { configurable: true, value: prefix ? \"\".concat(prefix, \" \", name) : name });\n};\nvar f = __setFunctionName(function () { }, \"f\");",
    );
}
