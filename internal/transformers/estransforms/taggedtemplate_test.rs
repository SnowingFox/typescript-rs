use super::*;

// Go: internal/transformers/estransforms/taggedtemplate.go:safeMultiLineComment
#[test]
fn safe_comment_no_close() {
    assert_eq!(safe_multi_line_comment("hello"), " hello ");
}

#[test]
fn safe_comment_escapes_close() {
    assert_eq!(safe_multi_line_comment("a*/b"), " a*_/b ");
}

#[test]
fn safe_comment_multiple_closes() {
    assert_eq!(safe_multi_line_comment("a*/b*/c"), " a*_/b*_/c ");
}

#[test]
fn safe_comment_empty() {
    assert_eq!(safe_multi_line_comment(""), "  ");
}

// Go: internal/transformers/estransforms/taggedtemplate.go:newTaggedTemplateLiftRestrictionTransformer
// The transformer is a pass-through (DEFER until helpers are ported).
#[test]
fn transformer_is_pass_through() {
    use crate::test_support::{emit, parse_shared};
    use std::rc::Rc;
    let (ec, source_file) = parse_shared("var x = tag`hello`;");
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut tx = new_tagged_template_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(
        emit(&ec, result, "var x = tag`hello`;"),
        "var x = tag `hello`;"
    );
}

// ───────────────────────────────────────────────────────────────────────
// T2-10 integration tests: tagged template verification
// ───────────────────────────────────────────────────────────────────────

// Go: internal/transformers/estransforms/taggedtemplate.go:hasInvalidEscape
// A NoSubstitutionTemplateLiteral without an invalid escape returns false.
#[test]
fn has_invalid_escape_no_sub_clean() {
    use crate::test_support::parse_shared;
    let (ec, source_file) = parse_shared("`hello world`;");
    let ec_ref = ec.borrow();
    let arena = ec_ref.arena();
    let first_stmt = match arena.data(source_file) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("expected source file"),
    };
    let expr = match arena.data(first_stmt) {
        tsgo_ast::NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expected expression statement"),
    };
    assert!(!has_invalid_escape(arena, expr));
}

// Go: internal/transformers/estransforms/taggedtemplate.go:hasInvalidEscape
// A template expression without any invalid escapes returns false.
#[test]
fn has_invalid_escape_template_expr_clean() {
    use crate::test_support::parse_shared;
    let (ec, source_file) = parse_shared("`hello ${x} world`;");
    let ec_ref = ec.borrow();
    let arena = ec_ref.arena();
    let first_stmt = match arena.data(source_file) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("expected source file"),
    };
    let expr = match arena.data(first_stmt) {
        tsgo_ast::NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expected expression statement"),
    };
    assert!(!has_invalid_escape(arena, expr));
}

// Go: internal/transformers/estransforms/taggedtemplate.go:hasInvalidEscape
// A non-template node returns false (guard: unrelated kind).
#[test]
fn has_invalid_escape_non_template_node() {
    use crate::test_support::parse_shared;
    let (ec, source_file) = parse_shared("var x = 1;");
    let ec_ref = ec.borrow();
    let arena = ec_ref.arena();
    let first_stmt = match arena.data(source_file) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("expected source file"),
    };
    assert!(!has_invalid_escape(arena, first_stmt));
}

// Go: internal/transformers/estransforms/taggedtemplate.go:safeMultiLineComment
// A long text with adjacent close sequences.
#[test]
fn safe_comment_adjacent_closes() {
    assert_eq!(safe_multi_line_comment("*/*/"), " *_/*_/ ");
}

// Go: internal/transformers/estransforms/taggedtemplate.go:newTaggedTemplateLiftRestrictionTransformer
// Template expression with substitution passes through unchanged (DEFER).
#[test]
fn template_with_substitution_pass_through() {
    use crate::test_support::{emit, parse_shared};
    use std::rc::Rc;
    let input = "var x = tag`hello ${y} world`;";
    let (ec, source_file) = parse_shared(input);
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut tx = new_tagged_template_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "var x = tag `hello ${y} world`;");
}
