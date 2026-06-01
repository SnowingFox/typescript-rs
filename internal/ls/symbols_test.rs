use tsgo_lsproto::{Position, Range, SymbolKind};

use crate::symbols::DocumentSymbol;
use crate::test_support::build_service;

/// Builds an LSP range from `(line, char)` pairs.
fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Range {
    Range {
        start: Position {
            line: sl,
            character: sc,
        },
        end: Position {
            line: el,
            character: ec,
        },
    }
}

/// The LSP `SymbolKind` wire values used by these tests.
const NAMESPACE: SymbolKind = SymbolKind(3);
const CLASS: SymbolKind = SymbolKind(5);
const METHOD: SymbolKind = SymbolKind(6);
const PROPERTY: SymbolKind = SymbolKind(7);
const ENUM: SymbolKind = SymbolKind(10);
const INTERFACE: SymbolKind = SymbolKind(11);
const FUNCTION: SymbolKind = SymbolKind(12);
const VARIABLE: SymbolKind = SymbolKind(13);
const ENUM_MEMBER: SymbolKind = SymbolKind(22);

/// Flattens a symbol tree into `(name, kind, depth)` triples (pre-order), so a
/// test can assert names + kinds + nesting without spelling out every range.
fn outline(symbols: &[DocumentSymbol]) -> Vec<(String, SymbolKind, usize)> {
    fn walk(out: &mut Vec<(String, SymbolKind, usize)>, syms: &[DocumentSymbol], depth: usize) {
        for s in syms {
            out.push((s.name.clone(), s.kind, depth));
            walk(out, &s.children, depth + 1);
        }
    }
    let mut out = Vec::new();
    walk(&mut out, symbols, 0);
    out
}

// Go: internal/ls/symbols.go:getDocumentSymbolsForChildren — a top-level
// function and a class with a method + a property field produce `f`(Function)
// and `C`(Class) with children `m`(Method) and `x`(Property).
#[test]
fn provide_document_symbols_function_class_members() {
    let ls = build_service(
        &[("/m.ts", "function f(){}\nclass C { m(){} x = 1; }")],
        "/",
        &["/m.ts"],
    );
    let symbols = ls.provide_document_symbols("/m.ts");
    assert_eq!(
        outline(&symbols),
        vec![
            ("f".to_string(), FUNCTION, 0),
            ("C".to_string(), CLASS, 0),
            ("m".to_string(), METHOD, 1),
            ("x".to_string(), PROPERTY, 1),
        ]
    );
}

// Go: internal/ls/symbols.go:newDocumentSymbol — a symbol's `range` covers its
// whole declaration while `selectionRange` covers just its name.
#[test]
fn provide_document_symbols_range_vs_selection_range() {
    let ls = build_service(&[("/m.ts", "function foo() {}")], "/", &["/m.ts"]);
    let symbols = ls.provide_document_symbols("/m.ts");
    assert_eq!(symbols.len(), 1);
    let foo = &symbols[0];
    assert_eq!(foo.name, "foo");
    assert_eq!(foo.kind, FUNCTION);
    // Whole declaration `function foo() {}` is byte 0..17.
    assert_eq!(foo.range, range(0, 0, 0, 17));
    // The name `foo` is byte 9..12.
    assert_eq!(foo.selection_range, range(0, 9, 0, 12));
    assert!(foo.children.is_empty());
}

// An empty file yields no symbols (no panic).
// Go: internal/ls/symbols.go:getDocumentSymbolsForChildren (empty)
#[test]
fn provide_document_symbols_empty_file_is_empty() {
    let ls = build_service(&[("/m.ts", "")], "/", &["/m.ts"]);
    assert!(ls.provide_document_symbols("/m.ts").is_empty());
}

// An unknown file yields no symbols (no panic).
// Go: internal/ls/languageservice.go:getProgramAndFile (missing file)
#[test]
fn provide_document_symbols_unknown_file_is_empty() {
    let ls = build_service(&[("/m.ts", "function f(){}")], "/", &["/m.ts"]);
    assert!(ls.provide_document_symbols("/missing.ts").is_empty());
}

// Go: internal/ls/symbols.go:visit (VariableDeclaration) — top-level variables
// become `Variable` symbols.
#[test]
fn provide_document_symbols_variables() {
    let ls = build_service(&[("/m.ts", "const x = 1;\nlet y = 2;")], "/", &["/m.ts"]);
    assert_eq!(
        outline(&ls.provide_document_symbols("/m.ts")),
        vec![
            ("x".to_string(), VARIABLE, 0),
            ("y".to_string(), VARIABLE, 0),
        ]
    );
}

// Go: internal/ls/symbols.go:visit (EnumDeclaration / EnumMember) — an enum and
// its members.
#[test]
fn provide_document_symbols_enum_members() {
    let ls = build_service(&[("/m.ts", "enum E { A, B }")], "/", &["/m.ts"]);
    assert_eq!(
        outline(&ls.provide_document_symbols("/m.ts")),
        vec![
            ("E".to_string(), ENUM, 0),
            ("A".to_string(), ENUM_MEMBER, 1),
            ("B".to_string(), ENUM_MEMBER, 1),
        ]
    );
}

// Go: internal/ls/symbols.go:visit (InterfaceDeclaration + member group) — an
// interface with a method signature and a property signature.
#[test]
fn provide_document_symbols_interface_members() {
    let ls = build_service(
        &[("/m.ts", "interface I { m(): void; p: number; }")],
        "/",
        &["/m.ts"],
    );
    assert_eq!(
        outline(&ls.provide_document_symbols("/m.ts")),
        vec![
            ("I".to_string(), INTERFACE, 0),
            ("m".to_string(), METHOD, 1),
            ("p".to_string(), PROPERTY, 1),
        ]
    );
}

// Go: internal/ls/symbols.go:visit (ModuleDeclaration) — a namespace and a
// function nested in its body.
#[test]
fn provide_document_symbols_namespace() {
    let ls = build_service(
        &[("/m.ts", "namespace N { function g(){} }")],
        "/",
        &["/m.ts"],
    );
    assert_eq!(
        outline(&ls.provide_document_symbols("/m.ts")),
        vec![
            ("N".to_string(), NAMESPACE, 0),
            ("g".to_string(), FUNCTION, 1),
        ]
    );
}

// Go: internal/ls/symbols.go:mergeExpandos — two same-name namespaces merge
// into one, combining their members.
#[test]
fn provide_document_symbols_merges_same_name_namespaces() {
    let ls = build_service(
        &[(
            "/m.ts",
            "namespace N { export const a = 1; }\nnamespace N { export const b = 2; }",
        )],
        "/",
        &["/m.ts"],
    );
    assert_eq!(
        outline(&ls.provide_document_symbols("/m.ts")),
        vec![
            ("N".to_string(), NAMESPACE, 0),
            ("a".to_string(), VARIABLE, 1),
            ("b".to_string(), VARIABLE, 1),
        ]
    );
}

// Go: internal/ls/symbols.go:visit (VariableDeclaration -> initializer) — an
// object-literal initializer's property assignment and method become nested
// child symbols.
#[test]
fn provide_document_symbols_object_literal_initializer() {
    let ls = build_service(&[("/m.ts", "const o = { a: 1, m() {} };")], "/", &["/m.ts"]);
    assert_eq!(
        outline(&ls.provide_document_symbols("/m.ts")),
        vec![
            ("o".to_string(), VARIABLE, 0),
            ("a".to_string(), PROPERTY, 1),
            ("m".to_string(), METHOD, 1),
        ]
    );
}
