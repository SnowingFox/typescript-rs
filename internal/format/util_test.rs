use super::*;
use tsgo_astnav::NavSourceFile;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

fn parse(text: &str) -> (tsgo_ast::NodeArena, NodeId, String) {
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    (r.arena, r.source_file, text.to_string())
}

// Go: internal/format/util.go:rangeIsOnOneLine
#[test]
fn range_is_on_one_line_detects_newlines() {
    let text = "ab\ncd";
    let ls = tsgo_core::compute_ecma_line_starts(text);
    assert!(range_is_on_one_line(&ls, TextRange::new(0, 2)));
    assert!(!range_is_on_one_line(&ls, TextRange::new(0, 4)));
}

// Go: internal/format/context.go:withTokenStart
#[test]
fn with_token_start_skips_leading_trivia() {
    let text = "  x;";
    let (arena, root, t) = parse(text);
    let nav = NavSourceFile::from_borrowed_arena(&arena, root, t);
    // The source file's token start skips the two leading spaces.
    let r = with_token_start(&nav, nav.root());
    assert_eq!(r.pos(), 2);
}

// Go: internal/format/util.go:GetLineStartPositionForPosition
#[test]
fn line_start_position() {
    let text = "ab\ncd";
    let ls = tsgo_core::compute_ecma_line_starts(text);
    assert_eq!(get_line_start_position_for_position(&ls, 0), 0);
    assert_eq!(get_line_start_position_for_position(&ls, 4), 3);
}

// Go: internal/format/util.go:findImmediatelyPrecedingTokenOfKind
#[test]
fn finds_immediately_preceding_open_brace() {
    let text = "function f() {\nreturn 1;\n}";
    let (arena, root, t) = parse(text);
    let nav = NavSourceFile::from_borrowed_arena(&arena, root, t);
    // `{` ends at byte 14.
    let brace = find_immediately_preceding_token_of_kind(&nav, 14, tsgo_ast::Kind::OpenBraceToken);
    assert!(brace.is_some());
    assert_eq!(nav.kind(brace.unwrap()), tsgo_ast::Kind::OpenBraceToken);
    // Wrong kind requested -> none.
    assert!(
        find_immediately_preceding_token_of_kind(&nav, 14, tsgo_ast::Kind::SemicolonToken)
            .is_none()
    );
}

// Go: internal/format/util.go:findOutermostNodeWithinListLevel + isListElement
#[test]
fn outermost_node_within_list_level_is_function_declaration() {
    let text = "function f() {\nreturn 1;\n}";
    let (arena, root, t) = parse(text);
    let nav = NavSourceFile::from_borrowed_arena(&arena, root, t);
    let close_brace = nav.find_preceding_token(text.len() as i32).unwrap();
    assert_eq!(nav.kind(close_brace), tsgo_ast::Kind::CloseBraceToken);
    let outermost = find_outermost_node_within_list_level(&nav, close_brace);
    assert_eq!(nav.kind(outermost), tsgo_ast::Kind::FunctionDeclaration);
}

// Go: internal/format/util.go:isListElement (statement list containers)
#[test]
fn statement_is_list_element_of_source_file() {
    let text = "const x = 1;";
    let (arena, root, t) = parse(text);
    let nav = NavSourceFile::from_borrowed_arena(&arena, root, t);
    let stmt = match nav.arena().data(nav.root()) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => unreachable!(),
    };
    assert!(is_list_element(&nav, nav.root(), stmt));
}

// Go: internal/format/util.go:isGrammarError (reachable kinds report false)
#[test]
fn grammar_error_false_for_reachable_kinds() {
    let text = "const x = 1;";
    let (arena, root, t) = parse(text);
    let nav = NavSourceFile::from_borrowed_arena(&arena, root, t);
    let stmt = match nav.arena().data(nav.root()) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => unreachable!(),
    };
    assert!(!is_grammar_error(&nav, nav.root(), stmt));
}
