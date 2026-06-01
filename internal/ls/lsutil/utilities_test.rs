use super::*;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, Symbol};
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

fn parse(text: &str) -> tsgo_parser::ParseResult {
    parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts)
}

/// Recursively find the first node of `kind`.
fn find_first(arena: &NodeArena, root: NodeId, kind: Kind) -> Option<NodeId> {
    fn go(arena: &NodeArena, id: NodeId, kind: Kind, out: &mut Option<NodeId>) {
        if out.is_some() {
            return;
        }
        if arena.kind(id) == kind {
            *out = Some(id);
            return;
        }
        arena.for_each_child(id, &mut |c| {
            go(arena, c, kind, out);
            out.is_some()
        });
    }
    let mut out = None;
    go(arena, root, kind, &mut out);
    out
}

// Go: internal/ls/lsutil/utilities.go:IsNonContextualKeyword
#[test]
fn is_non_contextual_keyword_accepts_reserved_keywords() {
    for kind in [
        Kind::IfKeyword,
        Kind::ForKeyword,
        Kind::ReturnKeyword,
        Kind::FunctionKeyword,
        Kind::ClassKeyword,
    ] {
        assert!(is_non_contextual_keyword(kind), "{kind:?}");
    }
}

// Go: internal/ls/lsutil/utilities.go:IsNonContextualKeyword
#[test]
fn is_non_contextual_keyword_rejects_contextual_and_non_keywords() {
    // Contextual keywords can be used as identifiers, so they are excluded.
    for kind in [Kind::AsKeyword, Kind::OfKeyword, Kind::GetKeyword] {
        assert!(!is_non_contextual_keyword(kind), "{kind:?} is contextual");
    }
    // Non-keyword kinds.
    for kind in [Kind::Identifier, Kind::SourceFile, Kind::PlusToken] {
        assert!(
            !is_non_contextual_keyword(kind),
            "{kind:?} is not a keyword"
        );
    }
}

// Go: internal/ls/lsutil/utilities.go:QuotePreferenceFromString
#[test]
fn quote_preference_from_single_quoted_literal() {
    let r = parse("const x = 'a';");
    let lit = find_first(&r.arena, r.source_file, Kind::StringLiteral).unwrap();
    assert_eq!(
        quote_preference_from_string(&r.arena, lit),
        QuotePreference::Single
    );
}

// Go: internal/ls/lsutil/utilities.go:QuotePreferenceFromString
#[test]
fn quote_preference_from_double_quoted_literal() {
    let r = parse("const x = \"a\";");
    let lit = find_first(&r.arena, r.source_file, Kind::StringLiteral).unwrap();
    assert_eq!(
        quote_preference_from_string(&r.arena, lit),
        QuotePreference::Double
    );
}

// Go: internal/ls/lsutil/utilities.go:ModuleSpecifierToValidIdentifier
#[test]
fn module_specifier_camel_cases_across_invalid_chars() {
    assert_eq!(
        module_specifier_to_valid_identifier("./foo-bar", false),
        "fooBar"
    );
    assert_eq!(
        module_specifier_to_valid_identifier("./foo-bar-baz", false),
        "fooBarBaz"
    );
}

// Go: internal/ls/lsutil/utilities.go:ModuleSpecifierToValidIdentifier
#[test]
fn module_specifier_force_capitalize_uppercases_first() {
    assert_eq!(
        module_specifier_to_valid_identifier("./foo-bar", true),
        "FooBar"
    );
    assert_eq!(module_specifier_to_valid_identifier("./foo", true), "Foo");
}

// Go: internal/ls/lsutil/utilities.go:ModuleSpecifierToValidIdentifier
#[test]
fn module_specifier_strips_extension_and_index() {
    assert_eq!(
        module_specifier_to_valid_identifier("./foo/index.ts", false),
        "foo"
    );
    assert_eq!(
        module_specifier_to_valid_identifier("./bar.d.ts", false),
        "bar"
    );
}

// Go: internal/ls/lsutil/utilities.go:ModuleSpecifierToValidIdentifier
#[test]
fn module_specifier_keyword_collision_gets_underscore() {
    // "if" parses to a reserved keyword token, so it is prefixed with "_".
    assert_eq!(module_specifier_to_valid_identifier("./if", false), "_if");
}

// Go: internal/ls/lsutil/utilities.go:ModuleSpecifierToValidIdentifier
#[test]
fn module_specifier_all_invalid_chars_becomes_underscore() {
    // No identifier-valid characters: the result is empty, so it becomes "_".
    assert_eq!(module_specifier_to_valid_identifier("./---", false), "_");
}

// Go: internal/ls/lsutil/utilities.go:ModuleSymbolToValidIdentifier
#[test]
fn module_symbol_strips_quotes_then_converts() {
    let mut sym = Symbol {
        name: "\"./foo-bar\"".to_string(),
        ..Default::default()
    };
    assert_eq!(module_symbol_to_valid_identifier(&sym, false), "fooBar");

    sym.name = "\"./foo-bar\"".to_string();
    assert_eq!(module_symbol_to_valid_identifier(&sym, true), "FooBar");
}

// Silence unused import warning when NodeData is only used in doctests.
#[allow(dead_code)]
fn _uses_node_data(_d: &NodeData) {}
