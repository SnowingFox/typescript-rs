//! Behavior tests for `utilities.rs`. Go ships no `*_test.go` for the scanner;
//! these cover the surrogate codec and identifier predicates via the public and
//! crate-internal API, with `// Go:` anchors to the implementation items.

use super::*;

// Go: internal/scanner/utilities.go:codePointIsHighSurrogate / codePointIsLowSurrogate
#[test]
fn surrogate_classification() {
    assert!(code_point_is_high_surrogate(0xD800));
    assert!(code_point_is_high_surrogate(0xDBFF));
    assert!(!code_point_is_high_surrogate(0xDC00));
    assert!(code_point_is_low_surrogate(0xDC00));
    assert!(code_point_is_low_surrogate(0xDFFF));
    assert!(!code_point_is_low_surrogate(0xD800));
}

// Go: internal/scanner/utilities.go:surrogatePairToCodepoint
#[test]
fn surrogate_pair_combination() {
    // U+1F600 (emoji) = high D83D, low DE00.
    assert_eq!(surrogate_pair_to_codepoint(0xD83D, 0xDE00), 0x1F600);
    // U+10000 = high D800, low DC00.
    assert_eq!(surrogate_pair_to_codepoint(0xD800, 0xDC00), 0x10000);
}

// Go: internal/scanner/utilities.go:tokenIsIdentifierOrKeyword
#[test]
fn identifier_or_keyword_classification() {
    use tsgo_ast::Kind;
    assert!(token_is_identifier_or_keyword(Kind::Identifier));
    assert!(token_is_identifier_or_keyword(Kind::LetKeyword));
    assert!(!token_is_identifier_or_keyword(Kind::PlusToken));
}

// Go: internal/scanner/utilities.go:IsIdentifierText
#[test]
fn identifier_text_predicate() {
    use tsgo_core::languagevariant::LanguageVariant;
    assert!(is_identifier_text("foo", LanguageVariant::Standard));
    assert!(is_identifier_text("_foo$1", LanguageVariant::Standard));
    assert!(!is_identifier_text("1foo", LanguageVariant::Standard));
    assert!(!is_identifier_text("", LanguageVariant::Standard));
    assert!(!is_identifier_text("foo-bar", LanguageVariant::Standard));
    assert!(is_identifier_text("foo-bar", LanguageVariant::Jsx));
}

// Go: internal/scanner/utilities.go:IsIntrinsicJsxName
#[test]
fn intrinsic_jsx_name_predicate() {
    assert!(is_intrinsic_jsx_name("div"));
    assert!(is_intrinsic_jsx_name("my-element"));
    assert!(is_intrinsic_jsx_name("Foo-Bar")); // contains '-'
    assert!(!is_intrinsic_jsx_name("Component"));
    assert!(!is_intrinsic_jsx_name(""));
}
