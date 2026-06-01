use std::rc::Rc;

use tsgo_ast::{NodeData, NodeId};
use tsgo_astnav::RcSourceFile;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_core::text::TextRange;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

use super::*;
use crate::tracker::{ChangeTracker, LeadingTriviaOption, TrailingTriviaOption};

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

// Slice 6: Exclude returns the token start (after leading whitespace).
// Go: internal/ls/change/trackerimpl.go:getAdjustedStartPosition (Exclude)
#[test]
fn adjusted_start_exclude_is_token_start() {
    let text = "  const x = 1;";
    let sf = parse(text);
    let stmt = statements(&sf)[0];
    let t = ChangeTracker::new("\n");
    assert_eq!(
        t.get_adjusted_start_position(&sf, stmt, LeadingTriviaOption::Exclude, false),
        2
    );
}

// Slice 6: StartLine returns the start of the node's own line.
// Go: internal/ls/change/trackerimpl.go:getAdjustedStartPosition (StartLine)
#[test]
fn adjusted_start_start_line() {
    let text = "const a = 1;\nconst b = 2;\nconst c = 3;\n";
    let sf = parse(text);
    let stmt1 = statements(&sf)[1];
    let t = ChangeTracker::new("\n");
    assert_eq!(
        t.get_adjusted_start_position(&sf, stmt1, LeadingTriviaOption::StartLine, false),
        13
    );
}

// Slice 6: IncludeAll on a node whose leading trivia is on a previous line
// pulls the start back to the leading comment's line.
// Go: internal/ls/change/trackerimpl.go:getAdjustedStartPosition (IncludeAll)
#[test]
fn adjusted_start_include_all_pulls_to_comment_line() {
    let text = "const a = 1;\n// hello\nconst b = 2;\n";
    let sf = parse(text);
    let stmt1 = statements(&sf)[1];
    let t = ChangeTracker::new("\n");
    // IncludeAll reaches up to the start of the comment line (13).
    assert_eq!(
        t.get_adjusted_start_position(&sf, stmt1, LeadingTriviaOption::IncludeAll, false),
        13
    );
    // StartLine keeps the comment, starting at the statement's own line (22).
    assert_eq!(
        t.get_adjusted_start_position(&sf, stmt1, LeadingTriviaOption::StartLine, false),
        22
    );
}

// Exclude end is the node end (no trivia).
// Go: internal/ls/change/trackerimpl.go:getAdjustedEndPosition (Exclude)
#[test]
fn adjusted_end_exclude_is_node_end() {
    let text = "const a = 1;\nconst b = 2;\n";
    let sf = parse(text);
    let stmt0 = statements(&sf)[0];
    let t = ChangeTracker::new("\n");
    assert_eq!(
        t.get_adjusted_end_position(&sf, stmt0, TrailingTriviaOption::Exclude),
        12
    );
}

// Slice 5: Include end consumes the trailing line break.
// Go: internal/ls/change/trackerimpl.go:getAdjustedEndPosition (Include)
#[test]
fn adjusted_end_include_consumes_newline() {
    let text = "const a = 1;\nconst b = 2;\n";
    let sf = parse(text);
    let stmt0 = statements(&sf)[0];
    let t = ChangeTracker::new("\n");
    assert_eq!(
        t.get_adjusted_end_position(&sf, stmt0, TrailingTriviaOption::Include),
        13
    );
}

// ExcludeWhitespace end includes a trailing comment but stops before the newline.
// Go: internal/ls/change/trackerimpl.go:getAdjustedEndPosition (ExcludeWhitespace)
#[test]
fn adjusted_end_exclude_whitespace_keeps_comment() {
    let text = "x; // hi\n";
    let sf = parse(text);
    let stmt0 = statements(&sf)[0];
    let t = ChangeTracker::new("\n");
    // Comment `// hi` spans [3, 8); the newline at 8 is excluded.
    assert_eq!(
        t.get_adjusted_end_position(&sf, stmt0, TrailingTriviaOption::ExcludeWhitespace),
        8
    );
}

// Include end follows a multiline trailing comment onto the next line.
// Go: internal/ls/change/trackerimpl.go:getEndPositionOfMultilineTrailingComment
#[test]
fn adjusted_end_include_multiline_trailing_comment() {
    let text = "x; /* a\nb */\nrest";
    let sf = parse(text);
    let stmt0 = statements(&sf)[0];
    let t = ChangeTracker::new("\n");
    // The block comment ends at 12 (the newline); skip past it to line 2 start 13.
    assert_eq!(
        t.get_adjusted_end_position(&sf, stmt0, TrailingTriviaOption::Include),
        13
    );
}

// get_adjusted_range combines the start and end adjustments.
// Go: internal/ls/change/trackerimpl.go:GetAdjustedRange
#[test]
fn adjusted_range_combines() {
    let text = "const a = 1;\nconst b = 2;\nconst c = 3;\n";
    let sf = parse(text);
    let stmt1 = statements(&sf)[1];
    let t = ChangeTracker::new("\n");
    assert_eq!(
        t.get_adjusted_range(
            &sf,
            stmt1,
            stmt1,
            LeadingTriviaOption::StartLine,
            TrailingTriviaOption::Include,
        ),
        TextRange::new(13, 26)
    );
}

// positions_are_on_same_line compares line starts.
// Go: internal/ls/change/delete.go:positionsAreOnSameLine
#[test]
fn positions_on_same_line() {
    let text = "ab\ncd";
    let sf = parse(text);
    assert!(positions_are_on_same_line(&sf, 0, 1));
    assert!(!positions_are_on_same_line(&sf, 1, 4));
}

// get_text_changes_from_changes maps every accumulated edit to a TextChange.
// Go: internal/ls/change/trackerimpl.go:getTextChangesFromChanges
#[test]
fn text_changes_from_changes_maps_all() {
    let text = "abcdef";
    let sf = parse(text);
    let mut t = ChangeTracker::new("\n");
    t.replace_range_with_text(&sf, TextRange::new(0, 1), "A");
    let out = t.get_text_changes_from_changes();
    assert_eq!(out[""].len(), 1);
    assert_eq!(out[""][0].new_text, "A");
}
