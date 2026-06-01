//! The formatting rule model.
//!
//! 1:1 port of Go `internal/format/rule.go`: a rule pairs a left/right
//! [`TokenRange`] with a [`RuleImpl`] declaring the whitespace [`RuleAction`] to
//! apply between two adjacent tokens in a matching `FormattingContext`.

use crate::context::ContextPredicate;
use bitflags::bitflags;
use std::sync::Arc;
use tsgo_ast::Kind;

bitflags! {
    /// The whitespace/token annotation a rule declares between two tokens.
    ///
    /// Mirrors Go's `ruleAction` bitset: a rule may insert/delete a space or
    /// newline, delete a token, insert a trailing semicolon, or stop further
    /// space/token processing. The composite masks ([`RuleAction::STOP_ACTION`],
    /// [`RuleAction::MODIFY_SPACE_ACTION`], [`RuleAction::MODIFY_TOKEN_ACTION`])
    /// group the primitive flags exactly as Go does.
    ///
    /// # Examples
    /// ```
    /// use tsgo_format::RuleAction;
    /// assert!(RuleAction::STOP_ACTION.contains(RuleAction::STOP_PROCESSING_SPACE_ACTIONS));
    /// assert_eq!(RuleAction::INSERT_SPACE.bits(), 1 << 2);
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/format/rule.go:ruleAction
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
    pub struct RuleAction: i32 {
        /// No action (`ruleActionNone`).
        const NONE = 0;
        /// Stop applying any further space-modifying actions at this position.
        const STOP_PROCESSING_SPACE_ACTIONS = 1 << 0;
        /// Stop applying any further token-modifying actions at this position.
        const STOP_PROCESSING_TOKEN_ACTIONS = 1 << 1;
        /// Insert a single space between the two tokens.
        const INSERT_SPACE = 1 << 2;
        /// Insert a newline between the two tokens.
        const INSERT_NEW_LINE = 1 << 3;
        /// Delete the whitespace between the two tokens.
        const DELETE_SPACE = 1 << 4;
        /// Delete the left token.
        const DELETE_TOKEN = 1 << 5;
        /// Insert a trailing semicolon after the left token.
        const INSERT_TRAILING_SEMICOLON = 1 << 6;

        /// `STOP_PROCESSING_SPACE_ACTIONS | STOP_PROCESSING_TOKEN_ACTIONS`.
        const STOP_ACTION =
            Self::STOP_PROCESSING_SPACE_ACTIONS.bits() | Self::STOP_PROCESSING_TOKEN_ACTIONS.bits();
        /// `INSERT_SPACE | INSERT_NEW_LINE | DELETE_SPACE`.
        const MODIFY_SPACE_ACTION =
            Self::INSERT_SPACE.bits() | Self::INSERT_NEW_LINE.bits() | Self::DELETE_SPACE.bits();
        /// `DELETE_TOKEN | INSERT_TRAILING_SEMICOLON`.
        const MODIFY_TOKEN_ACTION =
            Self::DELETE_TOKEN.bits() | Self::INSERT_TRAILING_SEMICOLON.bits();
    }
}

/// Whether a rule is permitted to delete newlines when it applies.
///
/// Mirrors Go's `ruleFlags` iota enum (`ruleFlagsNone` /
/// `ruleFlagsCanDeleteNewLines`).
///
/// # Examples
/// ```
/// use tsgo_format::RuleFlags;
/// assert_eq!(RuleFlags::default(), RuleFlags::None);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/format/rule.go:ruleFlags
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
#[repr(i32)]
pub enum RuleFlags {
    /// No special handling (`ruleFlagsNone`).
    #[default]
    None = 0,
    /// The rule may delete newlines (`ruleFlagsCanDeleteNewLines`).
    CanDeleteNewLines = 1,
}

/// A set of token kinds a rule's left or right side matches.
///
/// Mirrors Go's `tokenRange`. `is_specific` is true when the range came from an
/// explicit token (or explicit slice) rather than from an "any token" wildcard;
/// it controls the rule's bucket priority in [`crate::rulesmap`].
///
/// Side effects: none (pure value type).
// Go: internal/format/rule.go:tokenRange
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenRange {
    /// The matched token kinds.
    pub tokens: Vec<Kind>,
    /// Whether this range is a specific token set (vs. an "any token" wildcard).
    pub is_specific: bool,
}

impl From<Kind> for TokenRange {
    // Go: internal/format/rule.go:toTokenRange (ast.Kind case)
    fn from(kind: Kind) -> TokenRange {
        TokenRange {
            is_specific: true,
            tokens: vec![kind],
        }
    }
}

impl From<Vec<Kind>> for TokenRange {
    // Go: internal/format/rule.go:toTokenRange ([]ast.Kind case)
    fn from(tokens: Vec<Kind>) -> TokenRange {
        TokenRange {
            is_specific: true,
            tokens,
        }
    }
}

/// Coerces a [`Kind`], a `Vec<Kind>`, or a [`TokenRange`] into a [`TokenRange`].
///
/// Mirrors Go's `toTokenRange`: a single kind or slice becomes a *specific*
/// range; an existing [`TokenRange`] passes through unchanged (preserving
/// `is_specific`, so "any token" wildcards stay non-specific).
///
/// # Examples
/// ```
/// use tsgo_format::rule::to_token_range;
/// use tsgo_ast::Kind;
/// let tr = to_token_range(Kind::CommaToken);
/// assert!(tr.is_specific);
/// assert_eq!(tr.tokens, vec![Kind::CommaToken]);
/// ```
///
/// Side effects: none (pure).
// Go: internal/format/rule.go:toTokenRange
pub fn to_token_range(e: impl Into<TokenRange>) -> TokenRange {
    e.into()
}

/// A single formatting rule: the predicates that must hold, the action to apply,
/// and the debug name.
///
/// Mirrors Go's `ruleImpl`. Held behind an [`Arc`] inside a [`RuleSpec`] and the
/// built rules map.
///
/// Side effects: none (pure value type).
// Go: internal/format/rule.go:ruleImpl
pub struct RuleImpl {
    debug_name: &'static str,
    context: Vec<ContextPredicate>,
    action: RuleAction,
    flags: RuleFlags,
}

impl RuleImpl {
    /// The whitespace/token action this rule applies.
    ///
    /// Side effects: none (pure).
    // Go: internal/format/rule.go:ruleImpl.Action
    pub fn action(&self) -> RuleAction {
        self.action
    }

    /// The predicates that must all hold for this rule to apply.
    ///
    /// Side effects: none (pure).
    // Go: internal/format/rule.go:ruleImpl.Context
    pub fn context(&self) -> &[ContextPredicate] {
        &self.context
    }

    /// The rule's flags (e.g. whether it may delete newlines).
    ///
    /// Side effects: none (pure).
    // Go: internal/format/rule.go:ruleImpl.Flags
    pub fn flags(&self) -> RuleFlags {
        self.flags
    }

    /// The rule's debug name (Go's `String()`).
    ///
    /// Side effects: none (pure).
    // Go: internal/format/rule.go:ruleImpl.String
    pub fn name(&self) -> &'static str {
        self.debug_name
    }
}

impl std::fmt::Debug for RuleImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleImpl")
            .field("debug_name", &self.debug_name)
            .field("action", &self.action)
            .field("flags", &self.flags)
            .field("context_len", &self.context.len())
            .finish()
    }
}

/// A rule together with the left/right token ranges it matches.
///
/// Mirrors Go's `ruleSpec`. The [`RuleImpl`] is shared (`Arc`) because the
/// builder inserts it into many `(left, right)` buckets.
///
/// Side effects: none (pure value type).
// Go: internal/format/rule.go:ruleSpec
#[derive(Clone, Debug)]
pub struct RuleSpec {
    /// The left-token range.
    pub left_token_range: TokenRange,
    /// The right-token range.
    pub right_token_range: TokenRange,
    /// The shared rule.
    pub rule: Arc<RuleImpl>,
}

/// Builds a [`RuleSpec`] with no special [`RuleFlags`] (`ruleFlagsNone`).
///
/// Mirrors Go's `rule(...)` with the variadic `flags` omitted. `left`/`right`
/// accept a [`Kind`], a `Vec<Kind>`, or a [`TokenRange`] (see
/// [`to_token_range`]).
///
/// # Examples
/// ```
/// use tsgo_format::rule::rule;
/// use tsgo_format::{RuleAction};
/// use tsgo_format::context::any_context;
/// use tsgo_ast::Kind;
/// let spec = rule("NoSpaceBeforeSemicolon", Kind::Identifier, Kind::SemicolonToken,
///     any_context(), RuleAction::DELETE_SPACE);
/// assert_eq!(spec.rule.action(), RuleAction::DELETE_SPACE);
/// ```
///
/// Side effects: none (pure).
// Go: internal/format/rule.go:rule
pub fn rule(
    debug_name: &'static str,
    left: impl Into<TokenRange>,
    right: impl Into<TokenRange>,
    context: Vec<ContextPredicate>,
    action: RuleAction,
) -> RuleSpec {
    rule_flags(debug_name, left, right, context, action, RuleFlags::None)
}

/// Builds a [`RuleSpec`] with explicit [`RuleFlags`].
///
/// Mirrors Go's `rule(...)` when a `flags` argument is supplied.
///
/// # Examples
/// ```
/// use tsgo_format::rule::rule_flags;
/// use tsgo_format::{RuleAction, RuleFlags};
/// use tsgo_format::context::any_context;
/// use tsgo_ast::Kind;
/// let spec = rule_flags("X", Kind::TemplateHead, Kind::Identifier, any_context(),
///     RuleAction::INSERT_SPACE, RuleFlags::CanDeleteNewLines);
/// assert_eq!(spec.rule.flags(), RuleFlags::CanDeleteNewLines);
/// ```
///
/// Side effects: none (pure).
// Go: internal/format/rule.go:rule
pub fn rule_flags(
    debug_name: &'static str,
    left: impl Into<TokenRange>,
    right: impl Into<TokenRange>,
    context: Vec<ContextPredicate>,
    action: RuleAction,
    flags: RuleFlags,
) -> RuleSpec {
    RuleSpec {
        left_token_range: to_token_range(left),
        right_token_range: to_token_range(right),
        rule: Arc::new(RuleImpl {
            debug_name,
            context,
            action,
            flags,
        }),
    }
}

#[cfg(test)]
#[path = "rule_test.rs"]
mod tests;
