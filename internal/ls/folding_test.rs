use tsgo_lsproto::{FoldingRange, FoldingRangeKind};

use crate::test_support::build_service;

/// Builds a folding range with both characters present (the common shape).
fn fold(start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> FoldingRange {
    FoldingRange {
        start_line,
        start_character: Some(start_char),
        end_line,
        end_character: Some(end_char),
        kind: None,
        collapsed_text: None,
    }
}

/// Builds a `comment`-kind folding range.
fn fold_comment(start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> FoldingRange {
    FoldingRange {
        start_line,
        start_character: Some(start_char),
        end_line,
        end_character: Some(end_char),
        kind: Some(FoldingRangeKind::comment()),
        collapsed_text: None,
    }
}

// Go: internal/ls/folding.go:getOutliningSpanForNode (Block -> functionSpan) —
// a function body block folds from the open brace's full start to the close
// brace's end.
#[test]
fn provide_folding_ranges_function_block() {
    let ls = build_service(
        &[("/m.ts", "function f() {\n  return 1;\n}")],
        "/",
        &["/m.ts"],
    );
    let ranges = ls.provide_folding_ranges("/m.ts");
    // `{` full start is right after `)` (byte 12); `}` ends at byte 28 (line 2,
    // char 1).
    assert_eq!(ranges, vec![fold(0, 12, 2, 1)]);
}

// Go: internal/ls/folding.go:getOutliningSpanForNode (ObjectLiteralExpression)
// — an object literal initializer folds from the open brace's full start to the
// close brace's end.
#[test]
fn provide_folding_ranges_object_literal() {
    let ls = build_service(
        &[("/m.ts", "const o = {\n a: 1,\n b: 2\n};")],
        "/",
        &["/m.ts"],
    );
    let ranges = ls.provide_folding_ranges("/m.ts");
    // `{` full start is the space after `=` (byte 9); `}` ends at byte 26
    // (line 3, char 1).
    assert_eq!(ranges, vec![fold(0, 9, 3, 1)]);
}

// Go: internal/ls/folding.go:getOutliningSpanForNode (ArrayLiteralExpression)
// — an array literal initializer folds from the open bracket's full start to
// the close bracket's end.
#[test]
fn provide_folding_ranges_array_literal() {
    let ls = build_service(&[("/m.ts", "const a = [\n 1,\n 2\n];")], "/", &["/m.ts"]);
    let ranges = ls.provide_folding_ranges("/m.ts");
    assert_eq!(ranges, vec![fold(0, 9, 3, 1)]);
}

// Go: internal/ls/folding.go:getOutliningSpanForNode (ClassDeclaration + nested
// method body) — the class body folds, and the method body block folds; the
// result is sorted by (start_line, start_character).
#[test]
fn provide_folding_ranges_class_with_method() {
    let ls = build_service(&[("/m.ts", "class C {\n  m() {}\n}")], "/", &["/m.ts"]);
    let ranges = ls.provide_folding_ranges("/m.ts");
    assert_eq!(ranges, vec![fold(0, 7, 2, 1), fold(1, 5, 1, 8)]);
}

// Go: internal/ls/folding.go:addOutliningForLeadingCommentsForPos
// (MultiLineCommentTrivia) — a multi-line comment (here the EOF's leading
// trivia) folds as a `comment`-kind range.
#[test]
fn provide_folding_ranges_multi_line_comment() {
    let ls = build_service(&[("/m.ts", "/* a\n b */")], "/", &["/m.ts"]);
    let ranges = ls.provide_folding_ranges("/m.ts");
    assert_eq!(ranges, vec![fold_comment(0, 0, 1, 5)]);
}

// Go: internal/ls/folding.go:combineAndAddMultipleSingleLineComments — two or
// more consecutive single-line comments combine into one `comment` fold.
#[test]
fn provide_folding_ranges_consecutive_single_line_comments() {
    let ls = build_service(&[("/m.ts", "// a\n// b\nfunction f(){}")], "/", &["/m.ts"]);
    let ranges = ls.provide_folding_ranges("/m.ts");
    // The two `//` comments combine into `(0,0)-(1,4)`; the function body folds
    // at `(2,12)-(2,14)`.
    assert_eq!(ranges, vec![fold_comment(0, 0, 1, 4), fold(2, 12, 2, 14)]);
}

// A single `//` comment is not folded (Go only folds runs of two or more).
// Go: internal/ls/folding.go:combineAndAddMultipleSingleLineComments
#[test]
fn provide_folding_ranges_single_comment_not_folded() {
    let ls = build_service(&[("/m.ts", "// just one\nfunction f(){}")], "/", &["/m.ts"]);
    let ranges = ls.provide_folding_ranges("/m.ts");
    assert_eq!(ranges, vec![fold(1, 12, 1, 14)]);
}

// An empty file yields no folding ranges (no panic).
// Go: internal/ls/folding.go:addNodeOutliningSpans (empty statements)
#[test]
fn provide_folding_ranges_empty_file_is_empty() {
    let ls = build_service(&[("/m.ts", "")], "/", &["/m.ts"]);
    assert!(ls.provide_folding_ranges("/m.ts").is_empty());
}

// An unknown file yields no folding ranges (no panic).
// Go: internal/ls/languageservice.go:getProgramAndFile (missing file)
#[test]
fn provide_folding_ranges_unknown_file_is_empty() {
    let ls = build_service(&[("/m.ts", "function f(){}")], "/", &["/m.ts"]);
    assert!(ls.provide_folding_ranges("/missing.ts").is_empty());
}
