use tsgo_lsproto::{Location, Position, Range};

use crate::test_support::build_service;

fn loc(line: u32, start: u32, end: u32) -> Location {
    Location {
        uri: tsgo_ls_lsconv::file_name_to_document_uri("/m.ts"),
        range: Range {
            start: Position {
                line,
                character: start,
            },
            end: Position {
                line,
                character: end,
            },
        },
    }
}

// Go: internal/ls/findallreferences.go:ProvideReferences — find-all-references on
// `const x = 1; x; x;` returns the declaration plus both uses (3 locations).
#[test]
fn provide_references_returns_declaration_and_all_uses() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x; x;")], "/", &["/m.ts"]);
    // Any `x` works; use the declaration name at byte/character 6.
    let locations = ls.provide_references(
        "/m.ts",
        Position {
            line: 0,
            character: 6,
        },
    );
    // Declaration name `x` at 6, first use at 13, second use at 16.
    assert_eq!(
        locations,
        vec![loc(0, 6, 7), loc(0, 13, 14), loc(0, 16, 17)]
    );
}

// Go: internal/ls/findallreferences.go:getReferencesAtLocation (getRelatedSymbol)
// — scope-aware resolution means a search at the inner `x` returns only the
// inner declaration + inner use, never the shadowed outer `x`.
#[test]
fn provide_references_respects_shadowing() {
    let src = "const x=1; function f(){ const x=2; x; } x;";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    // The inner `x` use is at byte/character 36.
    let locations = ls.provide_references(
        "/m.ts",
        Position {
            line: 0,
            character: 36,
        },
    );
    // Only the inner declaration (31) and the inner use (36) — not the outer
    // declaration (6) or outer use (41).
    assert_eq!(locations, vec![loc(0, 31, 32), loc(0, 36, 37)]);
}

// Searching from a *use* position returns the same complete set (declaration +
// all uses), not just references after the cursor.
// Go: internal/ls/findallreferences.go:getReferencedSymbolsForNode
#[test]
fn provide_references_from_a_use_position() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x; x;")], "/", &["/m.ts"]);
    // Start at the first use (byte/character 13), not the declaration.
    let locations = ls.provide_references(
        "/m.ts",
        Position {
            line: 0,
            character: 13,
        },
    );
    assert_eq!(
        locations,
        vec![loc(0, 6, 7), loc(0, 13, 14), loc(0, 16, 17)]
    );
}

// A function symbol's references span its declaration name and every call site.
// Go: internal/ls/findallreferences.go:ProvideReferences
#[test]
fn provide_references_function_across_call_sites() {
    let mut ls = build_service(&[("/m.ts", "function f(){}\nf();\nf();")], "/", &["/m.ts"]);
    // Start at the `function f` declaration name (line 0, byte/character 9).
    let locations = ls.provide_references(
        "/m.ts",
        Position {
            line: 0,
            character: 9,
        },
    );
    assert_eq!(locations, vec![loc(0, 9, 10), loc(1, 0, 1), loc(2, 0, 1)]);
}

// An unknown file yields no references (no panic).
// Go: internal/ls/languageservice.go:getProgramAndFile (missing file)
#[test]
fn provide_references_unknown_file_is_empty() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x;")], "/", &["/m.ts"]);
    let locations = ls.provide_references(
        "/missing.ts",
        Position {
            line: 0,
            character: 6,
        },
    );
    assert!(locations.is_empty());
}

// A non-identifier token (the `const` keyword) yields no references.
// Go: internal/ls/findallreferences.go:getReferencedSymbolsForNode (no symbol)
#[test]
fn provide_references_on_keyword_is_empty() {
    let mut ls = build_service(&[("/m.ts", "const x = 1; x;")], "/", &["/m.ts"]);
    let locations = ls.provide_references(
        "/m.ts",
        Position {
            line: 0,
            character: 0,
        },
    );
    assert!(locations.is_empty());
}
