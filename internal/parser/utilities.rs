//! Free helper functions shared across the parser (`get_language_variant`,
//! token classification).

use tsgo_ast::utilities::{is_keyword_kind, is_punctuation_kind};
use tsgo_ast::Kind;
use tsgo_core::languagevariant::LanguageVariant;
use tsgo_core::scriptkind::ScriptKind;

/// Returns the language variant implied by a script kind. `.tsx`, `.jsx`,
/// `.js`, and `.json` are parsed with the JSX variant; everything else uses the
/// standard variant.
///
/// # Examples
/// ```
/// use tsgo_parser::utilities::get_language_variant;
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_core::languagevariant::LanguageVariant;
/// assert_eq!(get_language_variant(ScriptKind::Tsx), LanguageVariant::Jsx);
/// assert_eq!(get_language_variant(ScriptKind::Ts), LanguageVariant::Standard);
/// ```
///
/// Side effects: none (pure).
// Go: internal/parser/utilities.go:getLanguageVariant
pub fn get_language_variant(script_kind: ScriptKind) -> LanguageVariant {
    match script_kind {
        ScriptKind::Tsx | ScriptKind::Jsx | ScriptKind::Js | ScriptKind::Json => {
            LanguageVariant::Jsx
        }
        _ => LanguageVariant::Standard,
    }
}

/// Reports whether `token` is an identifier or any keyword (keywords sort at or
/// after [`Kind::Identifier`]).
///
/// Side effects: none (pure).
// Go: internal/parser/utilities.go:tokenIsIdentifierOrKeyword
pub fn token_is_identifier_or_keyword(token: Kind) -> bool {
    token >= Kind::Identifier
}

/// Reports whether `token` is `>` or an identifier/keyword (used by JSX
/// generic lookahead).
///
/// Side effects: none (pure).
// Go: internal/parser/utilities.go:tokenIsIdentifierOrKeywordOrGreaterThan
pub fn token_is_identifier_or_keyword_or_greater_than(token: Kind) -> bool {
    token == Kind::GreaterThanToken || token_is_identifier_or_keyword(token)
}

/// Reports whether `token` is a keyword or punctuation kind.
///
/// Side effects: none (pure).
// Go: internal/parser/utilities.go:isKeywordOrPunctuation
pub fn is_keyword_or_punctuation(token: Kind) -> bool {
    is_keyword_kind(token) || is_punctuation_kind(token)
}

/// Reports whether `text` looks like a JSDoc comment (`/**` but not `/**/`).
///
/// Side effects: none (pure).
// Go: internal/parser/utilities.go:isJSDocLikeText
pub fn is_jsdoc_like_text(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 4 && bytes[1] == b'*' && bytes[2] == b'*' && bytes[3] != b'/'
}

#[cfg(test)]
#[path = "utilities_test.rs"]
mod tests;
