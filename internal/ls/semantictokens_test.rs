use tsgo_lsproto::{Position, Range, ResolvedSemanticTokensClientCapabilities};

use crate::semantictokens::semantic_tokens_legend;
use crate::test_support::build_service;

/// Collects the encoded semantic-tokens `data` for a single in-memory file,
/// asserting the provider returned a non-null result.
fn tokens(src: &str) -> Vec<u32> {
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    ls.provide_semantic_tokens("/m.ts")
        .expect("expected semantic tokens")
        .data
}

// Go: internal/ls/semantictokens.go:ProvideSemanticTokens — `class C {}` classifies
// `C` as `class` (token type 1) with the `declaration` modifier (bit 0 = 1),
// encoded at deltaLine 0, deltaChar 6, length 1.
#[test]
fn class_declaration_is_class_with_declaration_modifier() {
    // `class ` is 6 chars, so `C` is at character 6.
    assert_eq!(tokens("class C {}"), vec![0, 6, 1, 1, 1]);
}

// Go: internal/ls/semantictokens.go:collectSemanticTokensInRange — a `const`
// variable is `variable` (token type 8) with `declaration` (bit 0 = 1) plus
// `readonly` (bit 2 = 4, because `GetCombinedNodeFlags` folds the list's `const`
// flag onto the declaration); 1 | 4 = 5.
#[test]
fn const_variable_is_variable_with_declaration_and_readonly() {
    // `const ` is 6 chars, so `x` is at character 6.
    assert_eq!(tokens("const x = 1"), vec![0, 6, 1, 8, 5]);
}

// A `let` variable is `variable` with only the `declaration` modifier — no
// `readonly`, since `GetCombinedNodeFlags` finds no `const` flag.
// Go: internal/ls/semantictokens.go:collectSemanticTokensInRange
#[test]
fn let_variable_is_variable_with_declaration_only() {
    assert_eq!(tokens("let y = 1"), vec![0, 4, 1, 8, 1]);
}

// A top-level `function` declaration is `function` (token type 13) with the
// `declaration` modifier; not `local` (its parent is the source file).
// Go: internal/ls/semantictokens.go:classifySymbol / tokenFromDeclarationMapping
#[test]
fn function_declaration_is_function_with_declaration() {
    // `function ` is 9 chars, so `f` is at character 9.
    assert_eq!(tokens("function f(){}"), vec![0, 9, 1, 13, 1]);
}

// An `interface` declaration is `interface` (token type 3) with the
// `declaration` modifier (the symbol has the `Interface` flag and the
// declaration-name meaning is `Type`).
// Go: internal/ls/semantictokens.go:classifySymbol (interface case)
#[test]
fn interface_declaration_is_interface_with_declaration() {
    // `interface ` is 10 chars, so `I` is at character 10.
    assert_eq!(tokens("interface I {}"), vec![0, 10, 1, 3, 1]);
}

// An `enum` declaration is `enum` (token type 2) with the `declaration`
// modifier (the symbol has the `Enum` flag; no `readonly` since the enum symbol
// is not an enum *member*).
// Go: internal/ls/semantictokens.go:classifySymbol (enum case)
#[test]
fn enum_declaration_is_enum_with_declaration() {
    // `enum ` is 5 chars, so `E` is at character 5.
    assert_eq!(tokens("enum E {}"), vec![0, 5, 1, 2, 1]);
}

// A class property is `property` (token type 9) with `declaration`. Two tokens
// on the same line exercise the relative `deltaStartChar` encoding: `C` at
// char 6, then `x` at char 10 (delta 4).
// Go: internal/ls/semantictokens.go:tokenFromDeclarationMapping (PropertyDeclaration)
#[test]
fn class_property_is_property_and_uses_relative_delta_char() {
    // `class C { x = 1 }`: `C` at char 6, `x` at char 10.
    assert_eq!(
        tokens("class C { x = 1 }"),
        vec![0, 6, 1, 1, 1, /* x */ 0, 4, 1, 9, 1]
    );
}

// A function parameter is `parameter` (token type 7) with `declaration`; the
// parameter token follows the function-name token on the same line (delta 2).
// Go: internal/ls/semantictokens.go:tokenFromDeclarationMapping (Parameter)
#[test]
fn function_parameter_is_parameter() {
    // `function f(p){}`: `f` at char 9, `p` at char 11.
    assert_eq!(
        tokens("function f(p){}"),
        vec![0, 9, 1, 13, 1, /* p */ 0, 2, 1, 7, 1]
    );
}

// Tokens on different lines exercise the `deltaLine` encoding: a function
// declaration plus two call-site uses across three lines. The declaration name
// carries `declaration`; the uses carry no modifier.
// Go: internal/ls/semantictokens.go:encodeSemanticTokens (relative line deltas)
#[test]
fn function_uses_across_lines_use_relative_line_delta() {
    assert_eq!(
        tokens("function f(){}\nf();\nf();"),
        vec![
            0, 9, 1, 13, 1, // `f` declaration on line 0, char 9
            1, 0, 1, 13, 0, // first use on line 1, char 0
            1, 0, 1, 13, 0, // second use on line 2, char 0
        ]
    );
}

// A `static` class member carries the `static` modifier (bit 3 = 8) on top of
// `declaration` (bit 0 = 1): 1 | 8 = 9.
// Go: internal/ls/semantictokens.go:collectSemanticTokensInRange (ModifierFlagsStatic)
#[test]
fn static_member_carries_static_modifier() {
    // `class C { static x = 1 }`: `C` at char 6, `x` at char 17.
    assert_eq!(
        tokens("class C { static x = 1 }"),
        vec![0, 6, 1, 1, 1, /* x */ 0, 11, 1, 9, 9]
    );
}

// An `async` function carries the `async` modifier (bit 6 = 64) on top of
// `declaration` (bit 0 = 1): 1 | 64 = 65.
// Go: internal/ls/semantictokens.go:collectSemanticTokensInRange (ModifierFlagsAsync)
#[test]
fn async_function_carries_async_modifier() {
    // `async function f(){}`: `f` at char 15.
    assert_eq!(tokens("async function f(){}"), vec![0, 15, 1, 13, 65]);
}

// A `readonly` class member carries the `readonly` modifier (bit 2 = 4) on top
// of `declaration`: 1 | 4 = 5.
// Go: internal/ls/semantictokens.go:collectSemanticTokensInRange (ModifierFlagsReadonly)
#[test]
fn readonly_member_carries_readonly_modifier() {
    // `class C { readonly x = 1 }`: `C` at char 6, `x` at char 19.
    assert_eq!(
        tokens("class C { readonly x = 1 }"),
        vec![0, 6, 1, 1, 1, /* x */ 0, 13, 1, 9, 5]
    );
}

// A variable declared inside a function body carries the `local` modifier (bit
// 10 = 1024); a `const` adds `readonly` (4): 1 | 4 | 1024 = 1029.
// Go: internal/ls/semantictokens.go:isLocalDeclaration
#[test]
fn nested_const_variable_carries_local_modifier() {
    // `function f(){ const y = 1; }`: `f` at char 9, `y` at char 20.
    assert_eq!(
        tokens("function f(){ const y = 1; }"),
        vec![0, 9, 1, 13, 1, /* y */ 0, 11, 1, 8, 1029]
    );
}

// A function declared inside another function carries `local` (1024) on top of
// `declaration` (1): 1 | 1024 = 1025; the outer top-level function is not local.
// Go: internal/ls/semantictokens.go:isLocalDeclaration (FunctionDeclaration case)
#[test]
fn nested_function_carries_local_modifier() {
    // `function f(){ function g(){} }`: `f` at char 9, `g` at char 23.
    assert_eq!(
        tokens("function f(){ function g(){} }"),
        vec![0, 9, 1, 13, 1, /* g */ 0, 14, 1, 13, 1025]
    );
}

// GUARD: a file with no classifiable identifier (only literals / operators /
// punctuation) yields a null result, not an empty token array.
// Go: internal/ls/semantictokens.go:ProvideSemanticTokens (len(tokens) == 0)
#[test]
fn no_identifiers_yields_null() {
    let mut ls = build_service(&[("/m.ts", "1 + 2;")], "/", &["/m.ts"]);
    assert!(ls.provide_semantic_tokens("/m.ts").is_none());
}

// GUARD: keywords and punctuation are not classified — only the identifier `x`
// is, never `const` / `=` / `1` / `;`.
// Go: internal/ls/semantictokens.go:collectSemanticTokensInRange (IsIdentifier guard)
#[test]
fn only_identifiers_are_classified_not_keywords() {
    // The single token is `x`; `const`, `=`, `1`, `;` contribute nothing.
    assert_eq!(tokens("const x = 1;"), vec![0, 6, 1, 8, 5]);
}

// GUARD: the `Infinity` / `NaN` globals are never classified, even alongside a
// real classified identifier; only `x` appears in the output.
// Go: internal/ls/semantictokens.go:isInfinityOrNaNString
#[test]
fn infinity_global_is_not_classified() {
    // `let x = Infinity;`: only `x` (char 4) is emitted, never `Infinity`.
    assert_eq!(tokens("let x = Infinity;"), vec![0, 4, 1, 8, 1]);
}

// GUARD: an unknown file yields a null result (no panic).
// Go: internal/ls/languageservice.go:getProgramAndFile (missing file)
#[test]
fn unknown_file_yields_null() {
    let mut ls = build_service(&[("/m.ts", "const x = 1;")], "/", &["/m.ts"]);
    assert!(ls.provide_semantic_tokens("/missing.ts").is_none());
}

// The range request classifies only the identifiers whose span overlaps the
// range: a range covering just line 0 returns `a` but not the line-1 `b`.
// Go: internal/ls/semantictokens.go:ProvideSemanticTokensRange
#[test]
fn range_request_limits_tokens_to_the_range() {
    let src = "const a = 1;\nconst b = 2;";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);

    // Full document: both `a` (line 0) and `b` (line 1).
    let full = ls
        .provide_semantic_tokens("/m.ts")
        .expect("expected full tokens")
        .data;
    assert_eq!(full, vec![0, 6, 1, 8, 5, /* b */ 1, 6, 1, 8, 5]);

    // Range covering only line 0: just `a`.
    let ranged = ls
        .provide_semantic_tokens_range(
            "/m.ts",
            Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 12,
                },
            },
        )
        .expect("expected ranged tokens")
        .data;
    assert_eq!(ranged, vec![0, 6, 1, 8, 5]);
}

// The legend keeps Go's canonical token-type / modifier order, dropping any the
// client did not advertise (even if the client lists them out of order).
// Go: internal/ls/semantictokens.go:SemanticTokensLegend
#[test]
fn legend_filters_and_preserves_canonical_order() {
    // Listed out of order and with an unsupported extra ("madeup").
    let caps = ResolvedSemanticTokensClientCapabilities {
        token_types: vec![
            "variable".to_string(),
            "class".to_string(),
            "madeup".to_string(),
        ],
        token_modifiers: vec!["readonly".to_string(), "declaration".to_string()],
        ..Default::default()
    };

    let legend = semantic_tokens_legend(&caps);
    // Canonical order: `class` (index 1) precedes `variable` (index 8).
    assert_eq!(
        legend.token_types,
        vec!["class".to_string(), "variable".to_string()]
    );
    // Canonical order: `declaration` (bit 0) precedes `readonly` (bit 2).
    assert_eq!(
        legend.token_modifiers,
        vec!["declaration".to_string(), "readonly".to_string()]
    );
}

// An empty client legend yields an empty server legend.
// Go: internal/ls/semantictokens.go:SemanticTokensLegend
#[test]
fn legend_empty_capabilities_yield_empty_legend() {
    let caps = ResolvedSemanticTokensClientCapabilities::default();
    let legend = semantic_tokens_legend(&caps);
    assert!(legend.token_types.is_empty());
    assert!(legend.token_modifiers.is_empty());
}
