use super::*;
use crate::{NodeArena, NodeFlags, NodeId, NodeList};

// Build `a(b, c)` by hand and return (call_id, [callee, arg1, arg2]).
fn build_call(arena: &mut NodeArena) -> (NodeId, Vec<NodeId>) {
    let callee = arena.new_identifier("a");
    let arg1 = arena.new_identifier("b");
    let arg2 = arena.new_identifier("c");
    let call = arena.new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![arg1, arg2]),
        NodeFlags::NONE,
    );
    (call, vec![callee, arg1, arg2])
}

// Go: internal/ast/deepclone_test.go:getChildren (VisitEachChild identity collects children)
#[test]
fn get_children_matches_for_each_child() {
    let mut arena = NodeArena::new();
    let (call, _) = build_call(&mut arena);

    let mut via_for_each = Vec::new();
    arena.for_each_child(call, &mut |c| {
        via_for_each.push(c);
        false
    });

    let via_visit = arena.get_children(call);
    assert_eq!(via_visit, via_for_each);
}

// Go: internal/ast/visitor.go:NodeVisitor.VisitEachChild (identity visit returns same node)
#[test]
fn visit_each_child_identity_returns_same_node() {
    let mut arena = NodeArena::new();
    let (call, _children) = build_call(&mut arena);
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    let result = arena.visit_each_child(call, opts, &mut |_a, c| c);
    assert_eq!(result, call);
}
