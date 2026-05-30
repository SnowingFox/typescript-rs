use crate::{NodeArena, NodeFlags, NodeId, NodeList};
use tsgo_core::text::TextRange;

// The full Go `TestDeepCloneNodeSanityCheck` (~270 table cases) builds trees via
// `parsetestutil.ParseTypeScript`, which depends on the parser. Because the
// `ast` crate cannot depend on `tsgo_parser` (that would invert the dependency
// edge), the parser-backed port of that table lives in
// `internal/parser/deepclone_test.rs` (it covers the node kinds the parser
// currently produces and grows as more productions are ported). The hand-built
// trees below keep `ast`-local coverage of the same clone invariants
// (distinct ids + matching child counts, synthetic vs reparse locations).

// BFS the original/clone in lockstep, asserting every pair is distinct and has
// the same number of children — the invariant from the Go sanity check.
// Go: internal/ast/deepclone_test.go:TestDeepCloneNodeSanityCheck
fn assert_deep_clone_structure(arena: &mut NodeArena, original: NodeId, clone: NodeId) {
    let mut work = vec![(original, clone)];
    while let Some((o, c)) = work.pop() {
        assert_ne!(o, c, "clone must produce a distinct node id");
        let oc = arena.get_children(o);
        let cc = arena.get_children(c);
        assert_eq!(oc.len(), cc.len(), "child counts must match");
        for (oi, ci) in oc.into_iter().zip(cc.into_iter()) {
            work.push((oi, ci));
        }
    }
}

// Go: internal/ast/deepclone_test.go:TestDeepCloneNodeSanityCheck (leaf)
#[test]
fn deep_clone_identifier_is_new_node() {
    let mut arena = NodeArena::new();
    let id = arena.new_identifier("a");
    let clone = arena.deep_clone_node(id);
    assert_ne!(id, clone);
    assert_eq!(arena.text(clone), "a");
}

// Go: internal/ast/deepclone_test.go:TestDeepCloneNodeSanityCheck/Clone PropertyAccess
#[test]
fn deep_clone_property_access_children() {
    let mut arena = NodeArena::new();
    let obj = arena.new_identifier("a");
    let name = arena.new_identifier("b");
    let pa = arena.new_property_access_expression(obj, None, name);
    let clone = arena.deep_clone_node(pa);
    assert_deep_clone_structure(&mut arena, pa, clone);
}

// Go: internal/ast/deepclone_test.go:TestDeepCloneNodeSanityCheck/Clone CallExpression
#[test]
fn deep_clone_call_with_args() {
    let mut arena = NodeArena::new();
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
    let clone = arena.deep_clone_node(call);
    assert_deep_clone_structure(&mut arena, call, clone);
    // The cloned argument ids differ from the originals.
    let clone_args = arena.get_children(clone);
    assert!(!clone_args.contains(&arg1));
    assert!(!clone_args.contains(&arg2));
}

// Go: internal/ast/deepclone.go:getDeepCloneVisitor (syntheticLocation branch)
#[test]
fn deep_clone_synthetic_location() {
    let mut arena = NodeArena::new();
    // Build `[a,]` with a trailing comma: element ends before the list end.
    let a = arena.new_identifier("a");
    arena.set_loc(a, TextRange::new(1, 2));
    let mut elements = NodeList::new(vec![a]);
    elements.loc = TextRange::new(0, 4); // list end (4) > element end (2) => trailing comma
    let arr = arena.new_array_literal_expression(elements);
    arena.set_loc(arr, TextRange::new(0, 5));

    let clone = arena.deep_clone_node(arr);
    // The cloned array node has a synthetic (-1, -1) location.
    assert_eq!(arena.loc(clone), TextRange::new(-1, -1));
    // Its single (trailing-comma) element gets the (-2, -2) marker.
    let clone_children = arena.get_children(clone);
    assert_eq!(clone_children.len(), 1);
    assert_eq!(arena.loc(clone_children[0]), TextRange::new(-2, -2));
}

// Go: internal/ast/deepclone.go:DeepCloneReparse
#[test]
fn deep_clone_reparse_sets_parent_and_flag() {
    let mut arena = NodeArena::new();
    let obj = arena.new_identifier("a");
    arena.set_loc(obj, TextRange::new(0, 1));
    let name = arena.new_identifier("b");
    arena.set_loc(name, TextRange::new(2, 3));
    let pa = arena.new_property_access_expression(obj, None, name);
    arena.set_loc(pa, TextRange::new(0, 3));

    let clone = arena.deep_clone_reparse(pa);
    assert_ne!(clone, pa);
    // Reparse keeps real (non-synthetic) locations.
    assert_eq!(arena.loc(clone), TextRange::new(0, 3));
    // Root is flagged as reparsed.
    assert!(arena.flags(clone).contains(NodeFlags::REPARSED));
    // Children point back to the cloned parent.
    let children = arena.get_children(clone);
    for child in children {
        assert_eq!(arena.parent(child), Some(clone));
    }
}
