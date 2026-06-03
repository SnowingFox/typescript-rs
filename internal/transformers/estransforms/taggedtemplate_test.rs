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
