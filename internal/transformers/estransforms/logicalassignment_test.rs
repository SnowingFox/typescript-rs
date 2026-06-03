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
