use super::*;
use crate::context::{FormatRequestKind, FormattingContext};
use crate::format_code_settings::get_default_format_code_settings;
use crate::rule::RuleImpl;
use tsgo_ast::Kind;

fn base_ctx() -> FormattingContext {
    FormattingContext::new(
        get_default_format_code_settings(),
        FormatRequestKind::FormatDocument,
    )
}

/// The winning whitespace action is the action of the first applied rule that
/// touches whitespace (rules are returned in priority order).
fn first_space_action(rules: &[std::sync::Arc<RuleImpl>]) -> Option<(&'static str, RuleAction)> {
    rules
        .iter()
        .find(|r| r.action().intersects(RuleAction::MODIFY_SPACE_ACTION))
        .map(|r| (r.name(), r.action()))
}

fn applied_names(rules: &[std::sync::Arc<RuleImpl>]) -> Vec<&'static str> {
    rules.iter().map(|r| r.name()).collect()
}

// Go: internal/format/rulesmap.go:getRuleBucketIndex
#[test]
fn rule_bucket_index_is_row_times_width_plus_column() {
    let row = Kind::CommaToken;
    let col = Kind::Identifier;
    assert_eq!(
        get_rule_bucket_index(row, col),
        (row as usize) * (MAP_ROW_LENGTH as usize) + (col as usize)
    );
}

// Go: internal/format/rulesmap.go:mapRowLength
#[test]
fn map_row_length_is_last_token_plus_one() {
    assert_eq!(MAP_ROW_LENGTH, Kind::LAST_TOKEN as i32 + 1);
}

// Go: internal/format/rulesmap.go:getRuleActionExclusion
#[test]
fn rule_action_exclusion_space_actions() {
    // Inserting a space excludes other space-modifying actions.
    assert_eq!(
        get_rule_action_exclusion(RuleAction::INSERT_SPACE),
        RuleAction::MODIFY_SPACE_ACTION
    );
    // Stop-processing-space excludes space-modifying actions.
    assert_eq!(
        get_rule_action_exclusion(RuleAction::STOP_PROCESSING_SPACE_ACTIONS),
        RuleAction::MODIFY_SPACE_ACTION
    );
}

// Go: internal/format/rulesmap.go:getRuleActionExclusion
#[test]
fn rule_action_exclusion_token_actions() {
    assert_eq!(
        get_rule_action_exclusion(RuleAction::DELETE_TOKEN),
        RuleAction::MODIFY_TOKEN_ACTION
    );
    assert_eq!(
        get_rule_action_exclusion(RuleAction::STOP_PROCESSING_TOKEN_ACTIONS),
        RuleAction::MODIFY_TOKEN_ACTION
    );
}

// Go: internal/format/rulesmap.go:getRuleActionExclusion
#[test]
fn rule_action_exclusion_none_is_none() {
    assert_eq!(
        get_rule_action_exclusion(RuleAction::NONE),
        RuleAction::NONE
    );
}

// Go: internal/format/rulesmap.go:RulesPosition
#[test]
fn rules_position_discriminants_match_go() {
    assert_eq!(RulesPosition::StopRulesSpecific as i32, 0);
    assert_eq!(RulesPosition::StopRulesAny as i32, 5);
    assert_eq!(RulesPosition::ContextRulesSpecific as i32, 10);
    assert_eq!(RulesPosition::ContextRulesAny as i32, 15);
    assert_eq!(RulesPosition::NoContextRulesSpecific as i32, 20);
    assert_eq!(RulesPosition::NoContextRulesAny as i32, 25);
}

// Go: internal/format/rulesmap.go:getRuleInsertionIndex
#[test]
fn get_rule_insertion_index_sums_lower_subbuckets() {
    assert_eq!(
        get_rule_insertion_index(0, RulesPosition::StopRulesSpecific),
        0
    );
    // bitmap with 1 in sub-bucket 0 -> insertion index at StopRulesSpecific is 1.
    assert_eq!(
        get_rule_insertion_index(1, RulesPosition::StopRulesSpecific),
        1
    );
    // bitmap = 32 (1 in sub-bucket 1): index at StopRulesAny sums sub-buckets 0..=1 = 0+1.
    assert_eq!(get_rule_insertion_index(32, RulesPosition::StopRulesAny), 1);
}

// Go: internal/format/rulesmap.go:increaseInsertionIndex
#[test]
fn increase_insertion_index_bumps_target_subbucket() {
    // empty bitmap, bump sub-bucket 0 -> 1.
    assert_eq!(
        increase_insertion_index(0, RulesPosition::StopRulesSpecific),
        1
    );
    // empty bitmap, bump sub-bucket 1 (shift 5) -> 1<<5 = 32.
    assert_eq!(increase_insertion_index(0, RulesPosition::StopRulesAny), 32);
    // bumping does not disturb other sub-buckets: start with 1 in bucket 0.
    assert_eq!(
        increase_insertion_index(1, RulesPosition::StopRulesAny),
        1 | 32
    );
}

// Go: internal/format/rulesmap.go:buildRulesMap
#[test]
fn build_rules_map_buckets_space_after_comma() {
    let map = build_rules_map();
    let bucket = &map[get_rule_bucket_index(Kind::CommaToken, Kind::Identifier)];
    let names: Vec<&str> = bucket.iter().map(|r| r.name()).collect();
    assert!(names.contains(&"SpaceAfterComma"));
    assert!(names.contains(&"NoSpaceAfterComma"));
}

// Behavior: `[1,2,3]` -> `[1, 2, 3]`. The (comma, identifier) pair in an array
// literal, with the default options, resolves to a single space insertion.
// Go: internal/format/rules.go:SpaceAfterComma
#[test]
fn space_after_comma_inserts_space() {
    let mut c = base_ctx();
    c.current_token_kind = Kind::CommaToken;
    c.next_token_kind = Kind::Identifier;
    c.context_node_kind = Kind::ArrayLiteralExpression;
    c.current_token_parent_kind = Kind::ArrayLiteralExpression;
    c.next_token_parent_kind = Kind::ArrayLiteralExpression;
    c.tokens_are_on_same_line = true;

    let applied = get_rules(&c, Vec::new());
    assert!(applied_names(&applied).contains(&"SpaceAfterComma"));
    assert_eq!(
        first_space_action(&applied),
        Some(("SpaceAfterComma", RuleAction::INSERT_SPACE))
    );
}

// Behavior: with the comma-spacing option disabled, the same pair deletes the
// space instead.
// Go: internal/format/rules.go:NoSpaceAfterComma
#[test]
fn no_space_after_comma_when_option_disabled() {
    let mut c = base_ctx();
    c.options.insert_space_after_comma_delimiter = tsgo_core::tristate::Tristate::False;
    c.current_token_kind = Kind::CommaToken;
    c.next_token_kind = Kind::Identifier;
    c.context_node_kind = Kind::ArrayLiteralExpression;
    c.tokens_are_on_same_line = true;

    let applied = get_rules(&c, Vec::new());
    assert_eq!(
        first_space_action(&applied),
        Some(("NoSpaceAfterComma", RuleAction::DELETE_SPACE))
    );
}

// Behavior: `1+2` -> `1 + 2`. The (plus, numeric) side of a binary expression
// resolves to a space insertion after the operator (default options).
// Go: internal/format/rules.go:SpaceAfterBinaryOperator
#[test]
fn space_after_binary_operator_inserts_space() {
    let mut c = base_ctx();
    c.current_token_kind = Kind::PlusToken;
    c.next_token_kind = Kind::NumericLiteral;
    c.context_node_kind = Kind::BinaryExpression;
    c.binary_operator_token_kind = Kind::PlusToken;
    c.current_token_parent_kind = Kind::BinaryExpression;
    c.next_token_parent_kind = Kind::BinaryExpression;
    c.tokens_are_on_same_line = true;

    let applied = get_rules(&c, Vec::new());
    assert_eq!(
        first_space_action(&applied),
        Some(("SpaceAfterBinaryOperator", RuleAction::INSERT_SPACE))
    );
}

// Behavior: the (numeric, plus) side resolves to a space *before* the operator.
// Go: internal/format/rules.go:SpaceBeforeBinaryOperator
#[test]
fn space_before_binary_operator_inserts_space() {
    let mut c = base_ctx();
    c.current_token_kind = Kind::NumericLiteral;
    c.next_token_kind = Kind::PlusToken;
    c.context_node_kind = Kind::BinaryExpression;
    c.binary_operator_token_kind = Kind::PlusToken;
    c.current_token_parent_kind = Kind::BinaryExpression;
    c.next_token_parent_kind = Kind::BinaryExpression;
    c.tokens_are_on_same_line = true;

    let applied = get_rules(&c, Vec::new());
    assert_eq!(
        first_space_action(&applied),
        Some(("SpaceBeforeBinaryOperator", RuleAction::INSERT_SPACE))
    );
}

// Behavior: `const x = 1 ;` -> `const x = 1;`. Any token before a semicolon on
// the same line deletes the preceding space.
// Go: internal/format/rules.go:NoSpaceBeforeSemicolon
#[test]
fn no_space_before_semicolon_deletes_space() {
    let mut c = base_ctx();
    c.current_token_kind = Kind::NumericLiteral;
    c.next_token_kind = Kind::SemicolonToken;
    c.context_node_kind = Kind::VariableStatement;
    c.tokens_are_on_same_line = true;

    let applied = get_rules(&c, Vec::new());
    assert_eq!(
        first_space_action(&applied),
        Some(("NoSpaceBeforeSemicolon", RuleAction::DELETE_SPACE))
    );
}

// Behavior: a `}` followed by `else` on the same line gets a separating space
// (the else/while tokens aren't in the tree so SpaceAfterCloseBrace can't apply).
// Go: internal/format/rules.go:SpaceBetweenCloseBraceAndElse
#[test]
fn space_between_close_brace_and_else() {
    let mut c = base_ctx();
    c.current_token_kind = Kind::CloseBraceToken;
    c.next_token_kind = Kind::ElseKeyword;
    c.context_node_kind = Kind::IfStatement;
    c.tokens_are_on_same_line = true;

    let applied = get_rules(&c, Vec::new());
    assert_eq!(
        first_space_action(&applied),
        Some(("SpaceBetweenCloseBraceAndElse", RuleAction::INSERT_SPACE))
    );
}

// Behavior: in a multi-line block, a newline is inserted before the closing
// brace (`function f() {\nreturn 1; }` -> `}` on its own line).
// Go: internal/format/rules.go:NewLineBeforeCloseBraceInBlockContext
#[test]
fn newline_before_close_brace_in_multiline_block() {
    let mut c = base_ctx();
    c.current_token_kind = Kind::Identifier;
    c.next_token_kind = Kind::CloseBraceToken;
    c.context_node_kind = Kind::Block;
    // multiline block: not all on one line, block not on one line
    c.context_node_all_on_same_line = false;
    c.context_node_block_is_on_one_line = false;
    c.tokens_are_on_same_line = false;

    let applied = get_rules(&c, Vec::new());
    assert_eq!(
        first_space_action(&applied),
        Some((
            "NewLineBeforeCloseBraceInBlockContext",
            RuleAction::INSERT_NEW_LINE
        ))
    );
}

// Behavior: an already-correct token pair with no matching rule produces no
// edits at all (the zero-edit / already-formatted case).
// Go: internal/format/rulesmap.go:getRules (empty bucket / no matching rule)
#[test]
fn no_applicable_rule_yields_no_edits() {
    let mut c = base_ctx();
    c.current_token_kind = Kind::Identifier;
    c.next_token_kind = Kind::Identifier;
    c.context_node_kind = Kind::VariableStatement;
    c.tokens_are_on_same_line = true;

    let applied = get_rules(&c, Vec::new());
    assert!(
        applied.is_empty(),
        "expected no rules, got {:?}",
        applied_names(&applied)
    );
}

// `get_rules_map` is the cached singleton; repeated calls return the same map.
// Go: internal/format/rulesmap.go:getRulesMap
#[test]
fn get_rules_map_is_cached() {
    let a = get_rules_map();
    let b = get_rules_map();
    assert!(std::ptr::eq(a, b));
}
