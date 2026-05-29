use super::*;

// Go side has no `*_test.go`; behavior is covered by P10 conformance/fourslash
// parity. The cases below are behavior-level checks through the public API,
// with expected values derived from `glob.go`'s documented semantics and the
// LSP glob spec. Each anchors to the implementation it pins.

// --- parse / Display round-trips ---

// Go: internal/glob/glob.go:Parse / (*Glob).String
#[test]
fn parse_roundtrip_literal() {
    assert_eq!(parse("foo.ts").unwrap().to_string(), "foo.ts");
}

// Go: internal/glob/glob.go:parse (star/starStar/anyChar) / String
#[test]
fn parse_roundtrip_star_globstar() {
    assert_eq!(parse("**/*.ts").unwrap().to_string(), "**/*.ts");
    assert_eq!(parse("a?c").unwrap().to_string(), "a?c");
}

// Go: internal/glob/glob.go:parse ({) / group.String
#[test]
fn parse_group_roundtrip() {
    assert_eq!(parse("**/*.{ts,js}").unwrap().to_string(), "**/*.{ts,js}");
}

// Go: internal/glob/glob.go:parse ([) / charRange.String
#[test]
fn parse_char_range_roundtrip() {
    assert_eq!(parse("example.[0-9]").unwrap().to_string(), "example.[0-9]");
}

// Go: internal/glob/glob.go:parse (** adjacency check)
#[test]
fn parse_err_double_star_adjacency() {
    assert_eq!(parse("a**b"), Err(GlobError::DoubleStarAdjacency));
    assert_eq!(
        parse("a**b").unwrap_err().to_string(),
        "** may only be adjacent to '/'"
    );
}

// Go: internal/glob/glob.go:parse (unmatched '{')
#[test]
fn parse_err_unmatched_brace() {
    assert_eq!(parse("{a"), Err(GlobError::UnmatchedBrace));
    assert_eq!(parse("{a").unwrap_err().to_string(), "unmatched '{'");
}

// Go: internal/glob/glob.go:parse / errBadRange
#[test]
fn parse_err_bad_range() {
    // Missing '-' separator.
    assert_eq!(parse("[a]"), Err(GlobError::BadRange));
    assert_eq!(
        parse("[a]").unwrap_err().to_string(),
        "'[' patterns must be of the form [x-y]"
    );
}

// --- match semantics ---

// Go: internal/glob/glob.go:match (literal)
#[test]
fn match_literal_exact() {
    let g = parse("foo.ts").unwrap();
    assert!(g.match_input("foo.ts"));
    assert!(!g.match_input("foo.js"));
}

// Go: internal/glob/glob.go:match (star)
#[test]
fn match_star_within_segment() {
    let g = parse("*.ts").unwrap();
    assert!(g.match_input("foo.ts"));
    assert!(!g.match_input("a/foo.ts"));
}

// Go: internal/glob/glob.go:match (anyChar)
#[test]
fn match_anychar() {
    let g = parse("a?c").unwrap();
    assert!(g.match_input("abc"));
    assert!(!g.match_input("a/c"));
}

// Go: internal/glob/glob.go:match (slash) / split
#[test]
fn match_slash_multiple() {
    let g = parse("a/b").unwrap();
    assert!(g.match_input("a/b"));
    assert!(g.match_input("a//b"));
}

// Go: internal/glob/glob.go:match (starStar)
#[test]
fn match_globstar_cross_segments() {
    let g = parse("**/*.ts").unwrap();
    assert!(g.match_input("a/b/c.ts"));
}

// Go: internal/glob/glob.go:match (starStar special case: matches none)
#[test]
fn match_globstar_matches_none() {
    let g = parse("**/a").unwrap();
    assert!(g.match_input("a"));
}

// Go: internal/glob/glob.go:match (starStar special case: trailing matches all)
#[test]
fn match_globstar_trailing() {
    let g = parse("a/**").unwrap();
    assert!(g.match_input("a/b/c"));
}

// Go: internal/glob/glob.go:match (group)
#[test]
fn match_group_or() {
    let g = parse("*.{ts,js}").unwrap();
    assert!(g.match_input("x.ts"));
    assert!(g.match_input("x.js"));
    assert!(!g.match_input("x.go"));
}

// Go: internal/glob/glob.go:match (charRange)
#[test]
fn match_char_range() {
    let g = parse("example.[0-9]").unwrap();
    assert!(g.match_input("example.0"));
    assert!(!g.match_input("example.a"));
}

// Go: internal/glob/glob.go:match (charRange.negate)
//
// Divergence: Go parses `[!...]` but its matcher never reads `negate`, so
// `[!0-9]` behaves exactly like `[0-9]`. This faithfully reproduces upstream;
// LSP-spec negation is deferred to P10 parity.
#[test]
fn match_char_range_negate() {
    let g = parse("example.[!0-9]").unwrap();
    assert!(g.match_input("example.0"));
    assert!(!g.match_input("example.a"));
}
