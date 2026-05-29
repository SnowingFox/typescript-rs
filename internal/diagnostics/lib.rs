//! `tsgo_diagnostics` — 1:1 Rust port of Go `internal/diagnostics`.
//!
//! Holds the full set of localizable diagnostic messages and the runtime that
//! looks up per-locale translations and substitutes `{0}`/`{1}` placeholders.
//! It is the single source of truth for every compiler/language-service error
//! string (e.g. `error TS1005: ')' expected.`); downstream stages reference the
//! generated message singletons rather than building strings by hand.

use std::fmt;

use tsgo_locale::Locale;

mod diagnostics_generated;
mod loc;
pub use diagnostics_generated::*;

/// Severity/kind of a diagnostic message.
///
/// Mirrors Go's `type Category int32` with `iota` constants, so the numeric
/// values are stable (`Warning = 0` … `Message = 3`) and match the ordering
/// the Go generator relies on.
///
/// # Examples
/// ```
/// use tsgo_diagnostics::Category;
/// assert_eq!(Category::Error.name(), "error");
/// assert_eq!(Category::Warning as i32, 0);
/// ```
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    /// A warning (`CategoryWarning`, value `0`).
    Warning,
    /// An error (`CategoryError`, value `1`).
    Error,
    /// A suggestion (`CategorySuggestion`, value `2`).
    Suggestion,
    /// An informational message (`CategoryMessage`, value `3`).
    Message,
}

impl Category {
    /// Returns the lowercase category name used in formatted diagnostics
    /// (`"warning"`, `"error"`, `"suggestion"`, `"message"`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_diagnostics::Category;
    /// assert_eq!(Category::Suggestion.name(), "suggestion");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/diagnostics/diagnostics.go:Category.Name
    pub fn name(&self) -> &'static str {
        match self {
            Category::Warning => "warning",
            Category::Error => "error",
            Category::Suggestion => "suggestion",
            Category::Message => "message",
        }
    }
}

impl fmt::Display for Category {
    // Go: internal/diagnostics/stringer_generated.go:Category.String (debug only)
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Category::Warning => "CategoryWarning",
            Category::Error => "CategoryError",
            Category::Suggestion => "CategorySuggestion",
            Category::Message => "CategoryMessage",
        };
        f.write_str(s)
    }
}

/// Stable identifier for a diagnostic message (e.g. `"Identifier_expected_1003"`).
///
/// Produced by the Go generator's `convertPropertyName`; it is the join key
/// between [`Message`], `key_to_message`, and the per-locale translation
/// tables, so the string value is byte-exact with Go.
///
/// Mirrors Go's `type Key string`. The generated singletons are all `'static`,
/// so a `&'static str` is sufficient.
// Go: internal/diagnostics/diagnostics.go:Key
pub type Key = &'static str;

/// A single immutable diagnostic message: its numeric code, category, lookup
/// key, default (English) text, and a few classification flags.
///
/// Mirrors Go's `type Message struct{...}`. All fields are private (as in Go)
/// and exposed through getters. Every instance in the generated table is a
/// compile-time `static`, so there is no runtime allocation.
///
/// # Examples
/// ```
/// use tsgo_diagnostics::{Category, IDENTIFIER_EXPECTED};
/// assert_eq!(IDENTIFIER_EXPECTED.code(), 1003);
/// assert_eq!(IDENTIFIER_EXPECTED.category(), Category::Error);
/// ```
#[derive(Debug)]
pub struct Message {
    code: i32,
    category: Category,
    key: Key,
    text: &'static str,
    reports_unnecessary: bool,
    elided_in_compatibility_pyramid: bool,
    reports_deprecated: bool,
}

impl Message {
    /// Returns the numeric diagnostic code (e.g. `1003` for `TS1003`).
    ///
    /// Side effects: none (pure).
    // Go: internal/diagnostics/diagnostics.go:Message.Code
    pub fn code(&self) -> i32 {
        self.code
    }

    /// Returns the message [`Category`].
    ///
    /// Side effects: none (pure).
    // Go: internal/diagnostics/diagnostics.go:Message.Category
    pub fn category(&self) -> Category {
        self.category
    }

    /// Returns the stable lookup [`Key`].
    ///
    /// Side effects: none (pure).
    // Go: internal/diagnostics/diagnostics.go:Message.Key
    pub fn key(&self) -> Key {
        self.key
    }

    /// Reports whether the diagnostic marks code as unnecessary (e.g. unused).
    ///
    /// Side effects: none (pure).
    // Go: internal/diagnostics/diagnostics.go:Message.ReportsUnnecessary
    pub fn reports_unnecessary(&self) -> bool {
        self.reports_unnecessary
    }

    /// Reports whether the diagnostic is elided in the compatibility pyramid.
    ///
    /// Side effects: none (pure).
    // Go: internal/diagnostics/diagnostics.go:Message.ElidedInCompatibilityPyramid
    pub fn elided_in_compatibility_pyramid(&self) -> bool {
        self.elided_in_compatibility_pyramid
    }

    /// Reports whether the diagnostic marks code as deprecated.
    ///
    /// Side effects: none (pure).
    // Go: internal/diagnostics/diagnostics.go:Message.ReportsDeprecated
    pub fn reports_deprecated(&self) -> bool {
        self.reports_deprecated
    }

    /// Localizes this message for `locale` and substitutes `args` into its
    /// placeholders.
    ///
    /// Delegates to the free [`localize`] function with this message and an
    /// empty key. Mirrors Go's `(*Message).Localize`, which first runs the
    /// arguments through `StringifyArgs`; here callers pass already-stringified
    /// `&str` arguments (see [`stringify_args`]).
    ///
    /// # Examples
    /// ```
    /// use tsgo_diagnostics::X_0_EXPECTED;
    /// let en = tsgo_locale::parse("en").unwrap();
    /// assert_eq!(X_0_EXPECTED.localize(&en, &[")"]), "')' expected.");
    /// ```
    ///
    /// Side effects: may lazily decompress + cache the locale's translation
    /// table on first use for that language (no other observable effects).
    // Go: internal/diagnostics/diagnostics.go:Message.Localize
    pub fn localize(&'static self, locale: &Locale, args: &[&str]) -> String {
        localize(locale, Some(self), "", args)
    }
}

impl fmt::Display for Message {
    // Go: internal/diagnostics/diagnostics.go:Message.String (for debugging only)
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text)
    }
}

/// A format argument for a diagnostic placeholder.
///
/// Go accepts `...any` and renders each value with `fmt.Sprintf("%v", arg)`
/// (strings are used verbatim). In practice diagnostics only ever pass strings
/// and integers, so this port models them with an explicit enum instead of a
/// `Box<dyn Any>`; see the divergence note in `impl.md`.
#[derive(Debug, Clone, Copy)]
pub enum Arg<'a> {
    /// A string argument, used verbatim.
    Str(&'a str),
    /// An integer argument, rendered with `Display`.
    Int(i64),
}

/// Converts typed [`Arg`] values into their string forms, ready to feed into
/// `format`.
///
/// Strings are taken verbatim; other values are rendered with `Display`
/// (mirroring Go's `fmt.Sprintf("%v", arg)`). An empty input yields an empty
/// vector (Go returns `nil`).
///
/// # Examples
/// ```
/// use tsgo_diagnostics::{stringify_args, Arg};
/// assert_eq!(stringify_args(&[Arg::Str("x"), Arg::Int(42)]), vec!["x", "42"]);
/// ```
///
/// Side effects: none (pure).
// Go: internal/diagnostics/diagnostics.go:StringifyArgs
pub fn stringify_args(args: &[Arg]) -> Vec<String> {
    args.iter()
        .map(|arg| match arg {
            Arg::Str(s) => (*s).to_string(),
            Arg::Int(n) => n.to_string(),
        })
        .collect()
}

/// Resolves a message (by value or by [`Key`]), applies the `locale`
/// translation if one exists, and substitutes `args` into the placeholders.
///
/// When `message` is `None` the message is looked up by `key`; if that also
/// fails the function panics with `"Unknown diagnostic message: <key>"`,
/// matching Go. The default English `text` is used unless the resolved
/// `locale` provides a translation for the message's key.
///
/// # Examples
/// ```
/// use tsgo_diagnostics::localize;
/// let en = tsgo_locale::parse("en").unwrap();
/// assert_eq!(
///     localize(&en, None, "Identifier_expected_1003", &[]),
///     "Identifier expected."
/// );
/// ```
///
/// Side effects: may lazily decompress + cache a language's translation table
/// on first use (no other observable effects).
// Go: internal/diagnostics/diagnostics.go:Localize
pub fn localize(
    locale: &Locale,
    message: Option<&'static Message>,
    key: &str,
    args: &[&str],
) -> String {
    let message = message
        .or_else(|| diagnostics_generated::key_to_message(key))
        .unwrap_or_else(|| panic!("Unknown diagnostic message: {key}"));

    let mut text: &str = message.text;
    if let Some(localized) = get_localized_messages(locale).and_then(|m| m.get(message.key)) {
        text = localized;
    }
    format(text, args)
}

/// Returns the translation table for `locale`, or `None` when English (or no
/// supported language) applies so the default `text` is used.
///
/// Mirrors Go's `getLocalizedMessages`: the undefined tag yields `None`
/// immediately; otherwise the locale is matched against [`loc::MATCHER_TAGS`]
/// and, on a hit with a translation, the language's table is lazily loaded.
///
/// Side effects: may lazily decompress + cache a language's table on first use.
// Go: internal/diagnostics/diagnostics.go:getLocalizedMessages
fn get_localized_messages(locale: &Locale) -> Option<&'static loc::LocaleMessages> {
    let tag = locale.to_string();
    if tag == "und" {
        return None;
    }
    let index = match_locale(&tag)?;
    loc::LOCALE_FUNCS.get(index).copied().flatten().map(|f| f())
}

/// Returns the index into [`loc::MATCHER_TAGS`] that best matches `tag`.
///
/// Simplified stand-in for Go's CLDR language-distance matcher
/// (`golang.org/x/text/language`): an exact (case-insensitive) tag match is
/// preferred, then a base-language match (e.g. `de` -> `de-DE`). No match
/// yields `None` (English fallback).
///
/// Side effects: none (pure).
// TODO(port): full CLDR language-distance matching; blocked-by: choosing icu_locid/oxilangtag.
fn match_locale(tag: &str) -> Option<usize> {
    if let Some(i) = loc::MATCHER_TAGS
        .iter()
        .position(|t| t.eq_ignore_ascii_case(tag))
    {
        return Some(i);
    }
    let base = tag.split('-').next().unwrap_or(tag);
    loc::MATCHER_TAGS
        .iter()
        .position(|t| t.split('-').next().unwrap_or(t).eq_ignore_ascii_case(base))
}

/// Substitutes `{0}`, `{1}`, … placeholders in `text` with the corresponding
/// entry in `args`.
///
/// When `args` is empty the text is returned verbatim (matching Go's early
/// return). Each `{n}` token (one or more ASCII digits between braces) is
/// replaced by `args[n]`; an out-of-range index panics with
/// `"Invalid formatting placeholder"`, mirroring Go. Tokens like `{}` or
/// `{abc}` are left untouched (Go's regexp is `{(\d+)}`).
///
/// # Examples
/// ```
/// assert_eq!(tsgo_diagnostics::format("'{0}' expected.", &[")"]), "')' expected.");
/// assert_eq!(tsgo_diagnostics::format("a{0}b", &[]), "a{0}b");
/// ```
///
/// Side effects: none (pure).
// Go: internal/diagnostics/diagnostics.go:Format
pub fn format(text: &str, args: &[&str]) -> String {
    if args.is_empty() {
        return text.to_string();
    }

    // Go sanitizes args with `strings.ToValidUTF8(arg, "\u{FFFD}")`. Rust
    // `&str` is always valid UTF-8, so this step is a no-op and is omitted.
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let (before, from_brace) = rest.split_at(open);
        out.push_str(before);
        // `from_brace` starts with '{'. Find the run of ASCII digits after it.
        let digits_end = from_brace[1..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|p| p + 1)
            .unwrap_or(from_brace.len());
        if digits_end > 1 && from_brace.as_bytes().get(digits_end) == Some(&b'}') {
            let index: usize = from_brace[1..digits_end]
                .parse()
                .expect("Invalid formatting placeholder");
            if index >= args.len() {
                panic!("Invalid formatting placeholder");
            }
            out.push_str(args[index]);
            rest = &from_brace[digits_end + 1..];
        } else {
            out.push('{');
            rest = &from_brace[1..];
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
