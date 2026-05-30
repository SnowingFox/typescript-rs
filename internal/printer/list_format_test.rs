use super::*;

// Go: internal/printer/printer.go:ListFormat (precomputed combinations)
#[test]
fn precomputed_formats_have_expected_bits() {
    assert!(ListFormat::SOURCE_FILE_STATEMENTS.contains(ListFormat::MULTI_LINE));
    assert!(ListFormat::SOURCE_FILE_STATEMENTS.contains(ListFormat::NO_TRAILING_NEW_LINE));
    assert!(ListFormat::CLASS_MEMBERS.contains(ListFormat::MULTI_LINE));
    assert!(ListFormat::CLASS_MEMBERS.contains(ListFormat::INDENTED));
    assert_eq!(
        ListFormat::CALL_EXPRESSION_ARGUMENTS & ListFormat::BRACKETS_MASK,
        ListFormat::PARENTHESIS
    );
    assert_eq!(
        ListFormat::ARRAY_LITERAL_EXPRESSION_ELEMENTS & ListFormat::BRACKETS_MASK,
        ListFormat::SQUARE_BRACKETS
    );
    assert!(ListFormat::ARRAY_LITERAL_EXPRESSION_ELEMENTS.contains(ListFormat::PRESERVE_LINES));
}

// Go: internal/printer/printer.go:ListFormat (masks)
#[test]
fn masks_cover_member_bits() {
    assert!(ListFormat::LINES_MASK.contains(ListFormat::MULTI_LINE));
    assert!(ListFormat::LINES_MASK.contains(ListFormat::PRESERVE_LINES));
    assert!(ListFormat::DELIMITERS_MASK.contains(ListFormat::COMMA_DELIMITED));
    assert!(ListFormat::OPTIONAL.contains(ListFormat::OPTIONAL_IF_NIL));
    assert!(ListFormat::OPTIONAL.contains(ListFormat::OPTIONAL_IF_EMPTY));
}
