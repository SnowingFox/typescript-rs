use super::*;
use crate::{Kind, NodeArena, NodeFlags, NodeList};

// Go: internal/ast/utilities.go:SetParentInChildren
#[test]
fn set_parent_in_children_wires_parents_recursively() {
    let mut arena = NodeArena::new();
    let obj = arena.new_identifier("a");
    let name = arena.new_identifier("b");
    let pa = arena.new_property_access_expression(obj, None, name);
    // Wrap in a call so we get a second level: `a.b()`.
    let call = arena.new_call_expression(pa, None, None, NodeList::new(vec![]), NodeFlags::NONE);

    arena.set_parent_in_children(call);

    // Root parent is left untouched.
    assert_eq!(arena.parent(call), None);
    assert_eq!(arena.parent(pa), Some(call));
    assert_eq!(arena.parent(obj), Some(pa));
    assert_eq!(arena.parent(name), Some(pa));
}

// Go: internal/ast/ast_generated.go:IsIdentifier / IsCallExpression / IsPropertyAccessExpression
#[test]
fn kind_predicates() {
    let mut arena = NodeArena::new();
    let id = arena.new_identifier("a");
    let name = arena.new_identifier("b");
    let pa = arena.new_property_access_expression(id, None, name);
    let call = arena.new_call_expression(pa, None, None, NodeList::new(vec![]), NodeFlags::NONE);

    assert!(is_identifier(&arena, id));
    assert!(!is_identifier(&arena, call));
    assert!(is_call_expression(&arena, call));
    assert!(is_property_access_expression(&arena, pa));
    assert!(!is_property_access_expression(&arena, call));
    assert_eq!(arena.kind(pa), Kind::PropertyAccessExpression);
}
