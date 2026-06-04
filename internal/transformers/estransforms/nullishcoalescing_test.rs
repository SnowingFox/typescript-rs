use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

fn check_nullish(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut tx = new_nullish_coalescing_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "nullish({input:?})");
}

// Go: internal/transformers/estransforms/nullishcoalescing.go:visitBinaryExpression
// A simple identifier `??` is lowered to a ternary without a temp.
#[test]
fn identifier_nullish_no_temp() {
    check_nullish(
        "var x = a ?? b;",
        "var x = a !== null && a !== void 0 ? a : b;",
    );
}

// Go: internal/transformers/estransforms/nullishcoalescing.go:visitBinaryExpression
// Non-`??` binary expressions pass through unchanged.
#[test]
fn non_nullish_binary_unchanged() {
    check_nullish("var x = a + b;", "var x = a + b;");
}

// Go: internal/transformers/estransforms/nullishcoalescing.go:visit
// Nodes without binary expressions pass through.
#[test]
fn plain_statement_unchanged() {
    check_nullish("var x = 1;", "var x = 1;");
}

// ───────────────────────────────────────────────────────────────────────
// T2-8 integration tests: nullish coalescing verification
// ───────────────────────────────────────────────────────────────────────

// Go: internal/transformers/estransforms/nullishcoalescing.go:visitBinaryExpression
// `a ?? b` as a standalone expression statement, not just an initializer.
#[test]
fn nullish_coalescing_as_expression_statement() {
    check_nullish("a ?? b;", "a !== null && a !== void 0 ? a : b;");
}

// Go: internal/transformers/estransforms/nullishcoalescing.go:visitBinaryExpression
// A numeric-literal left operand is simple-copiable and lowered without a temp.
#[test]
fn numeric_literal_left_no_temp() {
    check_nullish(
        "var x = 0 ?? b;",
        "var x = 0 !== null && 0 !== void 0 ? 0 : b;",
    );
}

// ───────────────────────────────────────────────────────────────────────
// T2-10 integration tests: nullish coalescing verification
// ───────────────────────────────────────────────────────────────────────

// Go: internal/transformers/estransforms/nullishcoalescing.go:visitBinaryExpression
// Nested `??` — `a ?? b ?? c` lowers left-to-right. The inner `a ?? b` lowers
// to a conditional; because that conditional is NOT simple-copiable, the outer
// `?? c` introduces a temp `_a` to cache the inner result.
#[test]
fn nested_nullish_two_levels() {
    check_nullish(
        "var x = a ?? b ?? c;",
        "var x = (_a = a !== null && a !== void 0 ? a : b) !== null && _a !== void 0 ? _a : c;",
    );
}

// Go: internal/transformers/estransforms/nullishcoalescing.go:visitBinaryExpression
// A string-literal left operand is simple-copiable and lowered without a temp.
#[test]
fn string_literal_left_no_temp() {
    check_nullish(
        "var x = \"hello\" ?? b;",
        "var x = \"hello\" !== null && \"hello\" !== void 0 ? \"hello\" : b;",
    );
}

// Go: internal/transformers/estransforms/nullishcoalescing.go:visitBinaryExpression
// A `this` keyword left operand is simple-copiable (keyword kind).
#[test]
fn this_keyword_left_no_temp() {
    check_nullish(
        "var x = this ?? b;",
        "var x = this !== null && this !== void 0 ? this : b;",
    );
}
