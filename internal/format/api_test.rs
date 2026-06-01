use super::*;
use crate::format_code_settings::{get_default_format_code_settings, FormatCodeSettings};
use tsgo_astnav::NavSourceFile;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_core::textchange::{apply_bulk_edits, TextChange};
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

/// Parses `text`, formats the whole document with default options, and returns
/// the resulting formatted string.
fn format_doc(text: &str) -> String {
    apply(text, format_doc_edits(text))
}

fn format_doc_edits(text: &str) -> Vec<TextChange> {
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    let nav = NavSourceFile::from_borrowed_arena(&r.arena, r.source_file, text.to_string());
    format_document(&nav, &get_default_format_code_settings())
}

fn apply(text: &str, mut edits: Vec<TextChange>) -> String {
    edits.sort_by(|a, b| (a.range.pos(), a.range.end()).cmp(&(b.range.pos(), b.range.end())));
    apply_bulk_edits(text, &edits)
}

// Slice 1: `[1,2,3]` -> `[1, 2, 3]` (space after comma).
// Go: internal/format/rules.go:SpaceAfterComma (driven through the span worker)
#[test]
fn format_document_array_inserts_space_after_comma() {
    assert_eq!(format_doc("[1,2,3]"), "[1, 2, 3]");
}

// Slice 2: `1+2` -> `1 + 2` (spaces around binary operator).
// Go: internal/format/rules.go:SpaceBeforeBinaryOperator / SpaceAfterBinaryOperator
#[test]
fn format_document_binary_operator_spaces() {
    assert_eq!(format_doc("1+2"), "1 + 2");
}

// Slice 3: `const x=1` -> `const x = 1` (spaces around `=` in a declaration).
// Go: internal/format/rules.go:SpaceBeforeBinaryOperator / SpaceAfterBinaryOperator
#[test]
fn format_document_const_assignment_spaces() {
    assert_eq!(format_doc("const x=1"), "const x = 1");
}

// Slice 4: a statement inside a block is indented one level (SmartIndenter).
// Go: internal/format/span.go (computeIndentation + dynamicIndenter) + indent.go
#[test]
fn format_document_indents_block_statement() {
    assert_eq!(
        format_doc("function f() {\nreturn 1;\n}"),
        "function f() {\n    return 1;\n}"
    );
}

// Slice 4b: a deeper combined case (spacing + indentation together).
#[test]
fn format_document_indent_and_spacing() {
    assert_eq!(
        format_doc("function f() {\nreturn 1+2;\n}"),
        "function f() {\n    return 1 + 2;\n}"
    );
}

// Slice 6: an already-formatted document produces ZERO edits (idempotence).
// Go: the rules engine yields no applicable action for correct pairs.
#[test]
fn format_document_already_formatted_is_idempotent() {
    assert!(format_doc_edits("[1, 2, 3]").is_empty());
    assert!(format_doc_edits("1 + 2").is_empty());
    assert!(format_doc_edits("const x = 1").is_empty());
    assert!(format_doc_edits("function f() {\n    return 1;\n}").is_empty());
}

// Idempotence (fixed point): formatting the formatted text again is a no-op.
#[test]
fn format_document_reaches_fixed_point() {
    for input in [
        "[1,2,3]",
        "1+2",
        "const x=1",
        "function f() {\nreturn 1;\n}",
    ] {
        let once = format_doc(input);
        let twice = format_doc(&once);
        assert_eq!(once, twice, "formatting should be idempotent for {input:?}");
    }
}

// Slice 5a: format-on-closing-curly indents the block body.
// Go: internal/format/api.go:FormatOnClosingCurly
#[test]
fn format_on_closing_curly_indents_block() {
    let text = "function f() {\nreturn 1;\n}";
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    let nav = NavSourceFile::from_borrowed_arena(&r.arena, r.source_file, text.to_string());
    let position = text.len() as i32; // just after the typed `}`
    let edits = format_on_closing_curly(&nav, position, &get_default_format_code_settings());
    assert_eq!(apply(text, edits), "function f() {\n    return 1;\n}");
}

// Slice 5b: format-on-semicolon formats the just-completed statement.
// Go: internal/format/api.go:FormatOnSemicolon
#[test]
fn format_on_semicolon_formats_statement() {
    let text = "const x=1;";
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    let nav = NavSourceFile::from_borrowed_arena(&r.arena, r.source_file, text.to_string());
    let position = text.len() as i32; // just after the typed `;`
    let edits = format_on_semicolon(&nav, position, &get_default_format_code_settings());
    assert_eq!(apply(text, edits), "const x = 1;");
}

// Option-driven: disabling comma spacing deletes the existing space instead.
// Go: internal/format/rules.go:NoSpaceAfterComma
#[test]
fn format_document_no_space_after_comma_when_disabled() {
    let text = "[1, 2, 3]";
    let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    let nav = NavSourceFile::from_borrowed_arena(&r.arena, r.source_file, text.to_string());
    let mut opts: FormatCodeSettings = get_default_format_code_settings();
    opts.insert_space_after_comma_delimiter = tsgo_core::tristate::Tristate::False;
    let edits = format_document(&nav, &opts);
    assert_eq!(apply(text, edits), "[1,2,3]");
}

// No-op on an empty file (no tokens to format).
#[test]
fn format_document_empty_file() {
    assert!(format_doc_edits("").is_empty());
}
