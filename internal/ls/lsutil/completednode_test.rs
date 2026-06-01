use super::*;
use crate::children::SourceFile;
use tsgo_ast::{Kind, NodeArena, NodeId};
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

/// Parses `text` as TypeScript into an owned navigation [`SourceFile`].
fn make(text: &str) -> SourceFile {
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    SourceFile::new(r.arena, r.source_file, text.to_string())
}

/// Depth-first search for the first node of `kind`.
fn find_first_of_kind(arena: &NodeArena, root: NodeId, kind: Kind) -> Option<NodeId> {
    if arena.kind(root) == kind {
        return Some(root);
    }
    let mut found = None;
    arena.for_each_child(root, &mut |c| {
        if let Some(f) = find_first_of_kind(arena, c, kind) {
            found = Some(f);
            true
        } else {
            false
        }
    });
    found
}

fn first_statement(file: &SourceFile) -> NodeId {
    match file.arena().data(file.root()) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => unreachable!(),
    }
}

// Go: internal/ls/lsutil/completednode.go:IsCompletedNode (Block / nodeEndsWith CloseBrace)
#[test]
fn is_completed_node_true_for_closed_block() {
    let file = make("function f() {}");
    let block = find_first_of_kind(file.arena(), file.root(), Kind::Block).expect("a block");
    assert!(is_completed_node(&file, block));
}

// Go: internal/ls/lsutil/completednode.go:IsCompletedNode (unclosed Block)
#[test]
fn is_completed_node_false_for_unclosed_block() {
    let file = make("function f() {");
    let block = find_first_of_kind(file.arena(), file.root(), Kind::Block).expect("a block");
    assert!(!is_completed_node(&file, block));
}

// Go: internal/ls/lsutil/completednode.go:IsCompletedNode (function body recursion)
#[test]
fn is_completed_node_follows_function_body() {
    let complete = make("function f() {}");
    let func = first_statement(&complete);
    assert!(is_completed_node(&complete, func));

    let incomplete = make("function f() {");
    let func = first_statement(&incomplete);
    assert!(!is_completed_node(&incomplete, func));
}

// Go: internal/ls/lsutil/completednode.go:IsCompletedNode (CallExpression / nodeEndsWith CloseParen)
#[test]
fn is_completed_node_call_expression_close_paren() {
    let complete = make("f()");
    let call = find_first_of_kind(complete.arena(), complete.root(), Kind::CallExpression)
        .expect("a call");
    assert!(is_completed_node(&complete, call));

    let incomplete = make("f(");
    let call = find_first_of_kind(incomplete.arena(), incomplete.root(), Kind::CallExpression)
        .expect("a call");
    assert!(!is_completed_node(&incomplete, call));
}

// Go: internal/ls/lsutil/completednode.go:IsCompletedNode (ExpressionStatement)
#[test]
fn is_completed_node_expression_statement() {
    // `a;` is completed (its expression is a complete identifier).
    let file = make("a;");
    let stmt = first_statement(&file);
    assert!(is_completed_node(&file, stmt));
}

// Go: internal/ls/lsutil/completednode.go:IsCompletedNode (CaseClause is never completed)
#[test]
fn is_completed_node_case_clause_never_completed() {
    let file = make("switch (x) { case 1: break; }");
    let case = find_first_of_kind(file.arena(), file.root(), Kind::CaseClause).expect("a case");
    assert!(!is_completed_node(&file, case));
}

// Go: internal/ls/lsutil/completednode.go:nodeEndsWith
#[test]
fn node_ends_with_close_brace() {
    let file = make("function f() {}");
    let block = find_first_of_kind(file.arena(), file.root(), Kind::Block).expect("a block");
    assert!(node_ends_with(&file, block, Kind::CloseBraceToken));
    assert!(!node_ends_with(&file, block, Kind::CloseParenToken));
}

// Go: internal/ls/lsutil/completednode.go:hasChildOfKind
#[test]
fn has_child_of_kind_finds_paren_not_class_keyword() {
    let file = make("function f() {}");
    let func = first_statement(&file);
    assert!(has_child_of_kind(&file, func, Kind::OpenParenToken));
    assert!(has_child_of_kind(&file, func, Kind::CloseParenToken));
    assert!(!has_child_of_kind(&file, func, Kind::ClassKeyword));
}

// Go: internal/ls/lsutil/completednode.go:PositionBelongsToNode
#[test]
fn position_belongs_to_node_completed_vs_open() {
    // A completed block: a position strictly inside belongs; the end position
    // does not (the node is completed and the cursor is past it).
    let file = make("function f() {}");
    let block = find_first_of_kind(file.arena(), file.root(), Kind::Block).expect("a block");
    let pos = file.arena().loc(block).pos();
    let end = file.arena().loc(block).end();
    assert!(position_belongs_to_node(&file, block, pos));
    assert!(!position_belongs_to_node(&file, block, end));

    // An unclosed block is never completed, so even its end position belongs.
    let open = make("function f() {");
    let block = find_first_of_kind(open.arena(), open.root(), Kind::Block).expect("a block");
    let end = open.arena().loc(block).end();
    assert!(position_belongs_to_node(&open, block, end));
}

// Go: internal/ls/lsutil/completednode.go:PositionBelongsToNode (precondition)
#[test]
#[should_panic(expected = "Expected candidate.pos <= position")]
fn position_belongs_to_node_panics_when_pos_after_position() {
    let file = make("function f() {}");
    let block = find_first_of_kind(file.arena(), file.root(), Kind::Block).expect("a block");
    let pos = file.arena().loc(block).pos();
    position_belongs_to_node(&file, block, pos - 1);
}
