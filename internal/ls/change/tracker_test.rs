use std::rc::Rc;

use tsgo_ast::{NodeData, NodeId};
use tsgo_astnav::RcSourceFile;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_core::text::TextRange;
use tsgo_core::textchange::apply_bulk_edits;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

use super::*;

/// Parses `text` as a TS file and wraps it in a shared navigation context.
fn parse(text: &str) -> Rc<RcSourceFile> {
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    Rc::new(RcSourceFile::from_rc_arena(
        Rc::new(r.arena),
        r.source_file,
        text.to_string(),
    ))
}

/// Returns the top-level statement node ids.
fn statements(sf: &RcSourceFile) -> Vec<NodeId> {
    match sf.arena().data(sf.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => unreachable!("root is a SourceFile"),
    }
}

// Slice 1: replace a node's span with text -> one TextChange (span + text).
// Go: internal/ls/change/tracker.go:ReplaceRangeWithText
#[test]
fn replace_range_with_text_one_edit() {
    let text = "const x = 1;";
    let sf = parse(text);
    let mut t = ChangeTracker::new("\n");

    // The `x` identifier is at byte offset [6, 7).
    t.replace_range_with_text(&sf, TextRange::new(6, 7), "y");

    let changes = t.get_changes();
    let edits = &changes[""];
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].range, TextRange::new(6, 7));
    assert_eq!(edits[0].new_text, "y");
    assert_eq!(apply_bulk_edits(text, edits), "const y = 1;");
}

// Slice 2: insert_text records an empty-range edit at the offset.
// Go: internal/ls/change/tracker.go:InsertText
#[test]
fn insert_text_empty_range_edit() {
    let text = "ab";
    let sf = parse(text);
    let mut t = ChangeTracker::new("\n");

    t.insert_text(&sf, 1, "X");

    let changes = t.get_changes();
    let edits = &changes[""];
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].range, TextRange::new(1, 1));
    assert_eq!(edits[0].new_text, "X");
    assert_eq!(apply_bulk_edits(text, edits), "aXb");
}

// Slice 3: delete_range records an empty-text edit over the range.
// Go: internal/ls/change/tracker.go:DeleteRange
#[test]
fn delete_range_empty_text_edit() {
    let text = "abcdef";
    let sf = parse(text);
    let mut t = ChangeTracker::new("\n");

    t.delete_range(&sf, TextRange::new(2, 4));

    let changes = t.get_changes();
    let edits = &changes[""];
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].range, TextRange::new(2, 4));
    assert_eq!(edits[0].new_text, "");
    assert_eq!(apply_bulk_edits(text, edits), "abef");
}

// Slice 4: get_changes sorts edits and applies cleanly (non-overlapping).
// Go: internal/ls/change/trackerimpl.go:getTextChangesFromChanges
#[test]
fn get_changes_sorts_and_applies() {
    let text = "abcdefghij";
    let sf = parse(text);
    let mut t = ChangeTracker::new("\n");

    // Record out of order: a tail delete, a head replace, a middle insert.
    t.delete_range(&sf, TextRange::new(8, 10));
    t.replace_range_with_text(&sf, TextRange::new(0, 2), "Y");
    t.insert_text(&sf, 5, "X");

    let changes = t.get_changes();
    let edits = &changes[""];
    // Sorted ascending by (pos, end).
    assert_eq!(edits[0].range, TextRange::new(0, 2));
    assert_eq!(edits[1].range, TextRange::new(5, 5));
    assert_eq!(edits[2].range, TextRange::new(8, 10));
    assert_eq!(apply_bulk_edits(text, edits), "YcdeXfgh");
}

// Slice 4: get_changes panics when two edits in a file overlap.
// Go: internal/ls/change/trackerimpl.go:getTextChangesFromChanges (overlap assert)
#[test]
#[should_panic(expected = "changes overlap")]
fn get_changes_panics_on_overlap() {
    let text = "abcdefghij";
    let sf = parse(text);
    let mut t = ChangeTracker::new("\n");

    t.replace_range_with_text(&sf, TextRange::new(0, 5), "Y");
    t.replace_range_with_text(&sf, TextRange::new(3, 8), "Z");

    t.get_changes();
}

// Touching ranges (end == start) are allowed (not an overlap).
#[test]
fn get_changes_allows_touching_ranges() {
    let text = "abcdef";
    let sf = parse(text);
    let mut t = ChangeTracker::new("\n");

    t.replace_range_with_text(&sf, TextRange::new(0, 3), "X");
    t.replace_range_with_text(&sf, TextRange::new(3, 6), "Y");

    let changes = t.get_changes();
    assert_eq!(apply_bulk_edits(text, &changes[""]), "XY");
}

// Empty trackers produce no file entries.
#[test]
fn get_changes_empty_is_empty() {
    let mut t = ChangeTracker::new("\n");
    assert!(t.get_changes().is_empty());
}

// new_line accessor round-trips.
#[test]
fn new_line_accessor() {
    let t = ChangeTracker::new("\r\n");
    assert_eq!(t.new_line(), "\r\n");
}

// Slice 7: public delete_node of a statement removes the node + trailing line.
// Go: internal/ls/change/tracker.go:DeleteNode
#[test]
fn delete_node_statement_with_trailing_newline() {
    let text = "const a = 1;\nconst b = 2;\nconst c = 3;\n";
    let sf = parse(text);
    let stmts = statements(&sf);
    let mut t = ChangeTracker::new("\n");

    t.delete_node(
        &sf,
        stmts[1],
        LeadingTriviaOption::StartLine,
        TrailingTriviaOption::Include,
    );

    let changes = t.get_changes();
    assert_eq!(
        apply_bulk_edits(text, &changes[""]),
        "const a = 1;\nconst c = 3;\n"
    );
}

// Edits for two different files are keyed separately.
#[test]
fn edits_keyed_per_file() {
    let text_a = "let a = 1;";
    let text_b = "let b = 2;";
    let ra = parse_source_file(
        SourceFileParseOptions {
            file_name: "a.ts".to_string(),
        },
        text_a,
        ScriptKind::Ts,
    );
    let rb = parse_source_file(
        SourceFileParseOptions {
            file_name: "b.ts".to_string(),
        },
        text_b,
        ScriptKind::Ts,
    );
    let sf_a = RcSourceFile::from_rc_arena(Rc::new(ra.arena), ra.source_file, text_a.to_string());
    let sf_b = RcSourceFile::from_rc_arena(Rc::new(rb.arena), rb.source_file, text_b.to_string());

    let mut t = ChangeTracker::new("\n");
    t.replace_range_with_text(&sf_a, TextRange::new(4, 5), "x");
    t.replace_range_with_text(&sf_b, TextRange::new(4, 5), "y");

    let changes = t.get_changes();
    assert_eq!(changes["a.ts"][0].new_text, "x");
    assert_eq!(changes["b.ts"][0].new_text, "y");
}
