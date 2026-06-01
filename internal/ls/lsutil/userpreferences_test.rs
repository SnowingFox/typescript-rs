use super::*;

// Go: internal/ls/lsutil/userpreferences.go:QuotePreference constants
#[test]
fn quote_preference_wire_values() {
    assert_eq!(QuotePreference::Unknown.as_str(), "");
    assert_eq!(QuotePreference::Auto.as_str(), "auto");
    assert_eq!(QuotePreference::Double.as_str(), "double");
    assert_eq!(QuotePreference::Single.as_str(), "single");
}

// Go: internal/ls/lsutil/userpreferences.go:QuotePreference (zero value is Unknown)
#[test]
fn quote_preference_default_is_unknown() {
    assert_eq!(QuotePreference::default(), QuotePreference::Unknown);
}
