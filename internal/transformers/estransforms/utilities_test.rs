//! Tests for `estransforms/utilities.rs`.

use super::*;
use tsgo_ast::{Kind, NodeArena, NodeData};

#[test]
fn create_not_null_condition_normal() {
    let mut ec = tsgo_printer::EmitContext::new();
    let left = ec.arena_mut().new_identifier("a");
    let right = ec.arena_mut().new_identifier("b");
    let result = create_not_null_condition(&mut ec, left, right, false);
    assert_eq!(ec.arena().kind(result), Kind::BinaryExpression);
    let outer = match ec.arena().data(result) {
        NodeData::BinaryExpression(d) => d,
        _ => panic!("expected binary"),
    };
    assert_eq!(
        ec.arena().kind(outer.operator_token),
        Kind::AmpersandAmpersandToken
    );
    let left_cmp = match ec.arena().data(outer.left) {
        NodeData::BinaryExpression(d) => d,
        _ => panic!("expected binary"),
    };
    assert_eq!(
        ec.arena().kind(left_cmp.operator_token),
        Kind::ExclamationEqualsEqualsToken
    );
    assert_eq!(ec.arena().kind(left_cmp.right), Kind::NullKeyword);
}

#[test]
fn create_not_null_condition_inverted() {
    let mut ec = tsgo_printer::EmitContext::new();
    let left = ec.arena_mut().new_identifier("x");
    let right = ec.arena_mut().new_identifier("y");
    let result = create_not_null_condition(&mut ec, left, right, true);
    let outer = match ec.arena().data(result) {
        NodeData::BinaryExpression(d) => d,
        _ => panic!("expected binary"),
    };
    assert_eq!(ec.arena().kind(outer.operator_token), Kind::BarBarToken);
    let left_cmp = match ec.arena().data(outer.left) {
        NodeData::BinaryExpression(d) => d,
        _ => panic!("expected binary"),
    };
    assert_eq!(
        ec.arena().kind(left_cmp.operator_token),
        Kind::EqualsEqualsEqualsToken
    );
}

#[test]
fn create_not_null_condition_right_is_void_zero() {
    let mut ec = tsgo_printer::EmitContext::new();
    let left = ec.arena_mut().new_identifier("a");
    let right = ec.arena_mut().new_identifier("b");
    let result = create_not_null_condition(&mut ec, left, right, false);
    let outer = match ec.arena().data(result) {
        NodeData::BinaryExpression(d) => d,
        _ => panic!("expected binary"),
    };
    let right_cmp = match ec.arena().data(outer.right) {
        NodeData::BinaryExpression(d) => d,
        _ => panic!("expected binary"),
    };
    assert_eq!(ec.arena().kind(right_cmp.right), Kind::VoidExpression);
}

#[test]
fn is_update_expression_identifies_increment_decrement() {
    let mut arena = NodeArena::new();
    let operand = arena.new_identifier("x");
    let prefix_inc = arena.new_prefix_unary_expression(Kind::PlusPlusToken, operand);
    assert!(is_update_expression(&arena, prefix_inc));

    let prefix_minus = arena.new_prefix_unary_expression(Kind::MinusToken, operand);
    assert!(!is_update_expression(&arena, prefix_minus));

    let postfix_dec = arena.new_postfix_unary_expression(operand, Kind::MinusMinusToken);
    assert!(is_update_expression(&arena, postfix_dec));
}

#[test]
fn assignment_target_with_super_property_access() {
    let mut arena = NodeArena::new();
    let super_kw = arena.new_keyword_expression(Kind::SuperKeyword);
    let name = arena.new_identifier("foo");
    let prop_access = arena.new_property_access_expression(super_kw, None, name);
    assert!(assignment_target_contains_super_property(
        &arena,
        prop_access
    ));
}

#[test]
fn assignment_target_with_super_element_access() {
    let mut arena = NodeArena::new();
    let super_kw = arena.new_keyword_expression(Kind::SuperKeyword);
    let idx = arena.new_identifier("x");
    let elem_access = arena.new_element_access_expression(super_kw, None, idx);
    assert!(assignment_target_contains_super_property(
        &arena,
        elem_access
    ));
}

#[test]
fn assignment_target_without_super_returns_false() {
    let mut arena = NodeArena::new();
    let obj = arena.new_identifier("obj");
    let name = arena.new_identifier("foo");
    let prop_access = arena.new_property_access_expression(obj, None, name);
    assert!(!assignment_target_contains_super_property(
        &arena,
        prop_access
    ));
}

#[test]
fn super_access_state_tracks_property_access() {
    let mut arena = NodeArena::new();
    let super_kw = arena.new_keyword_expression(Kind::SuperKeyword);
    let name = arena.new_identifier("bar");
    let prop_access = arena.new_property_access_expression(super_kw, None, name);

    let mut state = SuperAccessState::new();
    state.track_super_access(&arena, prop_access);
    assert!(state.captured_super_properties.contains(&"bar".to_string()));
    assert!(!state.has_super_element_access);
    assert!(!state.has_super_property_assignment);
}

#[test]
fn super_access_state_tracks_element_access() {
    let mut arena = NodeArena::new();
    let super_kw = arena.new_keyword_expression(Kind::SuperKeyword);
    let idx = arena.new_identifier("x");
    let elem_access = arena.new_element_access_expression(super_kw, None, idx);

    let mut state = SuperAccessState::new();
    state.track_super_access(&arena, elem_access);
    assert!(state.has_super_element_access);
    assert!(state.captured_super_properties.is_empty());
}

#[test]
fn super_access_state_default_is_empty() {
    let state = SuperAccessState::default();
    assert!(state.captured_super_properties.is_empty());
    assert!(!state.has_super_element_access);
    assert!(!state.has_super_property_assignment);
}
