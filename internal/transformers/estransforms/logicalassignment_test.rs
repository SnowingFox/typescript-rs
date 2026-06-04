use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

fn check_logical(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut tx = new_logical_assignment_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "logical({input:?})");
}

// Go: internal/transformers/estransforms/logicalassignment.go:visitBinaryExpression
// Simple identifier `||=` is lowered without a temp.
#[test]
fn or_equals_identifier() {
    check_logical("a ||= b;", "a || (a = b);");
}

// Go: internal/transformers/estransforms/logicalassignment.go:visitBinaryExpression
// Simple identifier `&&=` is lowered without a temp.
#[test]
fn and_equals_identifier() {
    check_logical("a &&= b;", "a && (a = b);");
}

// Go: internal/transformers/estransforms/logicalassignment.go:visitBinaryExpression
// Simple identifier `??=` is lowered without a temp.
#[test]
fn nullish_equals_identifier() {
    check_logical("a ??= b;", "a ?? (a = b);");
}

// Go: internal/transformers/estransforms/logicalassignment.go:visitBinaryExpression
// Non-logical-assignment binary expressions pass through.
#[test]
fn non_logical_assignment_unchanged() {
    check_logical("var x = a + b;", "var x = a + b;");
}

// Go: internal/transformers/estransforms/logicalassignment.go:visit
// Nodes without binary expressions pass through.
#[test]
fn plain_statement_unchanged() {
    check_logical("var x = 1;", "var x = 1;");
}

// ───────────────────────────────────────────────────────────────────────
// T2-10 integration tests: logical assignment verification
// ───────────────────────────────────────────────────────────────────────

// Go: internal/transformers/estransforms/logicalassignment.go:visitBinaryExpression
// Property access LHS: `a.x ||= b` → receiver `a` is simple-copiable, so no
// temp is needed.
#[test]
fn or_equals_property_access_simple_receiver() {
    check_logical("a.x ||= b;", "a.x || (a.x = b);");
}

// Go: internal/transformers/estransforms/logicalassignment.go:visitBinaryExpression
// Property access `&&=` with simple receiver.
#[test]
fn and_equals_property_access_simple_receiver() {
    check_logical("a.x &&= b;", "a.x && (a.x = b);");
}

// Go: internal/transformers/estransforms/logicalassignment.go:visitBinaryExpression
// Property access `??=` with simple receiver.
#[test]
fn nullish_equals_property_access_simple_receiver() {
    check_logical("a.x ??= b;", "a.x ?? (a.x = b);");
}

// Go: internal/transformers/estransforms/logicalassignment.go:visitBinaryExpression
// Element access `||=` with simple key: `a[0] ||= b` — both receiver and key
// are simple-copiable (identifier / literal), so no temp is needed.
#[test]
fn or_equals_element_access_simple_key() {
    check_logical("a[0] ||= b;", "a[0] || (a[0] = b);");
}

// Go: internal/transformers/estransforms/logicalassignment.go:visitBinaryExpression
// Regular addition assignment is not a logical assignment and passes through.
#[test]
fn plus_equals_passes_through() {
    check_logical("a += b;", "a += b;");
}
