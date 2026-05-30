use super::*;
use crate::utilities::GetLiteralTextFlags;
use tsgo_ast::NodeId;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

/// Parses `input` and returns `(arena, first_expression_statement_expression)`.
fn parse_first_expr(input: &str) -> (tsgo_ast::NodeArena, NodeId) {
    let parse = parse_source_file(
        SourceFileParseOptions {
            file_name: "/f.ts".to_string(),
        },
        input,
        ScriptKind::Ts,
    );
    let arena = parse.arena;
    let stmt = match arena.data(parse.source_file) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => unreachable!(),
    };
    let expr = match arena.data(stmt) {
        tsgo_ast::NodeData::ExpressionStatement(d) => d.expression,
        _ => unreachable!(),
    };
    (arena, expr)
}

// Go: internal/printer/utilities.go:getLiteralText (original-text path)
#[test]
fn uses_original_source_text_for_string() {
    let (arena, expr) = parse_first_expr("'hi'");
    assert_eq!(
        get_literal_text(&arena, "'hi'", expr, GetLiteralTextFlags::NONE),
        "'hi'"
    );
}

// Go: internal/printer/utilities.go:getLiteralText (canonical numeric path)
#[test]
fn uses_canonical_text_for_separated_numeric() {
    // `1_000` has a numeric separator and the default flags disallow it, so the
    // canonical (separator-free) node text is used.
    let (arena, expr) = parse_first_expr("1_000");
    assert_eq!(
        get_literal_text(&arena, "1_000", expr, GetLiteralTextFlags::NONE),
        "1000"
    );
}

// Go: internal/printer/utilities.go:getLiteralText (numeric original-text path)
#[test]
fn uses_original_text_for_plain_numeric() {
    let (arena, expr) = parse_first_expr("42");
    assert_eq!(
        get_literal_text(&arena, "42", expr, GetLiteralTextFlags::NONE),
        "42"
    );
}
