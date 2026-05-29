use super::*;

// Go: internal/diagnostics/diagnostics.go:Category.Name
#[test]
fn category_name_mapping() {
    assert_eq!(Category::Warning.name(), "warning");
    assert_eq!(Category::Error.name(), "error");
    assert_eq!(Category::Suggestion.name(), "suggestion");
    assert_eq!(Category::Message.name(), "message");
}

// Go: internal/diagnostics/diagnostics.go:Category
#[test]
fn category_repr_values() {
    assert_eq!(Category::Warning as i32, 0);
    assert_eq!(Category::Error as i32, 1);
    assert_eq!(Category::Suggestion as i32, 2);
    assert_eq!(Category::Message as i32, 3);
}

// Go: internal/diagnostics/stringer_generated.go:Category.String
#[test]
fn category_stringer_display() {
    assert_eq!(Category::Warning.to_string(), "CategoryWarning");
    assert_eq!(Category::Error.to_string(), "CategoryError");
    assert_eq!(Category::Suggestion.to_string(), "CategorySuggestion");
    assert_eq!(Category::Message.to_string(), "CategoryMessage");
}

// Go: internal/diagnostics/diagnostics.go:Message getters
#[test]
fn message_getters() {
    assert_eq!(IDENTIFIER_EXPECTED.code(), 1003);
    assert_eq!(IDENTIFIER_EXPECTED.category(), Category::Error);
    assert_eq!(IDENTIFIER_EXPECTED.key(), "Identifier_expected_1003");
    assert!(!IDENTIFIER_EXPECTED.reports_unnecessary());
    assert!(!IDENTIFIER_EXPECTED.elided_in_compatibility_pyramid());
    assert!(!IDENTIFIER_EXPECTED.reports_deprecated());
}

// Go: internal/diagnostics/diagnostics.go:Message.String
#[test]
fn message_display_returns_text() {
    assert_eq!(IDENTIFIER_EXPECTED.to_string(), "Identifier expected.");
}

// Go: internal/diagnostics/diagnostics.go:Format
#[test]
fn format_substitutes_placeholders() {
    assert_eq!(format("'{0}' expected.", &[")"]), "')' expected.");
    assert_eq!(
        format(
            "The parser expected to find a '{1}' to match the '{0}' token here.",
            &["{", "}"]
        ),
        "The parser expected to find a '}' to match the '{' token here."
    );
}

// Go: internal/diagnostics/diagnostics.go:Format
#[test]
fn format_no_args_short_circuit() {
    assert_eq!(format("a{0}b", &[]), "a{0}b");
}

// Go: internal/diagnostics/diagnostics.go:Format
#[test]
#[should_panic(expected = "Invalid formatting placeholder")]
fn format_invalid_placeholder_panics() {
    let _ = format("{1}", &["x"]);
}

// Go: internal/diagnostics/diagnostics.go:Format (strings.ToValidUTF8)
#[test]
fn format_invalid_utf8_args_sanitized() {
    // Go sanitizes each arg via `strings.ToValidUTF8(arg, "\u{FFFD}")` because
    // Go strings may contain invalid UTF-8. Rust `&str` is guaranteed valid
    // UTF-8, so callers decode at the boundary (`String::from_utf8_lossy`),
    // which already yields U+FFFD; this verifies such an arg flows through
    // `format` unchanged.
    let sanitized = String::from_utf8_lossy(&[b'a', 0xFF, b'b']).into_owned();
    assert_eq!(sanitized, "a\u{FFFD}b");
    assert_eq!(format("<{0}>", &[&sanitized]), "<a\u{FFFD}b>");
}

// Go: internal/diagnostics/diagnostics.go:StringifyArgs
#[test]
fn stringify_args_mixed() {
    assert_eq!(
        stringify_args(&[Arg::Str("x"), Arg::Int(42)]),
        vec!["x", "42"]
    );
    assert!(stringify_args(&[]).is_empty());
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/english default
#[test]
fn localize_english_default() {
    let locale = tsgo_locale::parse("en").unwrap();
    assert_eq!(
        IDENTIFIER_EXPECTED.localize(&locale, &[]),
        "Identifier expected."
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/undefined locale uses english
#[test]
fn localize_undefined_locale_uses_english() {
    let locale = Locale::default();
    assert_eq!(
        IDENTIFIER_EXPECTED.localize(&locale, &[]),
        "Identifier expected."
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/with single argument
#[test]
fn localize_with_single_argument() {
    let locale = tsgo_locale::parse("en").unwrap();
    assert_eq!(X_0_EXPECTED.localize(&locale, &[")"]), "')' expected.");
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/with multiple arguments
#[test]
fn localize_with_multiple_arguments() {
    let locale = tsgo_locale::parse("en").unwrap();
    assert_eq!(
        THE_PARSER_EXPECTED_TO_FIND_A_1_TO_MATCH_THE_0_TOKEN_HERE.localize(&locale, &["{", "}"]),
        "The parser expected to find a '}' to match the '{' token here."
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/fallback to english for unknown locale
#[test]
fn localize_fallback_to_english_for_unknown_locale() {
    let locale = tsgo_locale::parse("af-ZA").unwrap();
    assert_eq!(
        IDENTIFIER_EXPECTED.localize(&locale, &[]),
        "Identifier expected."
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/german
#[test]
fn localize_german() {
    let locale = tsgo_locale::parse("de-DE").unwrap();
    assert_eq!(
        IDENTIFIER_EXPECTED.localize(&locale, &[]),
        "Es wurde ein Bezeichner erwartet."
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/french
#[test]
fn localize_french() {
    let locale = tsgo_locale::parse("fr-FR").unwrap();
    assert_eq!(
        IDENTIFIER_EXPECTED.localize(&locale, &[]),
        "Identificateur attendu."
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/spanish
#[test]
fn localize_spanish() {
    let locale = tsgo_locale::parse("es-ES").unwrap();
    assert_eq!(
        IDENTIFIER_EXPECTED.localize(&locale, &[]),
        "Se esperaba un identificador."
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/japanese
#[test]
fn localize_japanese() {
    let locale = tsgo_locale::parse("ja-JP").unwrap();
    assert_eq!(
        IDENTIFIER_EXPECTED.localize(&locale, &[]),
        "識別子が必要です。"
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/chinese simplified
#[test]
fn localize_chinese_simplified() {
    let locale = tsgo_locale::parse("zh-CN").unwrap();
    assert_eq!(IDENTIFIER_EXPECTED.localize(&locale, &[]), "应为标识符。");
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/korean
#[test]
fn localize_korean() {
    let locale = tsgo_locale::parse("ko-KR").unwrap();
    assert_eq!(
        IDENTIFIER_EXPECTED.localize(&locale, &[]),
        "식별자가 필요합니다."
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/russian
#[test]
fn localize_russian() {
    let locale = tsgo_locale::parse("ru-RU").unwrap();
    assert_eq!(
        IDENTIFIER_EXPECTED.localize(&locale, &[]),
        "Ожидался идентификатор."
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize/german with args
#[test]
fn localize_german_with_args() {
    let locale = tsgo_locale::parse("de-DE").unwrap();
    assert_eq!(
        X_0_EXPECTED.localize(&locale, &[")"]),
        "\")\" wurde erwartet."
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize_ByKey/by key without args
#[test]
fn localize_by_key_without_args() {
    let locale = tsgo_locale::parse("en").unwrap();
    assert_eq!(
        localize(&locale, None, "Identifier_expected_1003", &[]),
        "Identifier expected."
    );
}

// Go: internal/diagnostics/diagnostics_test.go:TestLocalize_ByKey/by key with args
#[test]
fn localize_by_key_with_args() {
    let locale = tsgo_locale::parse("en").unwrap();
    assert_eq!(
        localize(&locale, None, "_0_expected_1005", &[")"]),
        "')' expected."
    );
}

// Go: internal/diagnostics/diagnostics.go:Localize (unknown key panics)
#[test]
#[should_panic(expected = "Unknown diagnostic message: nope")]
fn localize_unknown_key_panics() {
    let locale = tsgo_locale::parse("en").unwrap();
    let _ = localize(&locale, None, "nope", &[]);
}
