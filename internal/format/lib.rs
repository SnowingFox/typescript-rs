//! `tsgo_format`: the TypeScript formatter rules engine.
//!
//! 1:1 Rust port of Go `internal/format` (the language-service formatter). The
//! formatter decides the whitespace/newline annotation between every pair of
//! adjacent tokens via a *rules engine* (`rules.go` + `rulesmap.go` +
//! `rule.go`), tested against a [`FormattingContext`] (`context.go` +
//! `rulecontext.go`); a `formattingScanner` walks the token stream; a
//! `SmartIndenter` computes indentation; and public entry points
//! (`FormatDocument`/`FormatSelection`/`FormatOnSemicolon`/...) turn the result
//! into `core.TextChange` edits.
//!
//! # Scope of this port (P7 `format`, rounds 1–2)
//!
//! **Round 1** ported the **deterministic rule engine**:
//!
//! - [`rule`]: the rule model (`RuleAction`, `RuleFlags`, `TokenRange`,
//!   `RuleSpec`, `RuleImpl`, [`rule`](rule::rule), `to_token_range`).
//! - [`rulesmap`]: the bucketed lookup — `get_rule_bucket_index`,
//!   `get_rule_action_exclusion`, the insertion-bitmap math, `build_rules_map`,
//!   and [`get_rules`](rulesmap::get_rules).
//! - [`rules`]: the full rule table ([`get_all_rules`](rules::get_all_rules))
//!   mirroring `rules.go`, plus the token-range constructors.
//! - [`context`] / [`rulecontext`]: [`FormattingContext`] (the projection of the
//!   AST that the predicates consult) and the reachable context predicates.
//! - [`format_code_settings`]: [`FormatCodeSettings`] + defaults (Go
//!   `lsutil/formatcodeoptions.go`).
//!
//! **Round 2** adds the AST-walking machinery over the `tsgo_astnav` shared
//! navigation surface ([`tsgo_astnav::NavSourceFile`]/[`tsgo_astnav::RcSourceFile`]):
//!
//! - [`scanner`]: the trivia-aware [`FormattingScanner`] (`scanner.go`).
//! - [`indent`]: the reachable [`SmartIndenter`](indent) subset
//!   (`should_indent_child_node` / `node_will_indent_child` /
//!   `get_indentation_for_node`) (`indent.go`).
//! - [`span`]: the [`FormatSpanWorker`] (`span.go`) that walks the AST, consults
//!   [`get_rules`](rulesmap::get_rules) between adjacent tokens, and emits
//!   `core.TextChange` edits — plus the pure
//!   [`get_indentation_string`](span::get_indentation_string).
//! - [`api`]: the public entries [`format_document`]/[`format_span`]/
//!   [`format_on_semicolon`]/[`format_on_closing_curly`] (`api.go`).
//!
//! # `FormattingContext` projection (deliberate, documented divergence)
//!
//! Go's `FormattingContext` holds raw `*ast.Node` pointers and the predicates
//! call accessors on them (`context.contextNode.AsBinaryExpression()...`). The
//! Rust `tsgo_ast` arena is borrowed by the navigation context, so
//! [`FormattingContext`] stores the *projection* of the AST the predicates read
//! (token kinds, context/parent node kinds, option values, line-relationship
//! booleans). The worker's `update_formatting_context` (Go's `UpdateContext`)
//! populates these fields from the real nodes via the nav context.
//!
//! # Worker traversal divergence (deliberate, documented)
//!
//! Go drives the walk with `ast.NewNodeVisitor`, distinguishing node *lists*
//! (`processChildNodes`, which opens a list indentation scope) from single
//! children (`processChildNode`). The shared `tsgo_astnav` surface exposes only
//! the flat `visit_each_child_and_jsdoc` child stream, so the worker collapses the
//! two into one path: list start/end tokens and commas are consumed as ordinary
//! "parent tokens" between children (exactly how Go consumes non-list tokens),
//! producing identical spacing/indent edits for the reachable cases. See
//! [`span`] and the worklog.
//!
//! # Deferred to later P7 `format` rounds (blocked-by)
//!
//! - Multi-line list **continuation** indent (`processChildNodes`' list scope +
//!   `tryComputeIndentationForListItem`), comment re-indentation
//!   (`indentMultilineComment` + the format-selection trivia tail), JSX/template
//!   rescans, decorators (`has_decorators` treated as false), and the
//!   position-based standalone `GetIndentation` (codefix / on-enter) with its
//!   comment-aware helpers.
//! - `FormatOnEnter` / `FormatOnOpeningCurly` / `FormatSelection` /
//!   `FormatNodeGivenIndentation`.
//! - The deep-AST context predicates (`isSemicolonDeletionContext`,
//!   `isSemicolonInsertionContext`, `isEndOfDecoratorContextOnSameLine`).
//!   blocked-by: `lsutil.PositionIsASICandidate` (deferred in `tsgo_ls_lsutil`)
//!   and decorator/expression parent walks.
//! - `rangeContainsError`: the nav context carries no diagnostics, so ranges are
//!   treated as error-free.
//!
//! See the phase-7 worklog for the full deferral list.

pub mod api;
pub mod context;
pub mod format_code_settings;
pub mod indent;
pub mod rule;
mod rulecontext;
pub mod rules;
pub mod rulesmap;
pub mod scanner;
pub mod span;
mod util;

pub use api::{format_document, format_on_closing_curly, format_on_semicolon, format_span};
pub use context::{any_context, ContextPredicate, FormatRequestKind, FormattingContext};
pub use format_code_settings::{
    get_default_format_code_settings, EditorSettings, FormatCodeSettings, IndentStyle,
    SemicolonPreference,
};
pub use rule::{
    rule, rule_flags, to_token_range, RuleAction, RuleFlags, RuleImpl, RuleSpec, TokenRange,
};
pub use rules::get_all_rules;
pub use rulesmap::{build_rules_map, get_rules, get_rules_map};
pub use scanner::{FormattingScanner, TextRangeWithKind, TokenInfo};
pub use span::{
    find_enclosing_node, get_indentation_string, get_own_or_inherited_delta,
    get_scan_start_position, FormatContext, FormatSpanWorker, LineAction,
};
