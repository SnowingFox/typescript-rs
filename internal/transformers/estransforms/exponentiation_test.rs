use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the exponentiation transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_exponentiation_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// Go: internal/transformers/estransforms/exponentiation.go:visitExponentiationExpression
// Tracer bullet: a real es-downlevel stage runs through the driver and lowers
// `**` to a `Math.pow` call.
#[test]
fn exponentiation_operator_lowered_to_math_pow() {
    check_downlevel("a ** b", "Math.pow(a, b);");
}

// Go: internal/transformers/estransforms/exponentiation.go:visitExponentiationAssignmentExpression
// `a **= b` with a simple identifier target lowers to `a = Math.pow(a, b)`.
#[test]
fn exponentiation_assignment_to_identifier_lowered() {
    check_downlevel("a **= b", "a = Math.pow(a, b);");
}

// Go: internal/transformers/estransforms/exponentiation.go:visitExponentiationAssignmentExpression
// `a.x **= b` hoists a temp for the receiver:
// `(_a = a).x = Math.pow(_a.x, b)` with a hoisted `var _a;`.
#[test]
fn exponentiation_assignment_to_property_access_hoists_temp() {
    check_downlevel("a.x **= b", "var _a;\n(_a = a).x = Math.pow(_a.x, b);");
}

// Go: internal/transformers/estransforms/exponentiation.go:visitExponentiationAssignmentExpression
// `a[x] **= b` hoists temps for the receiver and the index:
// `(_a = a)[_b = x] = Math.pow(_a[_b], b)` with `var _a, _b;`.
#[test]
fn exponentiation_assignment_to_element_access_hoists_temps() {
    check_downlevel(
        "a[x] **= b",
        "var _a, _b;\n(_a = a)[_b = x] = Math.pow(_a[_b], b);",
    );
}

// Go: internal/printer/emitcontext.go:EmitContext.VisitFunctionBody (per-scope variable environment)
// 6i: a `**=` whose property-access target needs a hoisted temp inside a
// function body must hoist its `var _a;` INTO that function's body, not at
// module top. Before 6i this was DEFER'd (left verbatim) because the arena-only
// descent into non-top-level scopes had no active variable environment.
//
// The body prints single-line (synthesized `Block`, no `Block.MultiLine`, as in
// 6c-1); the behavior under test is that `var _a;` lands inside `f`'s braces.
#[test]
fn property_assignment_inside_function_body_hoists_into_body() {
    check_downlevel(
        "function f() { a.x **= b; }",
        "function f() { var _a; (_a = a).x = Math.pow(_a.x, b); }",
    );
}
