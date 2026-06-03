use tsgo_lsproto::{Position, Range, SelectionRange};

use crate::test_support::build_service;

/// Builds an LSP [`Position`].
fn pos(line: u32, character: u32) -> Position {
    Position { line, character }
}

/// Builds an LSP [`Range`] from `(start_line, start_char)` to `(end_line, end_char)`.
fn rng(sl: u32, sc: u32, el: u32, ec: u32) -> Range {
    Range {
        start: pos(sl, sc),
        end: pos(el, ec),
    }
}

/// Builds a [`SelectionRange`] with an optional parent.
fn sel(range: Range, parent: Option<SelectionRange>) -> SelectionRange {
    SelectionRange {
        range,
        parent: parent.map(Box::new),
    }
}

// Go: internal/ls/selectionranges.go:getSmartSelectionRange — a position inside
// an identifier in a nested expression expands outward: identifier -> enclosing
// binary expression -> return statement -> (multi-line) function body block ->
// source file.
#[test]
fn provide_selection_ranges_nested_identifier_chain() {
    let ls = build_service(
        &[("/m.ts", "function f() {\n  return a + b;\n}")],
        "/",
        &["/m.ts"],
    );
    // Position on the `a` identifier (line 1, char 9).
    let ranges = ls.provide_selection_ranges("/m.ts", &[pos(1, 9)]);

    let file = sel(rng(0, 0, 2, 1), None);
    let block = sel(rng(0, 13, 2, 1), Some(file));
    let ret = sel(rng(1, 2, 1, 15), Some(block));
    let expr = sel(rng(1, 9, 1, 14), Some(ret));
    let ident = sel(rng(1, 9, 1, 10), Some(expr));

    assert_eq!(ranges, vec![ident]);
}

// Go: internal/ls/selectionranges.go:getSmartSelectionRange — a string literal
// gets a stop both inside its quotes (the content) and around the whole literal
// (`if ast.IsStringLiteral(node) { ... start+1, end-1 }`). A lone `const`
// declaration's variable-statement/list/declaration stops are deduped/skipped,
// so the chain is: inner content -> whole literal -> source file.
#[test]
fn provide_selection_ranges_string_literal_inner_and_outer() {
    let ls = build_service(&[("/m.ts", "const s = \"abc\";")], "/", &["/m.ts"]);
    // Position on the `b` inside `"abc"` (byte 12, line 0 char 12).
    let ranges = ls.provide_selection_ranges("/m.ts", &[pos(0, 12)]);

    let file = sel(rng(0, 0, 0, 16), None);
    let literal = sel(rng(0, 10, 0, 15), Some(file));
    let inner = sel(rng(0, 11, 0, 14), Some(literal));

    assert_eq!(ranges, vec![inner]);
}

// Guard: an empty file yields the (empty) full-file range with no parent and no
// panic (Go always returns at least the source-file range).
// Go: internal/ls/selectionranges.go:getSmartSelectionRange (initial fullRange)
#[test]
fn provide_selection_ranges_empty_file_is_full_range() {
    let ls = build_service(&[("/m.ts", "")], "/", &["/m.ts"]);
    let ranges = ls.provide_selection_ranges("/m.ts", &[pos(0, 0)]);
    assert_eq!(ranges, vec![sel(rng(0, 0, 0, 0), None)]);
}

// Guard: a position at the file's end boundary lands on no inner node and yields
// just the full-file range (no panic).
// Go: internal/ls/selectionranges.go:getSmartSelectionRange (nodeContainsPosition)
#[test]
fn provide_selection_ranges_boundary_position_is_full_range() {
    let ls = build_service(&[("/m.ts", "x;")], "/", &["/m.ts"]);
    // Byte offset 2 == end of file.
    let ranges = ls.provide_selection_ranges("/m.ts", &[pos(0, 2)]);
    assert_eq!(ranges, vec![sel(rng(0, 0, 0, 2), None)]);
}

// Guard: an unknown file yields no selection ranges (no panic).
// Go: internal/ls/selectionranges.go:ProvideSelectionRanges (sourceFile == nil)
#[test]
fn provide_selection_ranges_unknown_file_is_empty() {
    let ls = build_service(&[("/m.ts", "x;")], "/", &["/m.ts"]);
    assert!(ls
        .provide_selection_ranges("/missing.ts", &[pos(0, 0)])
        .is_empty());
}

// Guard: no requested positions yields no ranges.
// Go: internal/ls/selectionranges.go:ProvideSelectionRanges (empty Positions loop)
#[test]
fn provide_selection_ranges_no_positions_is_empty() {
    let ls = build_service(&[("/m.ts", "const x = 1;")], "/", &["/m.ts"]);
    assert!(ls.provide_selection_ranges("/m.ts", &[]).is_empty());
}

// Go: internal/ls/selectionranges.go:getSmartSelectionRange — a single-line
// comment trailing a node gets two stops: the whole comment, then its content
// after the `//`. A position inside the trailing `// hi` expands to the comment
// content -> the whole comment -> the source file.
#[test]
fn provide_selection_ranges_trailing_single_line_comment() {
    let ls = build_service(&[("/m.ts", "const x = 1; // hi")], "/", &["/m.ts"]);
    // Position on the `h` of `// hi` (byte 16, line 0 char 16).
    let ranges = ls.provide_selection_ranges("/m.ts", &[pos(0, 16)]);

    let file = sel(rng(0, 0, 0, 18), None);
    let comment = sel(rng(0, 13, 0, 18), Some(file));
    let content = sel(rng(0, 15, 0, 18), Some(comment));

    assert_eq!(ranges, vec![content]);
}

// Go: internal/ls/selectionranges.go:getSmartSelectionRange — a template span
// synthesizes a `${ ... }` stop (since `${`/`}` belong to sibling literals). A
// position on the `b` in `` `a${b}c` `` expands: identifier `b` -> `${b}` ->
// template inner content -> whole template -> source file.
#[test]
fn provide_selection_ranges_template_span_synthesized_stop() {
    let ls = build_service(&[("/m.ts", "`a${b}c`;")], "/", &["/m.ts"]);
    // Position on the `b` inside `${b}` (byte 4, line 0 char 4).
    let ranges = ls.provide_selection_ranges("/m.ts", &[pos(0, 4)]);

    let file = sel(rng(0, 0, 0, 9), None);
    let template = sel(rng(0, 0, 0, 8), Some(file));
    let inner = sel(rng(0, 1, 0, 7), Some(template));
    let span = sel(rng(0, 2, 0, 6), Some(inner));
    let ident = sel(rng(0, 4, 0, 5), Some(span));

    assert_eq!(ranges, vec![ident]);
}

// Go: internal/ls/selectionranges.go:getSmartSelectionRange (visitNodes) — a
// node list gets a stop spanning its first to last element. A position on the
// first parameter expands: parameter -> the whole parameter list -> the source
// file (the function declaration's range equals the file's and is deduped).
#[test]
fn provide_selection_ranges_parameter_list_span() {
    let ls = build_service(&[("/m.ts", "function f(a, b) {}")], "/", &["/m.ts"]);
    // Position on the first parameter `a` (byte 11, line 0 char 11).
    let ranges = ls.provide_selection_ranges("/m.ts", &[pos(0, 11)]);

    let file = sel(rng(0, 0, 0, 19), None);
    let params = sel(rng(0, 11, 0, 15), Some(file));
    // The parameter's range and its inner identifier's range coincide (deduped).
    let param = sel(rng(0, 11, 0, 12), Some(params));

    assert_eq!(ranges, vec![param]);
}

// Go: internal/ls/selectionranges.go:getSmartSelectionRange (visitNodes) — the
// import-specifier list gets its own stop. A position on the first specifier
// expands: specifier -> specifier list -> import clause -> source file.
#[test]
fn provide_selection_ranges_import_specifier_list_span() {
    let ls = build_service(&[("/m.ts", "import { a, b } from \"m\";")], "/", &["/m.ts"]);
    // Position on the first specifier `a` (byte 9, line 0 char 9).
    let ranges = ls.provide_selection_ranges("/m.ts", &[pos(0, 9)]);

    let file = sel(rng(0, 0, 0, 25), None);
    // The import clause and its `{ a, b }` named-imports range coincide (deduped).
    let clause = sel(rng(0, 7, 0, 15), Some(file));
    let elements = sel(rng(0, 9, 0, 13), Some(clause));
    // The specifier's range and its inner identifier's range coincide (deduped).
    let specifier = sel(rng(0, 9, 0, 10), Some(elements));

    assert_eq!(ranges, vec![specifier]);
}

// Go: internal/ls/selectionranges.go:ProvideSelectionRanges — one selection-range
// chain is returned per requested position, in order.
#[test]
fn provide_selection_ranges_returns_one_chain_per_position() {
    let ls = build_service(
        &[("/m.ts", "function f() {\n  return a + b;\n}")],
        "/",
        &["/m.ts"],
    );
    // Positions on `a` (line 1 char 9) and `b` (line 1 char 13).
    let ranges = ls.provide_selection_ranges("/m.ts", &[pos(1, 9), pos(1, 13)]);
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].range, rng(1, 9, 1, 10));
    assert_eq!(ranges[1].range, rng(1, 13, 1, 14));
}
