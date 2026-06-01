//! The bucketed rule lookup.
//!
//! 1:1 port of Go `internal/format/rulesmap.go`. Rules are bucketed by a flat
//! `(leftToken, rightToken)` index so that, given an adjacent token pair and a
//! `FormattingContext`, the applicable rules can be found without scanning the
//! whole rule list. Within a bucket, rules are ordered by specificity/priority
//! via the insertion-bitmap math (see [`add_rule`]).

use crate::context::FormattingContext;
use crate::rule::{RuleAction, RuleImpl};
use crate::rules::get_all_rules;
use std::sync::{Arc, OnceLock};
use tsgo_ast::Kind;

/// Number of bits each sub-bucket counter occupies in the construction bitmap.
// Go: internal/format/rulesmap.go:maskBitSize
pub const MASK_BIT_SIZE: i32 = 5;

/// Low-5-bit mask used to read/write a single sub-bucket counter.
// Go: internal/format/rulesmap.go:mask
pub const MASK: i32 = 0b11111;

/// Stride of the flat rules map: `KindLastToken + 1`.
///
/// The map is conceptually a `MAP_ROW_LENGTH * MAP_ROW_LENGTH` table indexed by
/// `(leftToken, rightToken)`.
// Go: internal/format/rulesmap.go:mapRowLength
pub const MAP_ROW_LENGTH: i32 = Kind::LAST_TOKEN as i32 + 1;

/// Computes the flat bucket index for an adjacent token pair `(row, column)`.
///
/// Both kinds must be tokens (`<= KindLastKeyword`); this is asserted in debug
/// builds, mirroring Go's `debug.Assert`.
///
/// # Examples
/// ```
/// use tsgo_format::rulesmap::{get_rule_bucket_index, MAP_ROW_LENGTH};
/// use tsgo_ast::Kind;
/// let i = get_rule_bucket_index(Kind::CommaToken, Kind::Identifier);
/// assert_eq!(i, Kind::CommaToken as usize * MAP_ROW_LENGTH as usize + Kind::Identifier as usize);
/// ```
///
/// Side effects: none (pure).
// Go: internal/format/rulesmap.go:getRuleBucketIndex
pub fn get_rule_bucket_index(row: Kind, column: Kind) -> usize {
    debug_assert!(
        row <= Kind::LAST_KEYWORD && column <= Kind::LAST_KEYWORD,
        "Must compute formatting context from tokens"
    );
    (row as usize) * (MAP_ROW_LENGTH as usize) + (column as usize)
}

/// For a given rule action, returns the mask of other actions that cannot be
/// applied at the same position.
///
/// A space action excludes other space actions; a token action excludes other
/// token actions; the corresponding stop actions exclude their families too.
///
/// # Examples
/// ```
/// use tsgo_format::RuleAction;
/// use tsgo_format::rulesmap::get_rule_action_exclusion;
/// assert_eq!(
///     get_rule_action_exclusion(RuleAction::INSERT_SPACE),
///     RuleAction::MODIFY_SPACE_ACTION
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/format/rulesmap.go:getRuleActionExclusion
pub fn get_rule_action_exclusion(rule_action: RuleAction) -> RuleAction {
    let mut mask = RuleAction::NONE;
    if rule_action.intersects(RuleAction::STOP_PROCESSING_SPACE_ACTIONS) {
        mask |= RuleAction::MODIFY_SPACE_ACTION;
    }
    if rule_action.intersects(RuleAction::STOP_PROCESSING_TOKEN_ACTIONS) {
        mask |= RuleAction::MODIFY_TOKEN_ACTION;
    }
    if rule_action.intersects(RuleAction::MODIFY_SPACE_ACTION) {
        mask |= RuleAction::MODIFY_SPACE_ACTION;
    }
    if rule_action.intersects(RuleAction::MODIFY_TOKEN_ACTION) {
        mask |= RuleAction::MODIFY_TOKEN_ACTION;
    }
    mask
}

/// Where in a bucket a rule is inserted, by specificity/priority class.
///
/// Mirrors Go's `RulesPosition`. The values are bit offsets into the
/// construction bitmap (each sub-bucket counter is [`MASK_BIT_SIZE`] bits wide),
/// so they double as shift amounts. Rules are ordered within a bucket as:
/// stop-specific, stop-any, context-specific, context-any, no-context-specific,
/// no-context-any.
///
/// # Examples
/// ```
/// use tsgo_format::rulesmap::RulesPosition;
/// assert_eq!(RulesPosition::ContextRulesSpecific as i32, 10);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/format/rulesmap.go:RulesPosition
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(i32)]
pub enum RulesPosition {
    /// Stop rules with a specific token combination (`= 0`).
    StopRulesSpecific = 0,
    /// Stop rules with any token combination (`= maskBitSize * 1`).
    StopRulesAny = 5,
    /// Context rules with a specific token combination (`= maskBitSize * 2`).
    ContextRulesSpecific = 10,
    /// Context rules with any token combination (`= maskBitSize * 3`).
    ContextRulesAny = 15,
    /// Non-context rules with a specific token combination (`= maskBitSize * 4`).
    NoContextRulesSpecific = 20,
    /// Non-context rules with any token combination (`= maskBitSize * 5`).
    NoContextRulesAny = 25,
}

/// Reads the insertion index for `mask_position` from a bucket's construction
/// bitmap by summing the counts of all sub-buckets up to and including it.
///
/// # Examples
/// ```
/// use tsgo_format::rulesmap::{get_rule_insertion_index, RulesPosition};
/// assert_eq!(get_rule_insertion_index(0, RulesPosition::StopRulesSpecific), 0);
/// ```
///
/// Side effects: none (pure).
// Go: internal/format/rulesmap.go:getRuleInsertionIndex
pub fn get_rule_insertion_index(mut index_bitmap: i32, mask_position: RulesPosition) -> usize {
    let mut index = 0i32;
    let mut pos = 0i32;
    while pos <= mask_position as i32 {
        index += index_bitmap & MASK;
        index_bitmap >>= MASK_BIT_SIZE;
        pos += MASK_BIT_SIZE;
    }
    index as usize
}

/// Bumps the sub-bucket counter at `mask_position` by one, returning the updated
/// bitmap.
///
/// Asserts (in debug builds) that no sub-bucket exceeds 31 entries, mirroring
/// Go's `debug.Assert`.
///
/// # Examples
/// ```
/// use tsgo_format::rulesmap::{increase_insertion_index, RulesPosition};
/// assert_eq!(increase_insertion_index(0, RulesPosition::StopRulesAny), 32);
/// ```
///
/// Side effects: none (pure).
// Go: internal/format/rulesmap.go:increaseInsertionIndex
pub fn increase_insertion_index(index_bitmap: i32, mask_position: RulesPosition) -> i32 {
    let shift = mask_position as i32;
    let value = ((index_bitmap >> shift) & MASK) + 1;
    debug_assert!(
        (value & MASK) == value,
        "Adding more rules into the sub-bucket than allowed. Maximum allowed is 32 rules."
    );
    (index_bitmap & !(MASK << shift)) | (value << shift)
}

/// Inserts `rule` into a bucket at the index dictated by its specificity and
/// whether it is a stop / context / no-context rule, updating the bucket's
/// construction bitmap.
///
/// Mirrors Go's `addRule`. The ordering classes are (highest priority first):
/// stop-specific, stop-any, context-specific, context-any, no-context-specific,
/// no-context-any.
///
/// Side effects: mutates `rules` (inserts an element) and
/// `construction_state[rules_bucket_index]`.
// Go: internal/format/rulesmap.go:addRule
fn add_rule(
    rules: &mut Vec<Arc<RuleImpl>>,
    rule: Arc<RuleImpl>,
    specific_tokens: bool,
    construction_state: &mut [i32],
    rules_bucket_index: usize,
) {
    let position = if rule.action().intersects(RuleAction::STOP_ACTION) {
        if specific_tokens {
            RulesPosition::StopRulesSpecific
        } else {
            RulesPosition::StopRulesAny
        }
    } else if !rule.context().is_empty() {
        if specific_tokens {
            RulesPosition::ContextRulesSpecific
        } else {
            RulesPosition::ContextRulesAny
        }
    } else if specific_tokens {
        RulesPosition::NoContextRulesSpecific
    } else {
        RulesPosition::NoContextRulesAny
    };

    let state = construction_state[rules_bucket_index];
    let index = get_rule_insertion_index(state, position);
    rules.insert(index, rule);
    construction_state[rules_bucket_index] = increase_insertion_index(state, position);
}

/// Builds the full flat rules map from [`get_all_rules`], bucketing each rule by
/// every `(left, right)` token pair it matches and ordering within buckets via
/// [`add_rule`].
///
/// The map is a `MAP_ROW_LENGTH * MAP_ROW_LENGTH` table (see
/// [`get_rule_bucket_index`]); most buckets are empty.
///
/// # Examples
/// ```
/// use tsgo_format::rulesmap::{build_rules_map, get_rule_bucket_index};
/// use tsgo_ast::Kind;
/// let map = build_rules_map();
/// let bucket = &map[get_rule_bucket_index(Kind::CommaToken, Kind::Identifier)];
/// assert!(bucket.iter().any(|r| r.name() == "SpaceAfterComma"));
/// ```
///
/// Side effects: none (pure; allocates the map).
// Go: internal/format/rulesmap.go:buildRulesMap
pub fn build_rules_map() -> Vec<Vec<Arc<RuleImpl>>> {
    let rules = get_all_rules();
    let size = (MAP_ROW_LENGTH as usize) * (MAP_ROW_LENGTH as usize);
    let mut m: Vec<Vec<Arc<RuleImpl>>> = vec![Vec::new(); size];
    let mut construction_state = vec![0i32; size];
    for spec in &rules {
        let specific_rule = spec.left_token_range.is_specific && spec.right_token_range.is_specific;
        for &left in &spec.left_token_range.tokens {
            for &right in &spec.right_token_range.tokens {
                let index = get_rule_bucket_index(left, right);
                add_rule(
                    &mut m[index],
                    Arc::clone(&spec.rule),
                    specific_rule,
                    &mut construction_state,
                    index,
                );
            }
        }
    }
    m
}

/// Returns the globally cached rules map, building it on first use.
///
/// Mirrors Go's `getRulesMap = sync.OnceValue(buildRulesMap)`.
///
/// Side effects: builds and caches the map on first call.
// Go: internal/format/rulesmap.go:getRulesMap
pub fn get_rules_map() -> &'static Vec<Vec<Arc<RuleImpl>>> {
    static RULES_MAP: OnceLock<Vec<Vec<Arc<RuleImpl>>>> = OnceLock::new();
    RULES_MAP.get_or_init(build_rules_map)
}

/// Returns the rules that apply to the current `(left, right)` token pair in
/// `context`, appending them to `rules` (mirroring Go's accumulator parameter).
///
/// Rules are taken from the matching bucket in priority order; a rule applies
/// only if every predicate holds and its action is not excluded by a
/// higher-priority rule already chosen (see [`get_rule_action_exclusion`]).
///
/// # Examples
/// ```
/// use tsgo_format::context::{FormattingContext, FormatRequestKind};
/// use tsgo_format::format_code_settings::get_default_format_code_settings;
/// use tsgo_format::rulesmap::get_rules;
/// use tsgo_ast::Kind;
/// let mut c = FormattingContext::new(get_default_format_code_settings(), FormatRequestKind::FormatDocument);
/// c.current_token_kind = Kind::CommaToken;
/// c.next_token_kind = Kind::Identifier;
/// c.context_node_kind = Kind::ArrayLiteralExpression;
/// c.tokens_are_on_same_line = true;
/// let applied = get_rules(&c, Vec::new());
/// assert!(applied.iter().any(|r| r.name() == "SpaceAfterComma"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/format/rulesmap.go:getRules
pub fn get_rules(context: &FormattingContext, mut rules: Vec<Arc<RuleImpl>>) -> Vec<Arc<RuleImpl>> {
    let bucket = &get_rules_map()
        [get_rule_bucket_index(context.current_token_kind, context.next_token_kind)];
    if bucket.is_empty() {
        return rules;
    }
    let mut rule_action_mask = RuleAction::NONE;
    'outer: for rule in bucket {
        let accept_rule_actions = !get_rule_action_exclusion(rule_action_mask);
        if rule.action().intersects(accept_rule_actions) {
            for pred in rule.context() {
                if !pred(context) {
                    continue 'outer;
                }
            }
            rules.push(Arc::clone(rule));
            rule_action_mask |= rule.action();
        }
    }
    rules
}

#[cfg(test)]
#[path = "rulesmap_test.rs"]
mod tests;
