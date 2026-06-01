use super::*;
use tsgo_ast::NodeArena;
use tsgo_astnav::NavSourceFile;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_core::text::TextRange;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

use crate::format_code_settings::get_default_format_code_settings;

fn collect(arena: &NodeArena, n: NodeId, out: &mut Vec<NodeId>) {
    out.push(n);
    arena.for_each_child(n, &mut |c| {
        collect(arena, c, out);
        false
    });
}

fn first_of_kind(arena: &NodeArena, root: NodeId, kind: Kind) -> NodeId {
    let mut all = Vec::new();
    collect(arena, root, &mut all);
    *all.iter()
        .find(|&&n| arena.kind(n) == kind)
        .expect("kind present")
}

// Go: internal/format/indent.go:GetIndentationForNode (SourceFile -> base indent 0)
#[test]
fn source_file_indentation_is_zero() {
    let text = "function f() {\nreturn 1;\n}";
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    let nav = NavSourceFile::from_borrowed_arena(&r.arena, r.source_file, text.to_string());
    let ls = tsgo_core::compute_ecma_line_starts(text);
    let opts = get_default_format_code_settings();
    let span = TextRange::new(0, nav.end(nav.root()));
    assert_eq!(
        get_indentation_for_node(&nav, &ls, nav.root(), Some(span), &opts),
        0
    );
}

// Go: internal/format/indent.go:NodeWillIndentChild (Block indents its statements)
#[test]
fn block_will_indent_statement() {
    let text = "function f() {\nreturn 1;\n}";
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    let nav = NavSourceFile::from_borrowed_arena(&r.arena, r.source_file, text.to_string());
    let ls = tsgo_core::compute_ecma_line_starts(text);
    let opts = get_default_format_code_settings();

    let block = first_of_kind(&r.arena, r.source_file, Kind::Block);
    let ret = first_of_kind(&r.arena, r.source_file, Kind::ReturnStatement);
    assert!(node_will_indent_child(
        &opts,
        &nav,
        &ls,
        true,
        block,
        Some(ret),
        false
    ));
    assert!(should_indent_child_node(
        &opts,
        &nav,
        &ls,
        true,
        block,
        Some(ret),
        false
    ));
}

// Go: internal/format/indent.go:NodeWillIndentChild (a function does NOT indent its block child)
#[test]
fn function_declaration_does_not_indent_block_child() {
    let text = "function f() {\nreturn 1;\n}";
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    let nav = NavSourceFile::from_borrowed_arena(&r.arena, r.source_file, text.to_string());
    let ls = tsgo_core::compute_ecma_line_starts(text);
    let opts = get_default_format_code_settings();

    let func = first_of_kind(&r.arena, r.source_file, Kind::FunctionDeclaration);
    let block = first_of_kind(&r.arena, r.source_file, Kind::Block);
    assert!(!node_will_indent_child(
        &opts,
        &nav,
        &ls,
        true,
        func,
        Some(block),
        false
    ));
}

// Go: internal/format/indent.go:findFirstNonWhitespaceCharacterAndColumn
#[test]
fn first_non_whitespace_column_counts_spaces() {
    let opts = get_default_format_code_settings();
    let (character, column) = find_first_non_whitespace_character_and_column(0, 4, "    x", &opts);
    assert_eq!(character, 4);
    assert_eq!(column, 4);
    assert_eq!(find_first_non_whitespace_column(0, 2, "  x", &opts), 2);
}
