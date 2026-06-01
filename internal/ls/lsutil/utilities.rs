//! Misc syntactic helpers (reachable subset of Go `utilities.go`).
//!
//! Ports the keyword / identifier / quote helpers from
//! `internal/ls/lsutil/utilities.go`. The program- and preference-dependent
//! functions in that file (`ShouldUseUriStyleNodeCoreModules`,
//! `GetQuotePreference`, `ProbablyUsesSemicolons`) are deferred (see crate docs).

use tsgo_ast::utilities::is_keyword_kind;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, Symbol, TokenFlags};
use tsgo_scanner::{is_identifier_part, is_identifier_start, string_to_token};
use tsgo_stringutil::strip_quotes;
use tsgo_tspath::{get_base_file_name, remove_file_extension};

use crate::QuotePreference;

/// Reports whether `token` is a reserved (non-contextual) keyword.
///
/// A keyword that is *not* contextual: contextual keywords (e.g. `as`, `of`,
/// `get`) can be used as identifiers, so they are excluded.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::is_non_contextual_keyword;
/// use tsgo_ast::Kind;
/// assert!(is_non_contextual_keyword(Kind::IfKeyword));
/// assert!(!is_non_contextual_keyword(Kind::AsKeyword)); // contextual
/// assert!(!is_non_contextual_keyword(Kind::Identifier)); // not a keyword
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/utilities.go:IsNonContextualKeyword
pub fn is_non_contextual_keyword(token: Kind) -> bool {
    is_keyword_kind(token) && !is_contextual_keyword(token)
}

/// Reports whether `token` is a contextual keyword.
///
/// `tsgo_ast` has not ported `ast.IsContextualKeyword`, so this mirrors it
/// locally using the generated `Kind` range constants.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsContextualKeyword
fn is_contextual_keyword(token: Kind) -> bool {
    Kind::FIRST_CONTEXTUAL_KEYWORD <= token && token <= Kind::LAST_CONTEXTUAL_KEYWORD
}

/// Go-compatible simple uppercase of a single code point.
///
/// Go `unicode.ToUpper` returns one rune (simple case mapping). Rust's
/// `char::to_uppercase` performs full case mapping (which can yield multiple
/// chars, e.g. `ß` -> `SS`); for identifier characters this never happens, so
/// taking the first mapped char reproduces Go's behavior for all realistic
/// module-name inputs.
///
/// Side effects: none (pure).
// Go: unicode.ToUpper
fn to_upper_simple(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

/// Returns the quote preference implied by a string literal's scanner flags.
///
/// Go takes `*ast.StringLiteral`; this port takes the owning `arena` and the
/// node id of a `StringLiteral`. A single-quoted literal yields
/// [`QuotePreference::Single`], anything else [`QuotePreference::Double`].
///
/// # Panics
/// Panics if `id` is not a `StringLiteral` node.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::{quote_preference_from_string, QuotePreference};
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_ast::{Kind, NodeData};
/// let r = parse_source_file(SourceFileParseOptions::default(), "const x = 'a';", ScriptKind::Ts);
/// // Find the string literal node.
/// let mut lit = None;
/// r.arena.for_each_child(r.source_file, &mut |c| { collect(&r.arena, c, &mut lit); false });
/// fn collect(a: &tsgo_ast::NodeArena, id: tsgo_ast::NodeId, out: &mut Option<tsgo_ast::NodeId>) {
///     if a.kind(id) == Kind::StringLiteral { *out = Some(id); return; }
///     a.for_each_child(id, &mut |c| { collect(a, c, out); false });
/// }
/// assert_eq!(quote_preference_from_string(&r.arena, lit.unwrap()), QuotePreference::Single);
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/utilities.go:QuotePreferenceFromString
pub fn quote_preference_from_string(arena: &NodeArena, id: NodeId) -> QuotePreference {
    let token_flags = match arena.data(id) {
        NodeData::StringLiteral(d) => d.token_flags,
        _ => panic!("quote_preference_from_string expects a StringLiteral node"),
    };
    if token_flags.contains(TokenFlags::SINGLE_QUOTE) {
        QuotePreference::Single
    } else {
        QuotePreference::Double
    }
}

/// Converts a module symbol's (quote-stripped) name into a valid identifier.
///
/// # Examples
/// ```
/// // Exercised via `module_specifier_to_valid_identifier`; see that function's
/// // tests. This thin wrapper strips quotes off the symbol name first.
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/utilities.go:ModuleSymbolToValidIdentifier
pub fn module_symbol_to_valid_identifier(module_symbol: &Symbol, force_capitalize: bool) -> String {
    module_specifier_to_valid_identifier(strip_quotes(&module_symbol.name), force_capitalize)
}

/// Converts a module specifier into a valid identifier name.
///
/// Mirrors TS `moduleSpecifierToValidIdentifier`: take the base file name (minus
/// extension and trailing `/index`), keep identifier-valid characters,
/// camel-case across dropped invalid characters, prefix `_` if the result is
/// empty or collides with a reserved keyword.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::module_specifier_to_valid_identifier;
/// assert_eq!(module_specifier_to_valid_identifier("./foo-bar", false), "fooBar");
/// assert_eq!(module_specifier_to_valid_identifier("./foo-bar", true), "FooBar");
/// assert_eq!(module_specifier_to_valid_identifier("./foo/index.ts", false), "foo");
/// assert_eq!(module_specifier_to_valid_identifier("./if", false), "_if"); // keyword
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/utilities.go:ModuleSpecifierToValidIdentifier
pub fn module_specifier_to_valid_identifier(
    module_specifier: &str,
    force_capitalize: bool,
) -> String {
    let without_ext = remove_file_extension(module_specifier);
    let without_index = without_ext.strip_suffix("/index").unwrap_or(without_ext);
    let base_name = get_base_file_name(without_index);
    let base_name_runes: Vec<char> = base_name.chars().collect();

    let mut res: Vec<char> = Vec::new();
    let mut last_char_was_valid = true;
    if let Some(&first) = base_name_runes.first() {
        if is_identifier_start(first as i32) {
            if force_capitalize {
                res.push(to_upper_simple(first));
            } else {
                res.push(first);
            }
        } else {
            last_char_was_valid = false;
        }
    } else {
        last_char_was_valid = false;
    }

    for &ch in base_name_runes.iter().skip(1) {
        let is_valid = is_identifier_part(ch as i32);
        if is_valid {
            if !last_char_was_valid {
                res.push(to_upper_simple(ch));
            } else {
                res.push(ch);
            }
        }
        last_char_was_valid = is_valid;
    }

    // Need `"_"` to ensure result isn't empty.
    let res_string: String = res.into_iter().collect();
    if !res_string.is_empty() && !is_non_contextual_keyword(string_to_token(&res_string)) {
        return res_string;
    }
    format!("_{res_string}")
}

#[cfg(test)]
#[path = "utilities_test.rs"]
mod tests;
