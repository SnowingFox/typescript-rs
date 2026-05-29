use super::*;

// Go: internal/ast/ast_generated.go:NewIdentifier
#[test]
fn new_identifier_sets_kind_and_text() {
    let mut arena = NodeArena::new();
    let id = arena.new_identifier("foo");
    assert_eq!(arena.kind(id), Kind::Identifier);
    assert_eq!(arena.text(id), "foo");
    assert_eq!(arena.node_count(), 1);
    assert_eq!(arena.text_count(), 1);
}

fn collect_children(arena: &NodeArena, id: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    arena.for_each_child(id, &mut |c| {
        out.push(c);
        false
    });
    out
}

// Go: internal/ast/ast_generated.go:QualifiedName.ForEachChild
#[test]
fn for_each_child_visits_qualified_name_left_then_right() {
    let mut arena = NodeArena::new();
    let left = arena.new_identifier("a");
    let right = arena.new_identifier("b");
    let qn = arena.new_qualified_name(left, right);
    assert_eq!(arena.kind(qn), Kind::QualifiedName);
    assert_eq!(collect_children(&arena, qn), vec![left, right]);
    // Leaves have no children.
    assert_eq!(collect_children(&arena, left), Vec::<NodeId>::new());
}

// Go: internal/ast/ast_generated.go:PropertyAccessExpression.ForEachChild
#[test]
fn for_each_child_property_access_skips_absent_optional() {
    let mut arena = NodeArena::new();
    let obj = arena.new_identifier("a");
    let name = arena.new_identifier("b");
    // No `?.` token.
    let pa = arena.new_property_access_expression(obj, None, name);
    assert_eq!(collect_children(&arena, pa), vec![obj, name]);
    // With `?.` token present, it is visited between expression and name.
    let obj2 = arena.new_identifier("a");
    let q = arena.new_token(Kind::QuestionDotToken);
    let name2 = arena.new_identifier("b");
    let pa2 = arena.new_property_access_expression(obj2, Some(q), name2);
    assert_eq!(collect_children(&arena, pa2), vec![obj2, q, name2]);
}

// Go: internal/ast/ast_generated.go:CallExpression.ForEachChild
#[test]
fn for_each_child_call_expression_visits_list_elements() {
    let mut arena = NodeArena::new();
    let callee = arena.new_identifier("a");
    let arg1 = arena.new_identifier("b");
    let arg2 = arena.new_identifier("c");
    let args = NodeList::new(vec![arg1, arg2]);
    let call = arena.new_call_expression(callee, None, None, args, NodeFlags::NONE);
    assert_eq!(arena.kind(call), Kind::CallExpression);
    assert_eq!(collect_children(&arena, call), vec![callee, arg1, arg2]);
}

// Go: internal/ast/ast_generated.go:BinaryExpression.ForEachChild
#[test]
fn for_each_child_binary_expression() {
    let mut arena = NodeArena::new();
    let l = arena.new_identifier("a");
    let op = arena.new_token(Kind::PlusToken);
    let r = arena.new_identifier("b");
    let bin = arena.new_binary_expression(l, op, r);
    assert_eq!(collect_children(&arena, bin), vec![l, op, r]);
}

// Go: internal/ast/ast_generated.go:Identifier.Clone
#[test]
fn clone_node_leaf_is_new_id_same_data() {
    let mut arena = NodeArena::new();
    let id = arena.new_identifier("foo");
    let c = arena.clone_node(id);
    assert_ne!(id, c);
    assert_eq!(arena.text(c), "foo");
    assert_eq!(arena.kind(c), Kind::Identifier);
    // A fresh clone has no parent yet.
    assert_eq!(arena.parent(c), None);
}

// Go: internal/ast/ast_generated.go:QualifiedName.Clone (shallow: shares children)
#[test]
fn clone_node_shares_children_shallowly() {
    let mut arena = NodeArena::new();
    let left = arena.new_identifier("a");
    let right = arena.new_identifier("b");
    let qn = arena.new_qualified_name(left, right);
    let c = arena.clone_node(qn);
    assert_ne!(qn, c);
    // Shallow clone keeps the same child ids.
    assert_eq!(collect_children(&arena, c), vec![left, right]);
}

// Go: internal/ast/ast_generated.go:ArrayLiteralExpression.ForEachChild
#[test]
fn for_each_child_array_literal_and_block_lists() {
    let mut arena = NodeArena::new();
    let e1 = arena.new_identifier("a");
    let e2 = arena.new_identifier("b");
    let arr = arena.new_array_literal_expression(NodeList::new(vec![e1, e2]));
    assert_eq!(collect_children(&arena, arr), vec![e1, e2]);

    let s1 = arena.new_expression_statement(e1);
    let block = arena.new_block(NodeList::new(vec![s1]));
    assert_eq!(collect_children(&arena, block), vec![s1]);
}
