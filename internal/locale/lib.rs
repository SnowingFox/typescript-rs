//! `tsgo_locale` — 1:1 Rust port of Go `internal/locale`.
//!
//! Represents the UI language (locale) used to localize diagnostics. The Go
//! package wraps `golang.org/x/text/language.Tag`; this port wraps a BCP-47
//! [`unic_langid::LanguageIdentifier`].
//!
//! # Divergence from Go
//! - Go threads the locale through `context.Context` via `WithLocale` /
//!   `FromContext`. Per the porting contract, `context.Context` has no Rust
//!   analogue and is replaced by explicit parameter passing; those two helpers
//!   are intentionally not ported here. Wiring the locale into diagnostics is
//!   deferred to P2.
//! - `golang.org/x/text/language` is replaced by `unic-langid`. Both parse
//!   leniently; per-tag leniency differences are covered by P10 parity.

use std::fmt;
use std::str::FromStr;

use unic_langid::LanguageIdentifier;

/// A UI language tag (BCP-47), used to localize diagnostic messages.
///
/// The [`Default`] value is the "undefined" tag (`und`), mirroring Go's
/// zero-value `language.Tag` (`locale.Default`).
///
/// # Examples
/// ```
/// let en = tsgo_locale::parse("en").unwrap();
/// assert_eq!(en.to_string(), "en");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Locale(LanguageIdentifier);

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Parses a locale string into a [`Locale`], returning [`None`] on failure.
///
/// Mirrors Go's `locale.Parse`, which "gracefully fails": a valid tag yields
/// `Some`, anything unparseable yields `None` (Go's `ok == false`).
///
/// # Examples
/// ```
/// assert!(tsgo_locale::parse("zh-CN").is_some());
/// assert!(tsgo_locale::parse("not a locale!!").is_none());
/// ```
///
/// Side effects: none (pure).
// Go: internal/locale/locale.go:Parse
pub fn parse(locale_str: &str) -> Option<Locale> {
    LanguageIdentifier::from_str(locale_str).ok().map(Locale)
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
