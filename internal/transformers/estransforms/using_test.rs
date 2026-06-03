use super::*;
use tsgo_ast::NodeFlags;

// Go: internal/transformers/estransforms/using.go:getUsingKindOfVariableDeclarationList
#[test]
fn using_kind_sync() {
    assert_eq!(get_using_kind_of_flags(NodeFlags::USING), UsingKind::Sync);
}

#[test]
fn using_kind_async() {
    assert_eq!(
        get_using_kind_of_flags(NodeFlags::AWAIT_USING),
        UsingKind::Async
    );
}

#[test]
fn using_kind_none_for_const() {
    assert_eq!(get_using_kind_of_flags(NodeFlags::CONST), UsingKind::None);
}

#[test]
fn using_kind_none_for_let() {
    assert_eq!(get_using_kind_of_flags(NodeFlags::LET), UsingKind::None);
}

#[test]
fn using_kind_none_for_empty() {
    assert_eq!(get_using_kind_of_flags(NodeFlags::NONE), UsingKind::None);
}

// Go: internal/transformers/estransforms/using.go:newUsingDeclarationTransformer
// The transformer is a pass-through (DEFER until parser supports `using`).
#[test]
fn transformer_is_pass_through() {
    use crate::test_support::{emit, parse_shared};
    use std::rc::Rc;
    let (ec, source_file) = parse_shared("var x = 1;");
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut tx = new_using_declaration_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, "var x = 1;"), "var x = 1;");
}
