use super::*;
use crate::context::any_context;
use tsgo_ast::Kind;

// Go: internal/format/rule.go:ruleAction (bit values)
#[test]
fn rule_action_bit_values_match_go() {
    assert_eq!(RuleAction::NONE.bits(), 0);
    assert_eq!(RuleAction::STOP_PROCESSING_SPACE_ACTIONS.bits(), 1 << 0);
    assert_eq!(RuleAction::STOP_PROCESSING_TOKEN_ACTIONS.bits(), 1 << 1);
    assert_eq!(RuleAction::INSERT_SPACE.bits(), 1 << 2);
    assert_eq!(RuleAction::INSERT_NEW_LINE.bits(), 1 << 3);
    assert_eq!(RuleAction::DELETE_SPACE.bits(), 1 << 4);
    assert_eq!(RuleAction::DELETE_TOKEN.bits(), 1 << 5);
    assert_eq!(RuleAction::INSERT_TRAILING_SEMICOLON.bits(), 1 << 6);
}

// Go: internal/format/rule.go:ruleAction (composite masks)
#[test]
fn rule_action_composite_masks_match_go() {
    assert_eq!(
        RuleAction::STOP_ACTION,
        RuleAction::STOP_PROCESSING_SPACE_ACTIONS | RuleAction::STOP_PROCESSING_TOKEN_ACTIONS
    );
    assert_eq!(
        RuleAction::MODIFY_SPACE_ACTION,
        RuleAction::INSERT_SPACE | RuleAction::INSERT_NEW_LINE | RuleAction::DELETE_SPACE
    );
    assert_eq!(
        RuleAction::MODIFY_TOKEN_ACTION,
        RuleAction::DELETE_TOKEN | RuleAction::INSERT_TRAILING_SEMICOLON
    );
}

// Go: internal/format/rule.go:ruleFlags
#[test]
fn rule_flags_values_match_go() {
    assert_eq!(RuleFlags::None as i32, 0);
    assert_eq!(RuleFlags::CanDeleteNewLines as i32, 1);
}

// Go: internal/format/rule.go:toTokenRange (single Kind)
#[test]
fn to_token_range_from_single_kind_is_specific() {
    let tr = to_token_range(Kind::CommaToken);
    assert!(tr.is_specific);
    assert_eq!(tr.tokens, vec![Kind::CommaToken]);
}

// Go: internal/format/rule.go:toTokenRange (slice of Kind)
#[test]
fn to_token_range_from_vec_is_specific() {
    let tr = to_token_range(vec![Kind::CommaToken, Kind::SemicolonToken]);
    assert!(tr.is_specific);
    assert_eq!(tr.tokens, vec![Kind::CommaToken, Kind::SemicolonToken]);
}

// Go: internal/format/rule.go:toTokenRange (tokenRange passthrough preserves isSpecific)
#[test]
fn to_token_range_passthrough_preserves_is_specific() {
    let any = TokenRange {
        is_specific: false,
        tokens: vec![Kind::Identifier],
    };
    let tr = to_token_range(any);
    assert!(!tr.is_specific);
    assert_eq!(tr.tokens, vec![Kind::Identifier]);
}

// Go: internal/format/rule.go:rule + ruleImpl accessors
#[test]
fn rule_builds_spec_with_ranges_and_action() {
    let spec = rule(
        "SpaceAfterComma",
        Kind::CommaToken,
        Kind::Identifier,
        any_context(),
        RuleAction::INSERT_SPACE,
    );
    assert_eq!(spec.left_token_range.tokens, vec![Kind::CommaToken]);
    assert_eq!(spec.right_token_range.tokens, vec![Kind::Identifier]);
    assert_eq!(spec.rule.action(), RuleAction::INSERT_SPACE);
    assert_eq!(spec.rule.flags(), RuleFlags::None);
    assert_eq!(spec.rule.name(), "SpaceAfterComma");
    assert!(spec.rule.context().is_empty());
}

// Go: internal/format/rule.go:rule (variadic flags)
#[test]
fn rule_flags_constructor_sets_flags() {
    let spec = rule_flags(
        "SpaceAfterTemplateHeadAndMiddle",
        Kind::TemplateHead,
        Kind::Identifier,
        any_context(),
        RuleAction::INSERT_SPACE,
        RuleFlags::CanDeleteNewLines,
    );
    assert_eq!(spec.rule.flags(), RuleFlags::CanDeleteNewLines);
}
