use tsgo_ast::Kind;
use tsgo_core::tristate::Tristate;
use tsgo_ls_lsutil::{IncludeInlayParameterNameHints, InlayHintsPreferences};
use tsgo_lsproto::{Position, Range};

use super::{is_any_inlay_hint_enabled, is_type_node_kind};
use crate::test_support::build_service;

/// An LSP range spanning a whole single-line file (line 0, generous columns).
fn whole_file() -> Range {
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 1000,
            character: 0,
        },
    }
}

/// Preferences with only enum-member-value hints enabled.
fn enum_member_prefs() -> InlayHintsPreferences {
    InlayHintsPreferences {
        include_inlay_enum_member_value_hints: Tristate::True,
        ..Default::default()
    }
}

/// Collects the inlay hints for a single in-memory file over the whole file,
/// asserting the provider returned a non-null result.
fn hints(src: &str, prefs: &InlayHintsPreferences) -> Vec<tsgo_lsproto::InlayHint> {
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    ls.provide_inlay_hints("/m.ts", whole_file(), prefs)
        .expect("expected a (possibly empty) inlay-hint array")
}

// Go: internal/ls/inlay_hints.go:visitEnumMember / addEnumMemberValueHints — an
// enum member with no initializer renders `= <auto value>` after its name with
// left padding and no kind. `enum E { A }` -> `A` auto-numbers to 0.
#[test]
fn enum_member_without_initializer_shows_auto_value() {
    let result = hints("enum E { A }", &enum_member_prefs());
    assert_eq!(result.len(), 1);
    let hint = &result[0];
    assert_eq!(hint.label.string.as_deref(), Some("= 0"));
    assert_eq!(hint.padding_left, Some(true));
    assert_eq!(hint.padding_right, None);
    assert_eq!(hint.kind, None);
    // `enum E { A }`: the member name `A` ends at character 10, where the hint
    // is anchored (`member.End()`).
    assert_eq!(
        hint.position,
        Position {
            line: 0,
            character: 10
        }
    );
}

/// Preferences with only variable-type hints enabled.
fn variable_type_prefs() -> InlayHintsPreferences {
    InlayHintsPreferences {
        include_inlay_variable_type_hints: Tristate::True,
        ..Default::default()
    }
}

// Go: internal/ls/inlay_hints.go:visitVariableLikeDeclaration — an un-annotated
// mutable `let x = 1` gets a `: number` type hint (the widened initializer
// type), anchored at the name's end, with `Type` kind and left padding.
#[test]
fn variable_type_hint_for_let_shows_widened_number() {
    let result = hints("let x = 1", &variable_type_prefs());
    assert_eq!(result.len(), 1);
    let hint = &result[0];
    assert_eq!(hint.label.string.as_deref(), Some(": number"));
    assert_eq!(hint.kind, Some(tsgo_lsproto::InlayHintKind::TYPE));
    assert_eq!(hint.padding_left, Some(true));
    assert_eq!(hint.padding_right, None);
    // `let x` -> the name `x` ends at character 5.
    assert_eq!(
        hint.position,
        Position {
            line: 0,
            character: 5
        }
    );
}

// Go: internal/ls/inlay_hints.go:visitVariableLikeDeclaration — a mutable
// `let x = "s"` widens its string-literal initializer to `string`.
#[test]
fn variable_type_hint_for_let_widens_string() {
    let result = hints("let x = \"s\"", &variable_type_prefs());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].label.string.as_deref(), Some(": string"));
}

// GUARD Go: internal/ls/inlay_hints.go:isHintableDeclaration — a `const` bound
// to a hintable literal (`const x = 1`) gets NO hint (the literal makes the type
// obvious), unlike the `let` case.
#[test]
fn variable_type_hint_suppressed_for_const_literal() {
    assert!(hints("const x = 1", &variable_type_prefs()).is_empty());
    assert!(hints("const x = \"s\"", &variable_type_prefs()).is_empty());
    assert!(hints("const x = true", &variable_type_prefs()).is_empty());
}

// GUARD Go: internal/ls/inlay_hints.go:visitVariableLikeDeclaration (typeAnnotation
// != nil) — an annotated declaration shows its type in source, so no hint.
#[test]
fn variable_type_hint_suppressed_when_annotated() {
    assert!(hints("let x: number = 1", &variable_type_prefs()).is_empty());
    assert!(hints("const x: number = 1", &variable_type_prefs()).is_empty());
}

// Go: internal/ls/inlay_hints.go:isHintableDeclaration — a `const` initialized
// from a CALL is hintable (the type is not obvious), and the inferred return
// type is rendered.
#[test]
fn variable_type_hint_for_const_call_inference() {
    let result = hints(
        "declare function f(): string;\nconst x = f();",
        &variable_type_prefs(),
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].label.string.as_deref(), Some(": string"));
}

// GUARD Go: internal/ls/inlay_hints.go:visitVariableLikeDeclaration (IsBindingPattern)
// — a destructuring declaration (`const { a } = o`) gets no variable-type hint.
#[test]
fn variable_type_hint_suppressed_for_binding_pattern() {
    let src = "declare const o: { a: number };\nconst { a } = o;";
    assert!(hints(src, &variable_type_prefs()).is_empty());
}

// Go: internal/ls/inlay_hints.go:visitVariableLikeDeclaration
// (…WhenTypeMatchesName) — when the inferred type's text case-insensitively
// equals the variable name, the hint is suppressed by default and shown only
// when the `…WhenTypeMatchesName` toggle is on.
#[test]
fn variable_type_hint_matches_name_suppression() {
    let src = "interface Foo {}\ndeclare function make(): Foo;\nconst foo = make();";

    // Default (toggle off): `foo` matches type `Foo` (case-insensitive) -> none.
    assert!(hints(src, &variable_type_prefs()).is_empty());

    // Toggle on: the matching hint is shown.
    let prefs = InlayHintsPreferences {
        include_inlay_variable_type_hints: Tristate::True,
        include_inlay_variable_type_hints_when_type_matches_name: Tristate::True,
        ..Default::default()
    };
    let result = hints(src, &prefs);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].label.string.as_deref(), Some(": Foo"));
}

/// Preferences with only property-declaration-type hints enabled.
fn property_type_prefs() -> InlayHintsPreferences {
    InlayHintsPreferences {
        include_inlay_property_declaration_type_hints: Tristate::True,
        ..Default::default()
    }
}

// Go: internal/ls/inlay_hints.go:visitVariableLikeDeclaration (property arm) —
// an un-annotated class field with an initializer (`a = 1`) gets a `: number`
// type hint (the widened initializer type). Matches the inlayHintsPropertyDeclarations
// baseline's `a = 1` -> `: number`.
#[test]
fn property_type_hint_for_initialized_field() {
    let result = hints("class C { a = 1 }", &property_type_prefs());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].label.string.as_deref(), Some(": number"));
    assert_eq!(result[0].kind, Some(tsgo_lsproto::InlayHintKind::TYPE));
}

// GUARD Go: internal/ls/inlay_hints.go:visitVariableLikeDeclaration — an
// annotated field (`b: number = 2`) shows no hint.
#[test]
fn property_type_hint_suppressed_when_annotated() {
    assert!(hints("class C { b: number = 2 }", &property_type_prefs()).is_empty());
}

// GUARD Go: internal/ls/inlay_hints.go:visitVariableLikeDeclaration (no
// initializer && type-at-location is `any`) — a bare `c;` field gets no hint.
// Matches the inlayHintsPropertyDeclarations baseline's `c;` (no hint).
#[test]
fn property_type_hint_suppressed_for_uninitialized_any_field() {
    assert!(hints("class C { c; }", &property_type_prefs()).is_empty());
}

// GUARD: the property-declaration toggle does not fire on plain variable
// declarations, and the variable toggle does not fire on class properties (the
// two kinds are gated independently in the dispatch chain).
#[test]
fn property_and_variable_toggles_are_independent() {
    // property toggle only: a `let` variable gets no hint.
    assert!(hints("let x = 1", &property_type_prefs()).is_empty());
    // variable toggle only: a class field gets no hint.
    assert!(hints("class C { a = 1 }", &variable_type_prefs()).is_empty());
}

/// The `= <value>` label strings of every hint, in walk (source) order.
fn labels(result: &[tsgo_lsproto::InlayHint]) -> Vec<String> {
    result
        .iter()
        .map(|h| h.label.string.clone().expect("a string label"))
        .collect()
}

// Go: internal/ls/inlay_hints.go:visitEnumMember — a member WITH an explicit
// initializer renders no hint (the value is already in the source).
#[test]
fn enum_member_with_initializer_shows_no_hint() {
    assert!(hints("enum E { A = 5 }", &enum_member_prefs()).is_empty());
}

// Go: internal/ls/inlay_hints.go:visitEnumMember — each member with no
// initializer auto-numbers from the previous value: `A`=0, `B`=1, `C`=2, in
// source order.
#[test]
fn enum_members_auto_number_in_source_order() {
    let result = hints("enum E { A, B, C }", &enum_member_prefs());
    assert_eq!(labels(&result), vec!["= 0", "= 1", "= 2"]);
}

// Go: internal/ls/inlay_hints.go:visitEnumMember — a member with an initializer
// is skipped, but a following member with no initializer still auto-numbers
// (from the initialized value + 1): `A = 1` (no hint), `B` -> `= 2`.
#[test]
fn enum_member_after_initializer_auto_numbers_from_it() {
    let result = hints("enum E { A = 1, B }", &enum_member_prefs());
    assert_eq!(labels(&result), vec!["= 2"]);
}

// Go: internal/ls/inlay_hints.go:visitEnumMember -> GetConstantValue — for a
// member *node* the value is folded regardless of const-ness, so a `const enum`
// member with no initializer still gets a `= 0` hint.
#[test]
fn const_enum_member_shows_auto_value() {
    let result = hints("const enum E { A }", &enum_member_prefs());
    assert_eq!(labels(&result), vec!["= 0"]);
}

// The range request prunes hints whose member does not overlap the span: a
// range covering only line 0 returns the line-0 member but not the line-2 one.
// Go: internal/ls/inlay_hints.go:visit (span.Intersects guard)
#[test]
fn range_request_limits_hints_to_the_range() {
    let src = "enum E {\n  A,\n}\nenum F {\n  B,\n}";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);

    // Whole file: both `A` (line 1) and `B` (line 4).
    let full = ls
        .provide_inlay_hints("/m.ts", whole_file(), &enum_member_prefs())
        .expect("full hints");
    assert_eq!(labels(&full), vec!["= 0", "= 0"]);

    // Range covering only the first enum (lines 0..=2): just `A`.
    let ranged = ls
        .provide_inlay_hints(
            "/m.ts",
            Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 2,
                    character: 1,
                },
            },
            &enum_member_prefs(),
        )
        .expect("ranged hints");
    assert_eq!(labels(&ranged), vec!["= 0"]);
}

// `is_type_node_kind` matches Go's `IsTypeNodeKind`: the explicit keyword /
// JSDoc / `ExpressionWithTypeArguments` cases plus the
// `FirstTypeNode..=LastTypeNode` range; non-type kinds are excluded.
// Go: internal/ast/utilities.go:IsTypeNodeKind
#[test]
fn is_type_node_kind_matches_go() {
    // Explicit-case keywords and `ExpressionWithTypeArguments`.
    assert!(is_type_node_kind(Kind::NumberKeyword));
    assert!(is_type_node_kind(Kind::ExpressionWithTypeArguments));
    assert!(is_type_node_kind(Kind::JSDocAllType));
    // In the `FirstTypeNode..=LastTypeNode` range.
    assert!(is_type_node_kind(Kind::TypeReference));
    assert!(is_type_node_kind(Kind::FIRST_TYPE_NODE));
    assert!(is_type_node_kind(Kind::LAST_TYPE_NODE));
    // Non-type kinds.
    assert!(!is_type_node_kind(Kind::Identifier));
    assert!(!is_type_node_kind(Kind::EnumMember));
    assert!(!is_type_node_kind(Kind::CallExpression));
}

// Go: internal/ls/inlay_hints.go:isAnyInlayHintEnabled — every gate off yields
// no request (Go returns `null`).
#[test]
fn is_any_inlay_hint_enabled_false_for_default() {
    assert!(!is_any_inlay_hint_enabled(&InlayHintsPreferences::default()));
}

// Each individual gate flips `isAnyInlayHintEnabled` to true.
// Go: internal/ls/inlay_hints.go:isAnyInlayHintEnabled
#[test]
fn is_any_inlay_hint_enabled_true_per_gate() {
    let all = InlayHintsPreferences {
        include_inlay_parameter_name_hints: IncludeInlayParameterNameHints::All,
        ..Default::default()
    };
    assert!(is_any_inlay_hint_enabled(&all));

    let literals = InlayHintsPreferences {
        include_inlay_parameter_name_hints: IncludeInlayParameterNameHints::Literals,
        ..Default::default()
    };
    assert!(is_any_inlay_hint_enabled(&literals));

    for prefs in [
        InlayHintsPreferences {
            include_inlay_function_parameter_type_hints: Tristate::True,
            ..Default::default()
        },
        InlayHintsPreferences {
            include_inlay_variable_type_hints: Tristate::True,
            ..Default::default()
        },
        InlayHintsPreferences {
            include_inlay_property_declaration_type_hints: Tristate::True,
            ..Default::default()
        },
        InlayHintsPreferences {
            include_inlay_function_like_return_type_hints: Tristate::True,
            ..Default::default()
        },
        InlayHintsPreferences {
            include_inlay_enum_member_value_hints: Tristate::True,
            ..Default::default()
        },
    ] {
        assert!(is_any_inlay_hint_enabled(&prefs));
    }
}

// A `None` parameter-name preference does not by itself enable hints (only the
// other suppression toggle being on does not count).
// Go: internal/ls/inlay_hints.go:isAnyInlayHintEnabled
#[test]
fn is_any_inlay_hint_enabled_ignores_suppression_only_toggles() {
    let prefs = InlayHintsPreferences {
        include_inlay_parameter_name_hints_when_argument_matches_name: Tristate::True,
        include_inlay_variable_type_hints_when_type_matches_name: Tristate::True,
        ..Default::default()
    };
    assert!(!is_any_inlay_hint_enabled(&prefs));
}

// GUARD: with every gate off, the provider returns `null` (None), not an empty
// array.
// Go: internal/ls/inlay_hints.go:ProvideInlayHint (isAnyInlayHintEnabled guard)
#[test]
fn disabled_preferences_yield_null() {
    let mut ls = build_service(&[("/m.ts", "enum E { A }")], "/", &["/m.ts"]);
    assert!(ls
        .provide_inlay_hints("/m.ts", whole_file(), &InlayHintsPreferences::default())
        .is_none());
}

// GUARD: an unknown file yields `null` (no panic).
// Go: internal/ls/languageservice.go:getProgramAndFile (missing file)
#[test]
fn unknown_file_yields_null() {
    let mut ls = build_service(&[("/m.ts", "enum E { A }")], "/", &["/m.ts"]);
    assert!(ls
        .provide_inlay_hints("/missing.ts", whole_file(), &enum_member_prefs())
        .is_none());
}
