//! Tests for `moduletransforms/utilities.rs`.

use super::*;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeList, TokenFlags};
use tsgo_printer::EmitContext;

#[test]
fn create_empty_imports_produces_export_declaration() {
    let mut ec = EmitContext::new();
    let result = create_empty_imports(&mut ec);
    assert_eq!(ec.arena().kind(result), Kind::ExportDeclaration);
    let decl = match ec.arena().data(result) {
        NodeData::ExportDeclaration(d) => d,
        _ => panic!("expected ExportDeclaration"),
    };
    assert!(!decl.is_type_only);
    assert!(decl.module_specifier.is_none());
    let export_clause = decl.export_clause.expect("should have export clause");
    assert_eq!(ec.arena().kind(export_clause), Kind::NamedExports);
}

#[test]
fn is_simple_inlineable_string_literal() {
    let mut arena = NodeArena::new();
    let lit = arena.new_string_literal("hello", TokenFlags::NONE);
    assert!(is_simple_inlineable_expression(&arena, lit));
}

#[test]
fn is_simple_inlineable_identifier_is_false() {
    let mut arena = NodeArena::new();
    let id = arena.new_identifier("x");
    assert!(!is_simple_inlineable_expression(&arena, id));
}

#[test]
fn is_simple_inlineable_numeric_literal() {
    let mut arena = NodeArena::new();
    let num = arena.new_numeric_literal("42", TokenFlags::NONE);
    assert!(is_simple_inlineable_expression(&arena, num));
}

#[test]
fn is_simple_inlineable_true_keyword() {
    let mut arena = NodeArena::new();
    let kw = arena.new_keyword_expression(Kind::TrueKeyword);
    assert!(is_simple_inlineable_expression(&arena, kw));
}

#[test]
fn is_simple_inlineable_void_zero() {
    let mut arena = NodeArena::new();
    let zero = arena.new_numeric_literal("0", TokenFlags::NONE);
    let void_zero = arena.new_void_expression(zero);
    assert!(is_simple_inlineable_expression(&arena, void_zero));
}

#[test]
fn is_simple_inlineable_call_expression_is_false() {
    let mut arena = NodeArena::new();
    let callee = arena.new_identifier("f");
    let call = arena.new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![]),
        tsgo_ast::NodeFlags::NONE,
    );
    assert!(!is_simple_inlineable_expression(&arena, call));
}

#[test]
fn get_external_module_name_from_path_returns_empty() {
    assert_eq!(get_external_module_name_from_path(&(), "/a.ts", "/"), "");
}

#[test]
fn try_rename_external_module_returns_none() {
    let mut arena = NodeArena::new();
    let lit = arena.new_string_literal("mod", TokenFlags::NONE);
    let sf = arena.new_identifier("fake"); // placeholder
    assert!(try_rename_external_module(&mut arena, lit, sf).is_none());
}
