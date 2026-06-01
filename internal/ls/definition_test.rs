use tsgo_lsproto::{Location, Position, Range};

use crate::test_support::build_service;

// Go: internal/ls/definition.go:ProvideDefinition / getDeclarationsFromLocation —
// go-to-definition at the use of `x` (line 2) jumps to the `x` binding's name
// range on line 1.
#[test]
fn provide_definition_local_variable_jumps_to_declaration_name() {
    let mut ls = build_service(&[("/m.ts", "const x = 1;\nx;")], "/", &["/m.ts"]);
    // The `x` use is the first token on line 1 (0-based line index 1).
    let locations = ls.provide_definition(
        "/m.ts",
        Position {
            line: 1,
            character: 0,
        },
    );
    assert_eq!(
        locations,
        vec![Location {
            uri: tsgo_ls_lsconv::file_name_to_document_uri("/m.ts"),
            range: Range {
                start: Position {
                    line: 0,
                    character: 6,
                },
                end: Position {
                    line: 0,
                    character: 7,
                },
            },
        }]
    );
}

fn single_range(line: u32, start: u32, end: u32) -> Vec<Location> {
    vec![Location {
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
    }]
}

// Go: internal/ls/definition.go:getDeclarationsFromLocation — go-to-definition
// at the `f` call (line 2) jumps to the `function f` declaration's name range.
#[test]
fn provide_definition_function_jumps_to_declaration_name() {
    let mut ls = build_service(&[("/m.ts", "function f(){}\nf();")], "/", &["/m.ts"]);
    // The `f` call is the first token on line 1.
    let locations = ls.provide_definition(
        "/m.ts",
        Position {
            line: 1,
            character: 0,
        },
    );
    // `function f` puts the name `f` at byte/character 9 on line 0.
    assert_eq!(locations, single_range(0, 9, 10));
}

// Go: internal/ls/definition.go:getDeclarationsFromLocation — go-to-definition
// at a parameter use jumps to the parameter declaration's name.
#[test]
fn provide_definition_parameter_jumps_to_declaration_name() {
    let mut ls = build_service(&[("/m.ts", "function f(p){ return p; }")], "/", &["/m.ts"]);
    // The `p` use inside the body is at byte/character 22.
    let locations = ls.provide_definition(
        "/m.ts",
        Position {
            line: 0,
            character: 22,
        },
    );
    // The parameter `p` is declared at byte/character 11.
    assert_eq!(locations, single_range(0, 11, 12));
}

// An unknown file yields no definitions (no panic).
// Go: internal/ls/languageservice.go:getProgramAndFile (missing file)
#[test]
fn provide_definition_unknown_file_is_empty() {
    let mut ls = build_service(&[("/m.ts", "const x = 1;\nx;")], "/", &["/m.ts"]);
    let locations = ls.provide_definition(
        "/missing.ts",
        Position {
            line: 1,
            character: 0,
        },
    );
    assert!(locations.is_empty());
}

// A non-identifier token (the `const` keyword) yields no definitions: Go's
// `getDeclarationsFromLocation` finds no symbol, and the reachable subset never
// feeds a synthesized keyword token to the checker.
// Go: internal/ls/definition.go:provideDefinitionWorker (no symbol)
#[test]
fn provide_definition_on_keyword_is_empty() {
    let mut ls = build_service(&[("/m.ts", "const x = 1;\nx;")], "/", &["/m.ts"]);
    // Position 0 on line 0 is the `const` keyword.
    let locations = ls.provide_definition(
        "/m.ts",
        Position {
            line: 0,
            character: 0,
        },
    );
    assert!(locations.is_empty());
}
