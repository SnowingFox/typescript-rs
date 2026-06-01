use tsgo_lsproto::{Position, Range};

use crate::test_support::build_service;
use crate::{DocumentHighlight, DocumentHighlightKind};

fn pos(line: u32, character: u32) -> Position {
    Position { line, character }
}

fn read(line: u32, start: u32, end: u32) -> DocumentHighlight {
    DocumentHighlight {
        range: range(line, start, end),
        kind: DocumentHighlightKind::Read,
    }
}

fn write(line: u32, start: u32, end: u32) -> DocumentHighlight {
    DocumentHighlight {
        range: range(line, start, end),
        kind: DocumentHighlightKind::Write,
    }
}

fn range(line: u32, start: u32, end: u32) -> Range {
    Range {
        start: Position {
            line,
            character: start,
        },
        end: Position {
            line,
            character: end,
        },
    }
}

// Go: internal/ls/documenthighlights.go:getSemanticDocumentHighlights /
// toDocumentHighlight — `let x = 1; x = 2; x;` highlights the declaration and
// the `x = 2` assignment target as Write, and the trailing `x;` read as Read.
#[test]
fn provide_document_highlights_classifies_write_and_read() {
    let mut ls = build_service(&[("/m.ts", "let x = 1; x = 2; x;")], "/", &["/m.ts"]);
    let highlights = ls.provide_document_highlights("/m.ts", pos(0, 4));
    assert_eq!(
        highlights,
        vec![write(0, 4, 5), write(0, 11, 12), read(0, 18, 19)]
    );
}

// Highlights are single-file + same-symbol only: a shadowing inner `x` query
// highlights only the inner declaration (Write, it has an initializer) and the
// inner use (Read), never the outer `x`.
// Go: internal/ls/documenthighlights.go:getSemanticDocumentHighlights (scope-aware)
#[test]
fn provide_document_highlights_respects_shadowing() {
    let src = "const x=1; function f(){ const x=2; x; } x;";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    let highlights = ls.provide_document_highlights("/m.ts", pos(0, 36));
    assert_eq!(highlights, vec![write(0, 31, 32), read(0, 36, 37)]);
}

// Querying from a use still highlights the whole symbol: the declaration (Write,
// it has an initializer) and both uses (Read).
// Go: internal/ls/documenthighlights.go:toDocumentHighlight
#[test]
fn provide_document_highlights_from_a_use_marks_declaration_write() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x; x;")], "/", &["/m.ts"]);
    let highlights = ls.provide_document_highlights("/m.ts", pos(0, 13));
    assert_eq!(
        highlights,
        vec![write(0, 6, 7), read(0, 13, 14), read(0, 16, 17)]
    );
}

// A compound assignment (`+=`) reads and writes, so its target is highlighted as
// Write (Go's `IsWriteAccess` is `accessKind != Read`).
// Go: internal/ast/ast.go:accessKind (compound assignment → ReadWrite)
#[test]
fn provide_document_highlights_compound_assignment_is_write() {
    let mut ls = build_service(&[("/m.ts", "let x = 0; x += 1;")], "/", &["/m.ts"]);
    let highlights = ls.provide_document_highlights("/m.ts", pos(0, 4));
    assert_eq!(highlights, vec![write(0, 4, 5), write(0, 11, 12)]);
}

// A postfix increment (`x++`) is a read-write access, highlighted as Write.
// Go: internal/ast/ast.go:accessKind (postfix `++` → ReadWrite)
#[test]
fn provide_document_highlights_postfix_increment_is_write() {
    let mut ls = build_service(&[("/m.ts", "let x = 0; x++;")], "/", &["/m.ts"]);
    let highlights = ls.provide_document_highlights("/m.ts", pos(0, 11));
    assert_eq!(highlights, vec![write(0, 4, 5), write(0, 11, 12)]);
}

// A non-identifier token (the `const` keyword) yields no highlights.
// Go: internal/ls/documenthighlights.go:provideDocumentHighlightsWorker (no symbol)
#[test]
fn provide_document_highlights_on_keyword_is_empty() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x;")], "/", &["/m.ts"]);
    let highlights = ls.provide_document_highlights("/m.ts", pos(0, 0));
    assert!(highlights.is_empty());
}

// An unknown file yields no highlights (no panic).
// Go: internal/ls/languageservice.go:getProgramAndFile (missing file)
#[test]
fn provide_document_highlights_unknown_file_is_empty() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x;")], "/", &["/m.ts"]);
    let highlights = ls.provide_document_highlights("/missing.ts", pos(0, 6));
    assert!(highlights.is_empty());
}
