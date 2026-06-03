use tsgo_ast::NodeId;
use tsgo_lsproto::{InsertTextFormat, Position, Range, TextEdit, VsOnAutoInsertResponseItem};

use crate::test_support::build_service;

/// Builds an LSP [`Position`].
fn pos(line: u32, character: u32) -> Position {
    Position { line, character }
}

/// Builds the expected snippet response item: a `$0`-prefixed closing
/// `text` inserted at a zero-width range at `position`.
fn snippet_at(position: Position, text: &str) -> VsOnAutoInsertResponseItem {
    VsOnAutoInsertResponseItem {
        vs_text_edit_format: InsertTextFormat::SNIPPET,
        vs_text_edit: TextEdit {
            range: Range {
                start: position.clone(),
                end: position,
            },
            new_text: text.to_string(),
        },
    }
}

// Go: internal/ls/autoinsert.go:ProvideOnAutoInsert — typing `>` with the
// cursor inside the JSX text of an UNCLOSED `<div>` returns a snippet edit that
// inserts the matching `</div>` closing tag at the cursor (the `IsJsxText`
// element branch). Ground truth: fourslash `autoCloseTag` marker /5
// (`<div> text |` -> `</div>`).
#[test]
fn provide_on_auto_insert_unclosed_element_inserts_closing_tag() {
    let ls = build_service(&[("/m.tsx", "const x = <div> text ;")], "/", &["/m.tsx"]);
    // Cursor on the `x` of ` text ` (byte 18), inside the JSX text node.
    let result = ls.provide_on_auto_insert("/m.tsx", pos(0, 18), ">");
    assert_eq!(result, Some(snippet_at(pos(0, 18), "$0</div>")));
}

// Go: internal/ls/autoinsert.go:ProvideOnAutoInsert — typing `>` with the
// cursor inside the JSX text of an UNCLOSED fragment `<>` inserts `</>` (the
// `IsJsxText` fragment branch + `isUnclosedFragment`). Ground truth: fourslash
// `autoCloseFragment` marker /5 (`<> text |` -> `</>`).
#[test]
fn provide_on_auto_insert_unclosed_fragment_inserts_closing_fragment() {
    let ls = build_service(&[("/m.tsx", "const x = <> text ;")], "/", &["/m.tsx"]);
    // Cursor on the `x` of ` text ` (byte 15), inside the JSX text node.
    let result = ls.provide_on_auto_insert("/m.tsx", pos(0, 15), ">");
    assert_eq!(result, Some(snippet_at(pos(0, 15), "$0</>")));
}

// Go: internal/ls/autoinsert.go:ProvideOnAutoInsert — a tag name containing `$`
// (a valid JSX identifier character) is snippet-escaped (`</$Foo>` becomes
// `</\$Foo>`) via `escapeSnippetText`. Ground truth: fourslash
// `autoCloseTagsWithTriviaAndComplexNames` marker /10 (`<$Foo>`).
#[test]
fn provide_on_auto_insert_escapes_dollar_in_tag_name() {
    let ls = build_service(&[("/m.tsx", "const x = <$Foo> text ;")], "/", &["/m.tsx"]);
    // Cursor inside ` text ` (byte 19).
    let result = ls.provide_on_auto_insert("/m.tsx", pos(0, 19), ">");
    assert_eq!(result, Some(snippet_at(pos(0, 19), "$0</\\$Foo>")));
}

// Go: internal/ls/autoinsert.go:ProvideOnAutoInsert — a namespaced tag name
// (`<ns:tag>`) is rebuilt via `EntityNameToString` (the `JsxNamespacedName`
// arm) into `</ns:tag>`. Ground truth: fourslash
// `autoCloseTagsWithTriviaAndComplexNames` marker /2 (`<ns:sometag>`).
#[test]
fn provide_on_auto_insert_namespaced_tag_name() {
    let ls = build_service(&[("/m.tsx", "const x = <ns:tag> text ;")], "/", &["/m.tsx"]);
    // Cursor inside ` text ` (byte 21).
    let result = ls.provide_on_auto_insert("/m.tsx", pos(0, 21), ">");
    assert_eq!(result, Some(snippet_at(pos(0, 21), "$0</ns:tag>")));
}

// Go: internal/ls/autoinsert.go:ProvideOnAutoInsert — a property-access tag
// name (`<a.b>`) is rebuilt via `EntityNameToString` (the
// `PropertyAccessExpression` arm) into `</a.b>`. Ground truth: fourslash
// `autoCloseTagsWithTriviaAndComplexNames` marker /5 (`someModule.SomeComponent`).
#[test]
fn provide_on_auto_insert_property_access_tag_name() {
    let ls = build_service(&[("/m.tsx", "const x = <a.b> text ;")], "/", &["/m.tsx"]);
    // Cursor inside ` text ` (byte 18).
    let result = ls.provide_on_auto_insert("/m.tsx", pos(0, 18), ">");
    assert_eq!(result, Some(snippet_at(pos(0, 18), "$0</a.b>")));
}

// Go: internal/ls/autoinsert.go:isUnclosedTag — an inner element that looks
// closed (`<div>...</div>`) is still treated as unclosed when nested in a
// same-named parent that is itself unclosed (the recursive `parent` branch).
// Ground truth: fourslash `autoCloseTag` marker /9 (nested unclosed -> `</div>`).
#[test]
fn provide_on_auto_insert_nested_unclosed_same_name_inserts_closing_tag() {
    let ls = build_service(
        &[("/m.tsx", "const x = <div> <div> text </div>;")],
        "/",
        &["/m.tsx"],
    );
    // Cursor inside the inner element's ` text ` (byte 24).
    let result = ls.provide_on_auto_insert("/m.tsx", pos(0, 24), ">");
    assert_eq!(result, Some(snippet_at(pos(0, 24), "$0</div>")));
}

// Guard: a trigger character other than `>` yields `None` (Go's first check
// `if params.VSCh != ">"`).
// Go: internal/ls/autoinsert.go:ProvideOnAutoInsert (VSCh != ">")
#[test]
fn provide_on_auto_insert_non_greater_than_char_is_none() {
    let ls = build_service(&[("/m.tsx", "const x = <div> text ;")], "/", &["/m.tsx"]);
    assert_eq!(ls.provide_on_auto_insert("/m.tsx", pos(0, 18), "/"), None);
}

// Guard: a CLOSED element (`<div> foo </div>`) is not unclosed, so the result
// is `None`. Ground truth: fourslash `autoCloseTag` marker /1 (closed -> nil).
// Go: internal/ls/autoinsert.go:ProvideOnAutoInsert (closingText == "")
#[test]
fn provide_on_auto_insert_closed_element_is_none() {
    let ls = build_service(
        &[("/m.tsx", "const x = <div> foo </div>;")],
        "/",
        &["/m.tsx"],
    );
    // Cursor inside ` foo ` (byte 17).
    assert_eq!(ls.provide_on_auto_insert("/m.tsx", pos(0, 17), ">"), None);
}

// Guard: a CLOSED fragment (`<> foo </>`) is not unclosed, so the result is
// `None`. Ground truth: fourslash `autoCloseFragment` marker /1 (closed -> nil).
// Go: internal/ls/autoinsert.go:ProvideOnAutoInsert (closingText == "")
#[test]
fn provide_on_auto_insert_closed_fragment_is_none() {
    let ls = build_service(&[("/m.tsx", "const x = <> foo </>;")], "/", &["/m.tsx"]);
    // Cursor inside ` foo ` (byte 14).
    assert_eq!(ls.provide_on_auto_insert("/m.tsx", pos(0, 14), ">"), None);
}

// Guard: a cursor in a non-JSX construct (a variable name) yields `None` (the
// preceding token is neither a `>` token nor JSX text).
// Go: internal/ls/autoinsert.go:ProvideOnAutoInsert (closingText == "")
#[test]
fn provide_on_auto_insert_non_jsx_position_is_none() {
    let ls = build_service(&[("/m.tsx", "const x = 1;")], "/", &["/m.tsx"]);
    // Cursor on the `x` identifier (byte 6).
    assert_eq!(ls.provide_on_auto_insert("/m.tsx", pos(0, 6), ">"), None);
}

// Guard: an unknown file yields `None` (no panic).
// Go: internal/ls/autoinsert.go:ProvideOnAutoInsert (sourceFile == nil)
#[test]
fn provide_on_auto_insert_unknown_file_is_none() {
    let ls = build_service(&[("/m.tsx", "const x = <div> text ;")], "/", &["/m.tsx"]);
    assert_eq!(
        ls.provide_on_auto_insert("/missing.tsx", pos(0, 18), ">"),
        None
    );
}

// Go: internal/ls/completions.go:escapeSnippetText — `$` is backslash-escaped
// so a tag name containing `$` is inserted literally; text without `$` is
// unchanged.
#[test]
fn escape_snippet_text_escapes_dollar_only() {
    assert_eq!(super::escape_snippet_text("</div>"), "</div>");
    assert_eq!(super::escape_snippet_text("</$Foo>"), "</\\$Foo>");
    assert_eq!(super::escape_snippet_text("$a$b"), "\\$a\\$b");
    assert_eq!(super::escape_snippet_text(""), "");
}

// `is_synthesized_token` mirrors astnav's high-bit tag: a small (real) arena id
// is not synthesized; an id with the high bit set is.
#[test]
fn is_synthesized_token_detects_high_bit() {
    assert!(!super::is_synthesized_token(NodeId(0)));
    assert!(!super::is_synthesized_token(NodeId(123)));
    assert!(super::is_synthesized_token(NodeId(1 << 31)));
    assert!(super::is_synthesized_token(NodeId((1 << 31) | 5)));
}
