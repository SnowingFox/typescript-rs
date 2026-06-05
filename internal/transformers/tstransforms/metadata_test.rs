//! Tests for `tstransforms/metadata.rs`.

use super::*;
use crate::test_support::{emit, parse_shared};
use crate::TransformOptions;
use tsgo_ast::Kind;

#[test]
fn should_add_type_metadata_for_method() {
    assert!(should_add_type_metadata(Kind::MethodDeclaration));
}

#[test]
fn should_add_type_metadata_for_property() {
    assert!(should_add_type_metadata(Kind::PropertyDeclaration));
}

#[test]
fn should_add_type_metadata_for_accessors() {
    assert!(should_add_type_metadata(Kind::GetAccessor));
    assert!(should_add_type_metadata(Kind::SetAccessor));
}

#[test]
fn should_not_add_type_metadata_for_class() {
    assert!(!should_add_type_metadata(Kind::ClassDeclaration));
    assert!(!should_add_type_metadata(Kind::ClassExpression));
}

#[test]
fn should_add_return_type_metadata_only_for_method() {
    assert!(should_add_return_type_metadata(Kind::MethodDeclaration));
    assert!(!should_add_return_type_metadata(Kind::GetAccessor));
    assert!(!should_add_return_type_metadata(Kind::PropertyDeclaration));
}

#[test]
fn new_metadata_transformer_passthrough() {
    let src = "let x = 1;";
    let (ec, sf) = parse_shared(src);
    let opts = TransformOptions {
        context: Some(ec.clone()),
        ..Default::default()
    };
    let mut tx = new_metadata_transformer(&opts);
    let result = tx.run_visit(&mut ec.borrow_mut(), sf);
    let text = emit(&ec, result, src);
    assert_eq!(text, "let x = 1;");
}

#[test]
fn new_metadata_transformer_class_passthrough() {
    let src = "class Foo { bar: number; }";
    let (ec, sf) = parse_shared(src);
    let opts = TransformOptions {
        context: Some(ec.clone()),
        ..Default::default()
    };
    let mut tx = new_metadata_transformer(&opts);
    let result = tx.run_visit(&mut ec.borrow_mut(), sf);
    let text = emit(&ec, result, src);
    assert!(text.contains("class Foo"));
}
