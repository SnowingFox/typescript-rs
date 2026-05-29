//! Behavior tests for wildcard `Pattern` (Go has no `_test.go`; behavior-level).

use super::*;

// Go: internal/core/pattern.go:TryParsePattern (behavior-level; no Go _test.go)
#[test]
fn try_parse_pattern_handles_exact_star_and_invalid() {
    // Exact match: no `*` -> star_index == -1.
    let exact = try_parse_pattern("lib/foo.ts");
    assert_eq!(exact.star_index, -1);
    assert_eq!(exact.text, "lib/foo.ts");

    // One `*`: star_index points at the `*`.
    let star = try_parse_pattern("a*c");
    assert_eq!(star.star_index, 1);
    assert_eq!(star.text, "a*c");

    // More than one `*`: invalid -> empty default pattern.
    let invalid = try_parse_pattern("a*b*c");
    assert_eq!(invalid, Pattern::default());
}

// Go: internal/core/pattern.go:IsValid (behavior-level; no Go _test.go)
#[test]
fn is_valid_reports_exact_and_in_range_star() {
    assert!(try_parse_pattern("exact").is_valid());
    assert!(try_parse_pattern("a*c").is_valid());
    assert!(!Pattern::default().is_valid());
}

// Go: internal/core/pattern.go:Matches (behavior-level; no Go _test.go)
#[test]
fn matches_exact_and_prefix_suffix() {
    let exact = try_parse_pattern("foo");
    assert!(exact.matches("foo"));
    assert!(!exact.matches("foobar"));

    let star = try_parse_pattern("src/*.ts");
    assert!(star.matches("src/app.ts"));
    assert!(!star.matches("lib/app.ts"));
    assert!(!star.matches("src/app.tsx"));
}

// Go: internal/core/pattern.go:MatchedText (behavior-level; no Go _test.go)
#[test]
fn matched_text_returns_star_substring() {
    let star = try_parse_pattern("src/*.ts");
    assert_eq!(star.matched_text("src/app.ts"), "app");
    // Exact pattern matched text is empty.
    assert_eq!(try_parse_pattern("foo").matched_text("foo"), "");
}

// Go: internal/core/pattern.go:FindBestPatternMatch (behavior-level; no Go _test.go)
#[test]
fn find_best_pattern_match_prefers_longest_fixed_prefix() {
    let patterns = vec![
        try_parse_pattern("*"),     // star_index 0 (shortest prefix)
        try_parse_pattern("src/*"), // star_index 4 (longer prefix)
    ];
    let best = find_best_pattern_match(&patterns, |p| p.clone(), "src/app.ts");
    assert_eq!(best, Some(1));
}
