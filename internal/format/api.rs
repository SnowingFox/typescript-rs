//! The public formatter entry points (reachable subset of `api.go`).
//!
//! 1:1 port of the reachable subset of Go `internal/format/api.go`:
//! [`format_document`], [`format_span`], [`format_on_semicolon`], and
//! [`format_on_closing_curly`]. Each runs the AST-walking
//! [`crate::span::FormatSpanWorker`] over a [`tsgo_astnav::NavEngine`]
//! shared-borrow navigation context and returns the [`tsgo_core::text::TextChange`]
//! edits.
//!
//! # Divergence from Go's context plumbing
//!
//! Go threads options + newline through a `context.Context`
//! (`WithFormatCodeSettings` / `GetFormatCodeSettingsFromContext` /
//! `GetNewLineOrDefaultFromContext`). These functions instead take the
//! [`crate::format_code_settings::FormatCodeSettings`] explicitly; the newline is
//! resolved from `options.editor.new_line_character` (else `"\n"`) inside
//! [`crate::span::FormatContext`].
//!
//! # Deferred (blocked-by)
//!
//! - `FormatOnEnter` / `FormatOnOpeningCurly` (large; need
//!   `GetECMAEndLinePosition` selection math / `findImmediatelyPrecedingTokenOfKind`
//!   on `{` plus list-level widening that is reachable only via the deferred
//!   list-scope handling). `FormatSelection`/`FormatNodeGivenIndentation` are
//!   thin wrappers deferred with them.

use std::borrow::Borrow;

use tsgo_ast::{NodeArena, NodeId};
use tsgo_astnav::{get_start_of_node, NavEngine};
use tsgo_core::text::TextRange;
use tsgo_core::textchange::TextChange;

use crate::context::FormatRequestKind;
use crate::format_code_settings::FormatCodeSettings;
use crate::indent::get_indentation_for_node;
use crate::span::{
    find_enclosing_node, get_own_or_inherited_delta, get_scan_start_position, FormatContext,
    FormatSpanWorker,
};
use crate::util::{
    find_immediately_preceding_token_of_kind, find_outermost_node_within_list_level,
    get_line_start_position_for_position,
};

/// Formats the whole source file (Go's `FormatDocument`).
///
/// # Examples
/// ```
/// use tsgo_astnav::NavSourceFile;
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_format::api::format_document;
/// use tsgo_format::format_code_settings::get_default_format_code_settings;
/// use tsgo_core::textchange::apply_bulk_edits;
/// let text = "[1,2,3]";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let nav = NavSourceFile::from_borrowed_arena(&r.arena, r.source_file, text.to_string());
/// let edits = format_document(&nav, &get_default_format_code_settings());
/// assert_eq!(apply_bulk_edits(text, &edits), "[1, 2, 3]");
/// ```
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/api.go:FormatDocument
pub fn format_document<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    options: &FormatCodeSettings,
) -> Vec<TextChange> {
    let span = TextRange::new(0, file.end(file.root()));
    format_span(file, span, options, FormatRequestKind::FormatDocument)
}

/// Formats a span of the source file (Go's `FormatSpan`).
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/api.go:FormatSpan
pub fn format_span<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    span: TextRange,
    options: &FormatCodeSettings,
    kind: FormatRequestKind,
) -> Vec<TextChange> {
    let ctx = FormatContext::new(file, options.clone(), kind);
    let enclosing_node = find_enclosing_node(file, span);
    let initial_indentation =
        get_indentation_for_node(file, &ctx.line_starts, enclosing_node, Some(span), options);
    let delta = get_own_or_inherited_delta(file, &ctx.line_starts, options, enclosing_node);
    let scan_start = get_scan_start_position(file, enclosing_node, span);
    let mut worker = FormatSpanWorker::new(
        &ctx,
        span,
        enclosing_node,
        initial_indentation,
        delta,
        scan_start,
    );
    worker.run()
}

/// Formats the lines spanned by `node` (Go's `formatNodeLines`).
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/api.go:formatNodeLines
fn format_node_lines<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    node: Option<NodeId>,
    request_kind: FormatRequestKind,
    options: &FormatCodeSettings,
) -> Vec<TextChange> {
    let node = match node {
        Some(n) => n,
        None => return Vec::new(),
    };
    let token_start = get_start_of_node(file, node, false);
    let line_starts = tsgo_core::compute_ecma_line_starts(file.text());
    let line_start = get_line_start_position_for_position(&line_starts, token_start);
    let span = TextRange::new(line_start, file.end(node));
    format_span(file, span, options, request_kind)
}

/// Formats after a typed `;` at `position` (Go's `FormatOnSemicolon`).
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/api.go:FormatOnSemicolon
pub fn format_on_semicolon<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    position: i32,
    options: &FormatCodeSettings,
) -> Vec<TextChange> {
    let semicolon =
        find_immediately_preceding_token_of_kind(file, position, tsgo_ast::Kind::SemicolonToken);
    let outermost = semicolon.map(|s| find_outermost_node_within_list_level(file, s));
    format_node_lines(
        file,
        outermost,
        FormatRequestKind::FormatOnSemicolon,
        options,
    )
}

/// Formats after a typed `}` at `position` (Go's `FormatOnClosingCurly`).
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/api.go:FormatOnClosingCurly
pub fn format_on_closing_curly<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    position: i32,
    options: &FormatCodeSettings,
) -> Vec<TextChange> {
    let preceding_token =
        find_immediately_preceding_token_of_kind(file, position, tsgo_ast::Kind::CloseBraceToken);
    let outermost = preceding_token.map(|t| find_outermost_node_within_list_level(file, t));
    format_node_lines(
        file,
        outermost,
        FormatRequestKind::FormatOnClosingCurlyBrace,
        options,
    )
}

#[cfg(test)]
#[path = "api_test.rs"]
mod tests;
