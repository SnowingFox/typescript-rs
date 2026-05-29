use super::*;

// Go: internal/ast/tokenflags.go (base bit positions)
#[test]
fn token_flags_base_bits() {
    assert_eq!(TokenFlags::NONE.bits(), 0);
    assert_eq!(TokenFlags::PRECEDING_LINE_BREAK.bits(), 1 << 0);
    assert_eq!(TokenFlags::UNTERMINATED.bits(), 1 << 2);
    assert_eq!(TokenFlags::HEX_SPECIFIER.bits(), 1 << 6);
    assert_eq!(TokenFlags::BINARY_SPECIFIER.bits(), 1 << 7);
    assert_eq!(TokenFlags::OCTAL_SPECIFIER.bits(), 1 << 8);
    assert_eq!(TokenFlags::SINGLE_QUOTE.bits(), 1 << 16);
    assert_eq!(TokenFlags::PRECEDING_JSDOC_WITH_SEE_OR_LINK.bits(), 1 << 18);
}

// Go: internal/ast/tokenflags.go (unions — values from Go literals)
#[test]
fn token_flags_unions() {
    assert_eq!(
        TokenFlags::BINARY_OR_OCTAL_SPECIFIER,
        TokenFlags::BINARY_SPECIFIER | TokenFlags::OCTAL_SPECIFIER
    );
    assert_eq!(
        TokenFlags::WITH_SPECIFIER,
        TokenFlags::HEX_SPECIFIER | TokenFlags::BINARY_OR_OCTAL_SPECIFIER
    );
    assert_eq!(
        TokenFlags::IS_INVALID,
        TokenFlags::OCTAL
            | TokenFlags::CONTAINS_LEADING_ZERO
            | TokenFlags::CONTAINS_INVALID_SEPARATOR
            | TokenFlags::CONTAINS_INVALID_ESCAPE
    );
}
