use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the object-rest-spread transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_object_rest_spread_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// Go: internal/transformers/estransforms/objectrestspread.go:visitObjectLiteralExpression
// Tracer bullet: a lone object spread `{ ...x }` lowers to `Object.assign({}, x)`.
#[test]
fn object_spread_only_lowers_to_assign() {
    check_downlevel("const o = { ...x };", "const o = Object.assign({}, x);");
}

// Go: internal/transformers/estransforms/objectrestspread.go:chunkObjectLiteralElements
// A spread followed by a property chunks into `Object.assign(Object.assign({}, x), { y })`.
#[test]
fn spread_then_property_chunks_pairwise() {
    check_downlevel(
        "const o = { ...x, y };",
        "const o = Object.assign(Object.assign({}, x), { y });",
    );
}

// Go: internal/transformers/estransforms/objectrestspread.go:chunkObjectLiteralElements
// A leading property chunk is the assign target (no synthetic `{}` prepended):
// `{ a, ...x }` -> `Object.assign({ a }, x)`.
#[test]
fn property_then_spread_uses_chunk_as_target() {
    check_downlevel(
        "const o = { a, ...x };",
        "const o = Object.assign({ a }, x);",
    );
}
