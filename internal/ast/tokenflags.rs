//! `TokenFlags` bit set produced by the scanner for literal tokens.

bitflags::bitflags! {
    /// Scanner-supplied flags describing how a literal token was written
    /// (numeric base, quote style, escape kinds, trivia, ...).
    ///
    /// Mirrors Go `TokenFlags` (an `int32` `iota` enum).
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::tokenflags::TokenFlags;
    /// assert_eq!(
    ///     TokenFlags::WITH_SPECIFIER,
    ///     TokenFlags::HEX_SPECIFIER | TokenFlags::BINARY_SPECIFIER | TokenFlags::OCTAL_SPECIFIER
    /// );
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/ast/tokenflags.go:TokenFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct TokenFlags: i32 {
        /// No flags.
        const NONE = 0;
        /// A line break precedes the token.
        const PRECEDING_LINE_BREAK = 1 << 0;
        /// A JSDoc comment precedes the token.
        const PRECEDING_JSDOC_COMMENT = 1 << 1;
        /// The literal was not terminated.
        const UNTERMINATED = 1 << 2;
        /// Extended unicode escape, e.g. `\u{10ffff}`.
        const EXTENDED_UNICODE_ESCAPE = 1 << 3;
        /// Scientific notation, e.g. `10e2`.
        const SCIENTIFIC = 1 << 4;
        /// Legacy octal, e.g. `0777`.
        const OCTAL = 1 << 5;
        /// Hex specifier, e.g. `0x00`.
        const HEX_SPECIFIER = 1 << 6;
        /// Binary specifier, e.g. `0b0110`.
        const BINARY_SPECIFIER = 1 << 7;
        /// Octal specifier, e.g. `0o777`.
        const OCTAL_SPECIFIER = 1 << 8;
        /// Contains a numeric separator, e.g. `0b1100_0101`.
        const CONTAINS_SEPARATOR = 1 << 9;
        /// Unicode escape, e.g. `\u00a0`.
        const UNICODE_ESCAPE = 1 << 10;
        /// Contains an invalid escape, e.g. `\uhello`.
        const CONTAINS_INVALID_ESCAPE = 1 << 11;
        /// Hex escape, e.g. `\xa0`.
        const HEX_ESCAPE = 1 << 12;
        /// Contains a leading zero, e.g. `0888`.
        const CONTAINS_LEADING_ZERO = 1 << 13;
        /// Contains an invalid separator, e.g. `0_1`.
        const CONTAINS_INVALID_SEPARATOR = 1 << 14;
        /// Preceding JSDoc leading asterisks.
        const PRECEDING_JSDOC_LEADING_ASTERISKS = 1 << 15;
        /// Single-quoted string, e.g. `'abc'`.
        const SINGLE_QUOTE = 1 << 16;
        /// Preceding JSDoc contains `@deprecated`.
        const PRECEDING_JSDOC_WITH_DEPRECATED = 1 << 17;
        /// Preceding JSDoc contains `@see` or `@link`.
        const PRECEDING_JSDOC_WITH_SEE_OR_LINK = 1 << 18;

        /// Binary or octal specifier.
        const BINARY_OR_OCTAL_SPECIFIER = Self::BINARY_SPECIFIER.bits() | Self::OCTAL_SPECIFIER.bits();
        /// Any numeric base specifier.
        const WITH_SPECIFIER = Self::HEX_SPECIFIER.bits() | Self::BINARY_OR_OCTAL_SPECIFIER.bits();
        /// Flags valid on a string literal.
        const STRING_LITERAL_FLAGS = Self::UNTERMINATED.bits()
            | Self::HEX_ESCAPE.bits()
            | Self::UNICODE_ESCAPE.bits()
            | Self::EXTENDED_UNICODE_ESCAPE.bits()
            | Self::CONTAINS_INVALID_ESCAPE.bits()
            | Self::SINGLE_QUOTE.bits();
        /// Flags valid on a numeric literal.
        const NUMERIC_LITERAL_FLAGS = Self::SCIENTIFIC.bits()
            | Self::OCTAL.bits()
            | Self::CONTAINS_LEADING_ZERO.bits()
            | Self::WITH_SPECIFIER.bits()
            | Self::CONTAINS_SEPARATOR.bits()
            | Self::CONTAINS_INVALID_SEPARATOR.bits();
        /// Flags valid on a template literal-like token.
        const TEMPLATE_LITERAL_LIKE_FLAGS = Self::UNTERMINATED.bits()
            | Self::HEX_ESCAPE.bits()
            | Self::UNICODE_ESCAPE.bits()
            | Self::EXTENDED_UNICODE_ESCAPE.bits()
            | Self::CONTAINS_INVALID_ESCAPE.bits();
        /// Flags valid on a regular expression literal.
        const REGULAR_EXPRESSION_LITERAL_FLAGS = Self::UNTERMINATED.bits();
        /// Flags marking the literal as invalid.
        const IS_INVALID = Self::OCTAL.bits()
            | Self::CONTAINS_LEADING_ZERO.bits()
            | Self::CONTAINS_INVALID_SEPARATOR.bits()
            | Self::CONTAINS_INVALID_ESCAPE.bits();
    }
}

#[cfg(test)]
#[path = "tokenflags_test.rs"]
mod tests;
