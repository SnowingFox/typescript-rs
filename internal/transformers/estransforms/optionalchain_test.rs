use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the optional-chain transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_optional_chain_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
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
