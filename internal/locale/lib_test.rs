use super::*;

// Go side has no `*_test.go`; behavior is covered by P10 parity. The cases
// below are behavior-level checks through the public API, with expected values
// derived from BCP-47 known tags and Go's lenient `Parse` failure semantics.

// Go: internal/locale/locale.go:Parse — valid simple tag
#[test]
fn parse_valid_simple() {
    assert!(parse("en").is_some());
}

// Go: internal/locale/locale.go:Parse — language-region tags
#[test]
fn parse_valid_region() {
    assert!(parse("zh-CN").is_some());
    assert!(parse("ja").is_some());
}

// Go: internal/locale/locale.go:Parse — invalid tag fails gracefully (ok=false)
#[test]
fn parse_invalid_returns_none() {
    assert!(parse("not a locale!!").is_none());
}

// Go: internal/locale/locale.go:Parse — empty string
//
// `unic-langid` rejects an empty tag (returns `None`), matching Go's lenient
// `ok == false`. Note `"und"` parses fine to the default; only the empty
// string fails. Exact x/text-vs-unic-langid leniency is deferred to P10 parity.
#[test]
fn parse_empty_returns_none_or_default() {
    assert!(parse("").is_none());
}

// Go: internal/locale/locale.go:Default — zero value is the undefined tag
#[test]
fn default_is_zero_value() {
    assert_eq!(Some(Locale::default()), parse("und"));
}
