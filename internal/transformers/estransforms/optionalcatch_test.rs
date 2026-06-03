use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

fn check_optional_catch(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut tx = new_optional_catch_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(
        emit(&ec, result, input),
        expected,
        "optional_catch({input:?})"
    );
}

// Go: internal/transformers/estransforms/optionalcatch.go:visitCatchClause
// A `catch` clause with no binding variable gains a synthesized temp variable.
#[test]
fn catch_without_binding_gets_temp_variable() {
    check_optional_catch("try { } catch { }", "try { }\ncatch (_a) { }");
}

// Go: internal/transformers/estransforms/optionalcatch.go:visitCatchClause
// A `catch` clause that already has a binding variable is left unchanged.
#[test]
fn catch_with_binding_is_unchanged() {
    check_optional_catch("try { } catch (e) { }", "try { }\ncatch (e) { }");
}

// Go: internal/transformers/estransforms/optionalcatch.go:visit
// When there are no catch clauses, nodes pass through unchanged.
#[test]
fn non_catch_nodes_pass_through() {
    check_optional_catch("var x = 1;", "var x = 1;");
}
