use std::rc::Rc;

use tsgo_ast::{NodeData, NodeId};
use tsgo_astnav::RcSourceFile;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_core::textchange::apply_bulk_edits;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

use crate::tracker::ChangeTracker;

fn parse(text: &str) -> Rc<RcSourceFile> {
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    Rc::new(RcSourceFile::from_rc_arena(
        Rc::new(r.arena),
        r.source_file,
        text.to_string(),
    ))
}

fn statements(sf: &RcSourceFile) -> Vec<NodeId> {
    match sf.arena().data(sf.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => unreachable!("root is a SourceFile"),
    }
}

/// Returns the first `VariableDeclaration` inside a `VariableStatement` node.
fn first_var_decl(sf: &RcSourceFile, var_statement: NodeId) -> NodeId {
    let list = match sf.arena().data(var_statement) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => unreachable!("expected a VariableStatement"),
    };
    match sf.arena().data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
        _ => unreachable!("expected a VariableDeclarationList"),
    }
}

// Slice 8: smart delete of a sole VariableDeclaration removes the whole
// statement (+ trailing line), via deleteVariableDeclaration -> VariableStatement.
// Go: internal/ls/change/delete.go:deleteVariableDeclaration (VariableStatement)
#[test]
fn delete_variable_declaration_removes_statement() {
    let text = "const a = 1;\nconst b = 2;\nconst c = 3;\n";
    let sf = parse(text);
    let stmt1 = statements(&sf)[1];
    let decl = first_var_decl(&sf, stmt1);

    let mut t = ChangeTracker::new("\n");
    t.delete(&sf, decl);

    let changes = t.get_changes();
    assert_eq!(changes[""].len(), 1);
    assert_eq!(
        apply_bulk_edits(text, &changes[""]),
        "const a = 1;\nconst c = 3;\n"
    );
}

// Slice 8 (the task's literal single-line example): delete `b` from
// `const a=1; const b=2; const c=3;` removes the node + its trailing space.
// Go: internal/ls/change/delete.go:deleteVariableDeclaration (VariableStatement)
#[test]
fn delete_variable_declaration_single_line() {
    let text = "const a = 1; const b = 2; const c = 3;";
    let sf = parse(text);
    let stmt1 = statements(&sf)[1];
    let decl = first_var_decl(&sf, stmt1);

    let mut t = ChangeTracker::new("\n");
    t.delete(&sf, decl);

    let changes = t.get_changes();
    assert_eq!(
        apply_bulk_edits(text, &changes[""]),
        "const a = 1; const c = 3;"
    );
}

// Slice 8: smart delete of a whole VariableStatement (default arm) is equivalent.
// Go: internal/ls/change/delete.go:deleteDeclaration (default)
#[test]
fn delete_whole_variable_statement() {
    let text = "const a = 1;\nconst b = 2;\nconst c = 3;\n";
    let sf = parse(text);
    let stmt1 = statements(&sf)[1];

    let mut t = ChangeTracker::new("\n");
    t.delete(&sf, stmt1);

    let changes = t.get_changes();
    assert_eq!(
        apply_bulk_edits(text, &changes[""]),
        "const a = 1;\nconst c = 3;\n"
    );
}

// Deleting the last statement consumes the final trailing newline.
// Go: internal/ls/change/delete.go:deleteVariableDeclaration (VariableStatement)
#[test]
fn delete_last_statement() {
    let text = "const a = 1;\nconst b = 2;\nconst c = 3;\n";
    let sf = parse(text);
    let stmt2 = statements(&sf)[2];
    let decl = first_var_decl(&sf, stmt2);

    let mut t = ChangeTracker::new("\n");
    t.delete(&sf, decl);

    let changes = t.get_changes();
    assert_eq!(
        apply_bulk_edits(text, &changes[""]),
        "const a = 1;\nconst b = 2;\n"
    );
}

// A node contained within another deleted node is skipped (only the outer edit).
// Go: internal/ls/change/tracker.go:finishDeleteDeclarations (isContained)
#[test]
fn contained_node_is_skipped() {
    let text = "const a = 1;\nconst b = 2;\n";
    let sf = parse(text);
    let stmt1 = statements(&sf)[1];
    let decl = first_var_decl(&sf, stmt1);

    let mut t = ChangeTracker::new("\n");
    // Queue both the statement and a node strictly inside it.
    t.delete(&sf, stmt1);
    t.delete(&sf, decl);

    let changes = t.get_changes();
    // The inner declaration is contained in the statement, so only one edit runs.
    assert_eq!(changes[""].len(), 1);
    assert_eq!(apply_bulk_edits(text, &changes[""]), "const a = 1;\n");
}

// Smart delete keeps a preceding comment when the statement is on its own line
// (StartLine leading trivia), unlike a raw IncludeAll deletion.
// Go: internal/ls/change/delete.go:deleteVariableDeclaration (VariableStatement)
#[test]
fn delete_variable_declaration_preserves_leading_comment() {
    let text = "const a = 1;\n// keep me\nconst b = 2;\n";
    let sf = parse(text);
    let stmt1 = statements(&sf)[1];
    let decl = first_var_decl(&sf, stmt1);

    let mut t = ChangeTracker::new("\n");
    t.delete(&sf, decl);

    let changes = t.get_changes();
    assert_eq!(
        apply_bulk_edits(text, &changes[""]),
        "const a = 1;\n// keep me\n"
    );
}

// start_position_to_delete_node_in_list finds the first non-whitespace offset
// in the node's leading trivia.
// Go: internal/ls/change/delete.go:startPositionToDeleteNodeInList
#[test]
fn start_position_skips_leading_whitespace() {
    let text = "  x;";
    let sf = parse(text);
    let stmt0 = statements(&sf)[0];
    let t = ChangeTracker::new("\n");
    assert_eq!(t.start_position_to_delete_node_in_list(&sf, stmt0), 2);
}
