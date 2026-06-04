use super::*;
use crate::test_support::parse_shared;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId};

// Go: internal/transformers/estransforms/classthis.go:isClassThisAssignmentBlock
// A non-ClassStaticBlockDeclaration node returns false.
#[test]
fn non_class_static_block_returns_false() {
    let (ec, source_file) = parse_shared("var x = 1;");
    let ec_ref = ec.borrow();
    let first_stmt = match ec_ref.arena().data(source_file) {
        NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("expected source file"),
    };
    assert!(!is_class_this_assignment_block(&ec_ref, first_stmt));
}

// Go: internal/transformers/estransforms/classthis.go:isClassThisAssignmentBlock
// A class with a static block that is NOT a simple `_classThis = this`
// assignment returns false.
#[test]
fn class_with_non_this_static_block_returns_false() {
    let (ec, source_file) = parse_shared("class C { static { console.log(1); } }");
    let ec_ref = ec.borrow();

    let static_block = find_first_kind(
        ec_ref.arena(),
        source_file,
        Kind::ClassStaticBlockDeclaration,
    );
    if let Some(sb) = static_block {
        assert!(!is_class_this_assignment_block(&ec_ref, sb));
    }
}

// Go: internal/transformers/estransforms/classthis.go:isClassThisAssignmentBlock
// DEFER(P5): A class with `static { _classThis = this; }` would return true
// once EmitContext::class_this is ported. Currently returns false (conservative).
#[test]
fn class_this_assignment_returns_false_until_emit_context_ported() {
    let (ec, source_file) = parse_shared("class C { static { _classThis = this; } }");
    let ec_ref = ec.borrow();
    let static_block = find_first_kind(
        ec_ref.arena(),
        source_file,
        Kind::ClassStaticBlockDeclaration,
    );
    if let Some(sb) = static_block {
        assert!(!is_class_this_assignment_block(&ec_ref, sb));
    }
}

fn find_first_kind(arena: &NodeArena, root: NodeId, target: Kind) -> Option<NodeId> {
    if arena.kind(root) == target {
        return Some(root);
    }
    let mut result: Option<NodeId> = None;
    arena.for_each_child(root, &mut |child| {
        if result.is_none() {
            result = find_first_kind(arena, child, target);
        }
        result.is_some()
    });
    result
}

// ───────────────────────────────────────────────────────────────────────
// T2-10 integration tests: classthis verification
// ───────────────────────────────────────────────────────────────────────

// Go: internal/transformers/estransforms/classthis.go:isClassThisAssignmentBlock
// A class with an empty static block (zero statements) returns false.
#[test]
fn class_with_empty_static_block_returns_false() {
    let (ec, source_file) = parse_shared("class C { static { } }");
    let ec_ref = ec.borrow();
    let static_block = find_first_kind(
        ec_ref.arena(),
        source_file,
        Kind::ClassStaticBlockDeclaration,
    );
    if let Some(sb) = static_block {
        assert!(!is_class_this_assignment_block(&ec_ref, sb));
    }
}

// Go: internal/transformers/estransforms/classthis.go:isClassThisAssignmentBlock
// A class with a static block containing two statements returns false
// (must be exactly one assignment statement).
#[test]
fn class_with_multi_statement_static_block_returns_false() {
    let (ec, source_file) =
        parse_shared("class C { static { _classThis = this; console.log(1); } }");
    let ec_ref = ec.borrow();
    let static_block = find_first_kind(
        ec_ref.arena(),
        source_file,
        Kind::ClassStaticBlockDeclaration,
    );
    if let Some(sb) = static_block {
        assert!(!is_class_this_assignment_block(&ec_ref, sb));
    }
}

// Go: internal/transformers/estransforms/classthis.go:isClassThisAssignmentBlock
// A class with a static block containing a non-assignment expression statement
// (e.g. a call expression) returns false.
#[test]
fn class_with_call_in_static_block_returns_false() {
    let (ec, source_file) = parse_shared("class C { static { foo(); } }");
    let ec_ref = ec.borrow();
    let static_block = find_first_kind(
        ec_ref.arena(),
        source_file,
        Kind::ClassStaticBlockDeclaration,
    );
    if let Some(sb) = static_block {
        assert!(!is_class_this_assignment_block(&ec_ref, sb));
    }
}

// Go: internal/transformers/estransforms/classthis.go:isClassThisAssignmentBlock
// A class with a static block containing `_classThis += this` (compound
// assignment, not plain `=`) returns false.
#[test]
fn class_with_compound_assignment_returns_false() {
    let (ec, source_file) = parse_shared("class C { static { _classThis += this; } }");
    let ec_ref = ec.borrow();
    let static_block = find_first_kind(
        ec_ref.arena(),
        source_file,
        Kind::ClassStaticBlockDeclaration,
    );
    if let Some(sb) = static_block {
        assert!(!is_class_this_assignment_block(&ec_ref, sb));
    }
}

// Go: internal/transformers/estransforms/classthis.go:isClassThisAssignmentBlock
// A class with `static { x = 42; }` — not assigning `this` on the RHS.
#[test]
fn class_with_non_this_rhs_returns_false() {
    let (ec, source_file) = parse_shared("class C { static { x = 42; } }");
    let ec_ref = ec.borrow();
    let static_block = find_first_kind(
        ec_ref.arena(),
        source_file,
        Kind::ClassStaticBlockDeclaration,
    );
    if let Some(sb) = static_block {
        assert!(!is_class_this_assignment_block(&ec_ref, sb));
    }
}
