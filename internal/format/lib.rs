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
//! # Scope of this port (P7 `format`, round 1)
//!
//! This round ports the **deterministic rule engine** — the genuinely reachable
//! core that needs only `tsgo_ast::Kind` + `tsgo_core::Tristate`:
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
//! - [`span`]: the pure indentation-string helper
//!   ([`get_indentation_string`](span::get_indentation_string)).
//!
//! # `FormattingContext` projection (deliberate, documented divergence)
//!
//! Go's `FormattingContext` holds raw `*ast.Node` pointers and the predicates
//! call accessors on them (`context.contextNode.AsBinaryExpression()...`). The
//! Rust `tsgo_ast` arena is owned by an `&mut` navigation context
//! (`tsgo_astnav::SourceFile`), which cannot be threaded through a recursive
//! visitor that also reads node data and drives a scanner. So — like the arena
//! deviation documented in `PORTING.md §5` — [`FormattingContext`] stores the
//! *projection* of the AST the predicates actually read (token kinds, the
//! context/parent node kinds, the option values, and the line-relationship
//! booleans that Go computes lazily from positions). Each predicate body still
//! translates 1:1 (`context.contextNode.Kind == ast.KindForStatement` becomes
//! `ctx.context_node_kind == Kind::ForStatement`). When the AST-walking worker is
//! ported, its `UpdateContext` populates these fields from real nodes.
//!
//! # Deferred to later P7 `format` rounds (blocked-by)
//!
//! - The AST-walking worker (`span.go`: `formatSpanWorker`), the
//!   `formattingScanner` (`scanner.go`), the `SmartIndenter` (`indent.go`), and
//!   the public entries (`api.go`: `FormatDocument`/`FormatSpan`/`FormatOn*`).
//!   blocked-by: the arena `&mut`-threading design + SourceFile-aware scanner
//!   line/position helpers (`GetECMALineOfPosition`/`GetECMALineStarts`/
//!   `GetTokenPosOfNode`) + `tsgo_astnav` `&mut SourceFile` integration +
//!   `ast::NodeVisitor`/`VisitEachChild` plumbing.
//! - The deep-AST context predicates (`isSemicolonDeletionContext`,
//!   `isSemicolonInsertionContext`, `isEndOfDecoratorContextOnSameLine`).
//!   blocked-by: `astnav.FindNextToken`/`FindPrecedingToken` and
//!   `lsutil.PositionIsASICandidate` (deferred in `tsgo_ls_lsutil`).
//!
//! See the phase-7 worklog for the full deferral list.

pub mod context;
pub mod format_code_settings;
pub mod rule;
mod rulecontext;
pub mod rules;
pub mod rulesmap;
pub mod span;

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
pub use span::get_indentation_string;
