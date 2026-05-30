use super::*;

// Go: internal/parser/utilities.go:getLanguageVariant
#[test]
fn language_variant_jsx_kinds() {
    assert_eq!(get_language_variant(ScriptKind::Tsx), LanguageVariant::Jsx);
    assert_eq!(get_language_variant(ScriptKind::Jsx), LanguageVariant::Jsx);
    assert_eq!(get_language_variant(ScriptKind::Js), LanguageVariant::Jsx);
    assert_eq!(get_language_variant(ScriptKind::Json), LanguageVariant::Jsx);
}

// Go: internal/parser/utilities.go:getLanguageVariant
#[test]
fn language_variant_standard_kinds() {
    assert_eq!(
        get_language_variant(ScriptKind::Ts),
        LanguageVariant::Standard
    );
    assert_eq!(get_language_variant(ScriptKind::Tsx), LanguageVariant::Jsx);
}

// Go: internal/parser/utilities.go:tokenIsIdentifierOrKeyword
#[test]
fn identifier_or_keyword_classification() {
    assert!(token_is_identifier_or_keyword(Kind::Identifier));
    assert!(token_is_identifier_or_keyword(Kind::IfKeyword));
    assert!(!token_is_identifier_or_keyword(Kind::PlusToken));
}

// Go: internal/parser/utilities.go:tokenIsIdentifierOrKeywordOrGreaterThan
#[test]
fn identifier_or_keyword_or_greater_than() {
    assert!(token_is_identifier_or_keyword_or_greater_than(
        Kind::GreaterThanToken
    ));
    assert!(token_is_identifier_or_keyword_or_greater_than(
        Kind::Identifier
    ));
    assert!(!token_is_identifier_or_keyword_or_greater_than(
        Kind::PlusToken
    ));
}

// Go: internal/parser/utilities.go:isJSDocLikeText
#[test]
fn jsdoc_like_text() {
    assert!(is_jsdoc_like_text("/** doc */"));
    assert!(!is_jsdoc_like_text("/**/"));
    assert!(!is_jsdoc_like_text("/* x */"));
    assert!(!is_jsdoc_like_text("/*"));
}
