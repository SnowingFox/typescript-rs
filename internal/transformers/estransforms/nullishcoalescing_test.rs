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
