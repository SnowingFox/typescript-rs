use super::*;
use tsgo_ast::Kind;

// Self-check: ALL_TOKEN_KINDS must list every token kind in discriminant order,
// from KindFirstToken (Unknown = 0) through KindLastToken (DeferKeyword). This
// guards the hand-transcribed table against drift from `tsgo_ast::Kind`.
#[test]
fn all_token_kinds_is_contiguous_token_range() {
    assert_eq!(ALL_TOKEN_KINDS.len(), Kind::LAST_TOKEN as usize + 1);
    for (i, k) in ALL_TOKEN_KINDS.iter().enumerate() {
        assert_eq!(*k as usize, i, "ALL_TOKEN_KINDS[{i}] out of order");
    }
    assert_eq!(*ALL_TOKEN_KINDS.first().unwrap(), Kind::Unknown);
    assert_eq!(*ALL_TOKEN_KINDS.last().unwrap(), Kind::LAST_TOKEN);
}

// Go: internal/format/rules.go:tokenRangeFromRange (keywords)
#[test]
fn token_range_from_range_keywords_count_matches() {
    let kw = token_range_from_range(Kind::FIRST_KEYWORD, Kind::LAST_KEYWORD);
    assert!(kw.is_specific);
    assert_eq!(
        kw.tokens.len(),
        (Kind::LAST_KEYWORD as i32 - Kind::FIRST_KEYWORD as i32 + 1) as usize
    );
    assert_eq!(*kw.tokens.first().unwrap(), Kind::FIRST_KEYWORD);
    assert_eq!(*kw.tokens.last().unwrap(), Kind::LAST_KEYWORD);
}

// Go: internal/format/rules.go:tokenRangeFrom
#[test]
fn token_range_from_is_specific() {
    let tr = token_range_from(&[Kind::Identifier, Kind::GreaterThanToken]);
    assert!(tr.is_specific);
    assert_eq!(tr.tokens, vec![Kind::Identifier, Kind::GreaterThanToken]);
}

// Go: internal/format/rules.go:tokenRangeFromEx
#[test]
fn token_range_from_ex_prepends_prefix() {
    let tr = token_range_from_ex(&[Kind::Identifier], &[Kind::MultiLineCommentTrivia]);
    assert!(tr.is_specific);
    assert_eq!(
        tr.tokens,
        vec![Kind::Identifier, Kind::MultiLineCommentTrivia]
    );
}

// Go: internal/format/rules.go:getAllRules
#[test]
fn get_all_rules_contains_known_rules() {
    let rules = get_all_rules();
    // The full table is large (Go has ~80+ rules across the three priority tiers).
    assert!(
        rules.len() > 80,
        "expected a large rule table, got {}",
        rules.len()
    );
    let names: Vec<&str> = rules.iter().map(|r| r.rule.name()).collect();
    for expected in [
        "SpaceAfterComma",
        "NoSpaceAfterComma",
        "SpaceBeforeBinaryOperator",
        "SpaceAfterBinaryOperator",
        "NoSpaceBeforeSemicolon",
        "SpaceAfterSemicolon",
        "NewLineBeforeCloseBraceInBlockContext",
        "NewLineAfterOpenBraceInBlockContext",
        "SpaceAfterCloseBrace",
    ] {
        assert!(names.contains(&expected), "missing rule {expected}");
    }
}

// Go: internal/format/rules.go:getAllRules (priority order is high, then user, then low)
#[test]
fn get_all_rules_priority_ordering() {
    let rules = get_all_rules();
    let pos = |name: &str| rules.iter().position(|r| r.rule.name() == name).unwrap();
    // A high-priority common rule precedes a user-configurable rule, which
    // precedes a low-priority common rule.
    assert!(pos("IgnoreBeforeComment") < pos("SpaceAfterComma"));
    assert!(pos("SpaceAfterComma") < pos("NoSpaceBeforeSemicolon"));
}
