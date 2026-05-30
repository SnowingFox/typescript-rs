//! Scanner utility helpers ported 1:1 from Go `internal/scanner/utilities.go`.
//!
//! Surrogate-pair arithmetic, the CESU-8 sentinel codec used by the (deferred)
//! regular-expression validator, the identifier/keyword token classifier, and
//! the source-text identifier predicates.
//!
//! The node-based helpers (`IdentifierToKeywordKind`, `GetTextOfNode*`,
//! `DeclarationNameToString`) are deferred until `ast.SourceFile`/`ast.Node`
//! land; see the package `impl.md`.

use tsgo_core::languagevariant::LanguageVariant;

use crate::{is_identifier_part_ex, is_identifier_start};

/// Start of the high-surrogate range.
// Go: internal/scanner/utilities.go:surr1
const SURR1: i32 = 0xd800;
/// Start of the low-surrogate range (= end of high surrogates).
// Go: internal/scanner/utilities.go:surr2
const SURR2: i32 = 0xdc00;
/// End of the surrogate range.
// Go: internal/scanner/utilities.go:surr3
const SURR3: i32 = 0xe000;
/// First code point above the BMP.
// Go: internal/scanner/utilities.go:surrSelf
const SURR_SELF: i32 = 0x10000;

/// Reports whether `r` is a high (leading) surrogate code unit.
// Go: internal/scanner/utilities.go:codePointIsHighSurrogate
pub(crate) fn code_point_is_high_surrogate(r: i32) -> bool {
    (SURR1..SURR2).contains(&r)
}

/// Reports whether `r` is a low (trailing) surrogate code unit.
// Go: internal/scanner/utilities.go:codePointIsLowSurrogate
pub(crate) fn code_point_is_low_surrogate(r: i32) -> bool {
    (SURR2..SURR3).contains(&r)
}

/// Combines a high/low surrogate pair into a single code point.
// Go: internal/scanner/utilities.go:surrogatePairToCodepoint
pub(crate) fn surrogate_pair_to_codepoint(r1: i32, r2: i32) -> i32 {
    (((r1 - SURR1) << 10) | (r2 - SURR2)) + SURR_SELF
}

/// Encodes a surrogate code unit as a CESU-8 sentinel.
///
/// Divergence: Go returns a `string` holding the raw bytes `0xED 0x.. 0x..`,
/// which is invalid UTF-8 and therefore cannot be a Rust `String`. This sentinel
/// is consumed only by the regular-expression validator (deferred to P10); no
/// string/template scanning path reaches it. Until the validator lands the
/// U+FFFD replacement character stands in.
///
/// TODO(port): represent the CESU-8 sentinel (e.g. as `Vec<u8>`) when porting
/// the regex validator in P10.
// Go: internal/scanner/utilities.go:encodeSurrogate
pub(crate) fn encode_surrogate(_r: i32) -> String {
    "\u{FFFD}".to_string()
}

/// Reports whether `token` is an identifier or any keyword (i.e. would be a
/// valid property/member name).
// Go: internal/scanner/utilities.go:tokenIsIdentifierOrKeyword
pub(crate) fn token_is_identifier_or_keyword(token: tsgo_ast::Kind) -> bool {
    token >= tsgo_ast::Kind::Identifier
}

/// Reports whether `name` is a valid identifier under the given language
/// variant (JSX permits `-`/`:` in the parts).
///
/// # Examples
/// ```
/// use tsgo_scanner::is_identifier_text;
/// use tsgo_core::languagevariant::LanguageVariant;
/// assert!(is_identifier_text("foo", LanguageVariant::Standard));
/// assert!(!is_identifier_text("1foo", LanguageVariant::Standard));
/// assert!(is_identifier_text("foo-bar", LanguageVariant::Jsx));
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/utilities.go:IsIdentifierText
pub fn is_identifier_text(name: &str, language_variant: LanguageVariant) -> bool {
    let mut chars = name.char_indices();
    let first = match chars.next() {
        Some((_, c)) => c,
        None => return false,
    };
    if !is_identifier_start(first as i32) {
        return false;
    }
    for (_, ch) in chars {
        if !is_identifier_part_ex(ch as i32, language_variant) {
            return false;
        }
    }
    true
}

/// Reports whether `name` is an intrinsic JSX element name, i.e. it starts with
/// a lowercase ASCII letter or contains a `-`.
///
/// # Examples
/// ```
/// use tsgo_scanner::is_intrinsic_jsx_name;
/// assert!(is_intrinsic_jsx_name("div"));
/// assert!(is_intrinsic_jsx_name("my-element"));
/// assert!(!is_intrinsic_jsx_name("Component"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/utilities.go:IsIntrinsicJsxName
pub fn is_intrinsic_jsx_name(name: &str) -> bool {
    let b = name.as_bytes();
    !name.is_empty() && ((b[0] >= b'a' && b[0] <= b'z') || name.contains('-'))
}

#[cfg(test)]
#[path = "utilities_test.rs"]
mod tests;
