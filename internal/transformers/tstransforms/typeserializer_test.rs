//! Tests for `tstransforms/typeserializer.rs`.

use super::*;
use tsgo_ast::{Kind, NodeArena, TokenFlags};

#[test]
fn serialize_none_returns_object() {
    let mut arena = NodeArena::new();
    let result = serialize_type_node(&mut arena, None);
    assert_eq!(arena.kind(result), Kind::Identifier);
    assert_eq!(arena.text(result), "Object");
}

#[test]
fn serialize_void_keyword_returns_void_zero() {
    let mut arena = NodeArena::new();
    let void_kw = arena.new_keyword_expression(Kind::VoidKeyword);
    let result = serialize_type_node(&mut arena, Some(void_kw));
    assert_eq!(arena.kind(result), Kind::VoidExpression);
}

#[test]
fn serialize_undefined_keyword_returns_void_zero() {
    let mut arena = NodeArena::new();
    let undef = arena.new_keyword_expression(Kind::UndefinedKeyword);
    let result = serialize_type_node(&mut arena, Some(undef));
    assert_eq!(arena.kind(result), Kind::VoidExpression);
}

#[test]
fn serialize_never_keyword_returns_void_zero() {
    let mut arena = NodeArena::new();
    let never = arena.new_keyword_expression(Kind::NeverKeyword);
    let result = serialize_type_node(&mut arena, Some(never));
    assert_eq!(arena.kind(result), Kind::VoidExpression);
}

#[test]
fn serialize_boolean_keyword_returns_boolean() {
    let mut arena = NodeArena::new();
    let bool_kw = arena.new_keyword_expression(Kind::BooleanKeyword);
    let result = serialize_type_node(&mut arena, Some(bool_kw));
    assert_eq!(arena.kind(result), Kind::Identifier);
    assert_eq!(arena.text(result), "Boolean");
}

#[test]
fn serialize_string_keyword_returns_string() {
    let mut arena = NodeArena::new();
    let str_kw = arena.new_keyword_expression(Kind::StringKeyword);
    let result = serialize_type_node(&mut arena, Some(str_kw));
    assert_eq!(arena.kind(result), Kind::Identifier);
    assert_eq!(arena.text(result), "String");
}

#[test]
fn serialize_number_keyword_returns_number() {
    let mut arena = NodeArena::new();
    let num_kw = arena.new_keyword_expression(Kind::NumberKeyword);
    let result = serialize_type_node(&mut arena, Some(num_kw));
    assert_eq!(arena.kind(result), Kind::Identifier);
    assert_eq!(arena.text(result), "Number");
}

#[test]
fn serialize_bigint_keyword_returns_bigint() {
    let mut arena = NodeArena::new();
    let bi_kw = arena.new_keyword_expression(Kind::BigIntKeyword);
    let result = serialize_type_node(&mut arena, Some(bi_kw));
    assert_eq!(arena.kind(result), Kind::Identifier);
    assert_eq!(arena.text(result), "BigInt");
}

#[test]
fn serialize_symbol_keyword_returns_symbol() {
    let mut arena = NodeArena::new();
    let sym_kw = arena.new_keyword_expression(Kind::SymbolKeyword);
    let result = serialize_type_node(&mut arena, Some(sym_kw));
    assert_eq!(arena.kind(result), Kind::Identifier);
    assert_eq!(arena.text(result), "Symbol");
}

#[test]
fn serialize_object_keyword_returns_object() {
    let mut arena = NodeArena::new();
    let obj_kw = arena.new_keyword_expression(Kind::ObjectKeyword);
    let result = serialize_type_node(&mut arena, Some(obj_kw));
    assert_eq!(arena.kind(result), Kind::Identifier);
    assert_eq!(arena.text(result), "Object");
}

#[test]
fn serialize_any_keyword_returns_object() {
    let mut arena = NodeArena::new();
    let any_kw = arena.new_keyword_expression(Kind::AnyKeyword);
    let result = serialize_type_node(&mut arena, Some(any_kw));
    assert_eq!(arena.kind(result), Kind::Identifier);
    assert_eq!(arena.text(result), "Object");
}

#[test]
fn serialize_literal_type_string_returns_string() {
    let mut arena = NodeArena::new();
    let str_lit = arena.new_string_literal("hello", TokenFlags::NONE);
    let lit_type = arena.new_literal_type_node(str_lit);
    let result = serialize_type_node(&mut arena, Some(lit_type));
    assert_eq!(arena.kind(result), Kind::Identifier);
    assert_eq!(arena.text(result), "String");
}

#[test]
fn serialize_literal_type_number_returns_number() {
    let mut arena = NodeArena::new();
    let num_lit = arena.new_numeric_literal("42", TokenFlags::NONE);
    let lit_type = arena.new_literal_type_node(num_lit);
    let result = serialize_type_node(&mut arena, Some(lit_type));
    assert_eq!(arena.kind(result), Kind::Identifier);
    assert_eq!(arena.text(result), "Number");
}

#[test]
fn serialize_literal_type_true_returns_boolean() {
    let mut arena = NodeArena::new();
    let true_kw = arena.new_keyword_expression(Kind::TrueKeyword);
    let lit_type = arena.new_literal_type_node(true_kw);
    let result = serialize_type_node(&mut arena, Some(lit_type));
    assert_eq!(arena.kind(result), Kind::Identifier);
    assert_eq!(arena.text(result), "Boolean");
}

#[test]
fn serialize_literal_type_null_returns_void_zero() {
    let mut arena = NodeArena::new();
    let null_kw = arena.new_keyword_expression(Kind::NullKeyword);
    let lit_type = arena.new_literal_type_node(null_kw);
    let result = serialize_type_node(&mut arena, Some(lit_type));
    assert_eq!(arena.kind(result), Kind::VoidExpression);
}
