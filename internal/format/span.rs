//! The AST-walking formatting span worker (`formatSpanWorker`) + the pure
//! indentation-string helper.
//!
//! 1:1 port of the reachable subset of Go `internal/format/span.go`. The worker
//! walks the AST over a range, consults the round-1 rules engine
//! ([`crate::rulesmap::get_rules`]) between adjacent tokens, and emits
//! [`tsgo_core::text::TextChange`] edits (insert/delete space, newline, indent).
//!
//! # Child traversal divergence (documented)
//!
//! Go drives the walk with `ast.NewNodeVisitor`, whose `VisitNodes` hook
//! distinguishes node *lists* (handled by `processChildNodes`, which opens a
//! list indentation scope around the list start/end tokens) from single children
//! (`processChildNode`). The `tsgo_astnav` shared surface exposes only the flat
//! `visit_each_child_and_jsdoc` stream (its own docs note the callers "replace the
//! per-list binary search with a linear scan over the same (sorted) children —
//! same result, without the list/single distinction"). So this port collapses
//! the two into [`FormatSpanWorker::process_child_node`] over the flat children:
//! the list start/end tokens, commas, and close brackets are consumed as ordinary
//! "parent tokens" between children (exactly how Go consumes non-list tokens),
//! which produces identical spacing/indent edits for the reachable cases. The
//! multi-line list **continuation** indent (`processChildNodes`' list scope +
//! `tryComputeIndentationForListItem`) is deferred (see worklog).
//!
//! # Other deferrals (blocked-by)
//!
//! - Decorator handling (`getNonDecoratorTokenPosOfNode`, the
//!   undecorated-start-line and `getFirstNonDecoratorTokenOfNode` paths):
//!   `has_decorators` is treated as `false` (no modifier-list accessors yet, and
//!   no decorators in the reachable set), so undecorated start lines equal start
//!   lines.
//! - Comment re-indentation (`indentMultilineComment`) and the format-selection
//!   "remaining leading trivia" tail in `execute`.
//! - `rangeContainsError`: the nav context carries no diagnostics, so the worker
//!   treats every range as error-free (well-formed input has no errors).

use std::borrow::Borrow;

use tsgo_ast::utilities::node_is_missing;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId};
use tsgo_astnav::{get_start_of_node, visit_each_child_and_jsdoc, NavEngine};
use tsgo_core::languagevariant::LanguageVariant;
use tsgo_core::text::{TextPos, TextRange};
use tsgo_core::textchange::TextChange;
use tsgo_scanner::compute_line_of_position;

use crate::context::{FormatRequestKind, FormattingContext};
use crate::format_code_settings::FormatCodeSettings;
use crate::indent::{
    argument_starts_on_same_line_as_previous_argument,
    child_is_unindented_branch_of_conditional_expression,
    child_starts_on_the_same_line_with_else_in_if_statement, node_will_indent_child,
    should_indent_child_node,
};
use crate::rule::{RuleAction, RuleFlags, RuleImpl};
use crate::rulesmap::get_rules;
use crate::scanner::{FormattingScanner, TextRangeWithKind, TokenInfo, TokenRescanContext};
use crate::util::{is_grammar_error, node_parent, range_is_on_one_line, with_token_start};

/// Renders the indentation prefix string for `indentation` columns under
/// `options`.
///
/// When `convert_tabs_to_spaces` is true, emits that many spaces. Otherwise it
/// emits as many tabs as fit (`indentation / tab_size`) followed by the
/// remainder in spaces; a `tab_size` of 0 yields the empty string. Mirrors Go's
/// `getIndentationString`.
///
/// # Examples
/// ```
/// use tsgo_format::format_code_settings::get_default_format_code_settings;
/// use tsgo_format::span::get_indentation_string;
/// let opts = get_default_format_code_settings(); // spaces
/// assert_eq!(get_indentation_string(4, &opts), "    ");
/// ```
///
/// Side effects: none (pure).
// Go: internal/format/span.go:getIndentationString
pub fn get_indentation_string(indentation: i32, options: &FormatCodeSettings) -> String {
    if !options.editor.convert_tabs_to_spaces.is_true() {
        if options.editor.tab_size == 0 {
            return String::new();
        }
        let indentation = indentation.max(0);
        let tabs = indentation / options.editor.tab_size;
        let spaces = indentation - (tabs * options.editor.tab_size);
        let mut res = "\t".repeat(tabs as usize);
        if spaces > 0 {
            res.push_str(&" ".repeat(spaces as usize));
        }
        res
    } else {
        " ".repeat(indentation.max(0) as usize)
    }
}

/// The action a rule had on the relative line of two tokens.
///
/// Mirrors Go's `LineAction` iota.
///
/// Side effects: none (pure value type).
// Go: internal/format/span.go:LineAction
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineAction {
    /// No change to the relative line of the two tokens (`LineActionNone`).
    None,
    /// The next token was moved to a new line (`LineActionLineAdded`).
    LineAdded,
    /// The next token was joined onto this line (`LineActionLineRemoved`).
    LineRemoved,
}

/// The immutable per-span context shared by every worker method.
///
/// Holding the navigation context, source text, line map, options, and resolved
/// newline behind one `&Ctx` lets each method copy the shared reference
/// (`let ctx = self.ctx;`) and then freely mutate the worker's own state without
/// fighting the borrow checker.
///
/// Side effects: none (immutable).
pub struct FormatContext<'a, A: Borrow<NodeArena>> {
    /// The shared-borrow navigation context over the parsed source file.
    pub file: &'a NavEngine<A>,
    /// The source text (`file.text()`).
    pub text: &'a str,
    /// The ECMAScript line-start byte offsets (Go caches this on the file).
    pub line_starts: Vec<TextPos>,
    /// The formatter options.
    pub options: FormatCodeSettings,
    /// The resolved newline string (`GetNewLineOrDefaultFromContext`).
    pub newline: String,
    /// The active formatting request kind.
    pub request_kind: FormatRequestKind,
}

impl<'a, A: Borrow<NodeArena>> FormatContext<'a, A> {
    /// Builds the per-span context from a navigation context + options.
    ///
    /// The newline is resolved like Go's `GetNewLineOrDefaultFromContext`: the
    /// option's `new_line_character` if set, otherwise `"\n"`.
    ///
    /// Side effects: computes the line map.
    pub fn new(
        file: &'a NavEngine<A>,
        options: FormatCodeSettings,
        request_kind: FormatRequestKind,
    ) -> FormatContext<'a, A> {
        let text = file.text();
        let line_starts = tsgo_core::compute_ecma_line_starts(text);
        let newline = if options.editor.new_line_character.is_empty() {
            "\n".to_string()
        } else {
            options.editor.new_line_character.clone()
        };
        FormatContext {
            file,
            text,
            line_starts,
            options,
            newline,
            request_kind,
        }
    }
}

/// A node's indentation context, mirroring Go's `dynamicIndenter`.
///
/// Side effects: none on its own; `recompute_indentation` mutates it.
// Go: internal/format/span.go:dynamicIndenter
#[derive(Clone, Copy, Debug)]
struct DynamicIndenter {
    node: NodeId,
    node_start_line: i32,
    indentation: i32,
    delta: i32,
}

impl DynamicIndenter {
    /// The node's base indentation.
    // Go: internal/format/span.go:dynamicIndenter.getIndentation
    fn get_indentation(&self) -> i32 {
        self.indentation
    }

    /// The node's delta, suppressed to zero when the node won't indent `child`.
    // Go: internal/format/span.go:dynamicIndenter.getDelta
    fn get_delta<A: Borrow<NodeArena>>(
        &self,
        file: &NavEngine<A>,
        line_starts: &[TextPos],
        options: &FormatCodeSettings,
        child: Option<NodeId>,
    ) -> i32 {
        if node_will_indent_child(options, file, line_starts, true, self.node, child, true) {
            self.delta
        } else {
            0
        }
    }

    /// The indentation a token should receive, possibly adding the delta.
    // Go: internal/format/span.go:dynamicIndenter.getIndentationForToken
    #[allow(clippy::too_many_arguments)]
    fn get_indentation_for_token<A: Borrow<NodeArena>>(
        &self,
        file: &NavEngine<A>,
        line_starts: &[TextPos],
        options: &FormatCodeSettings,
        line: i32,
        kind: Kind,
        container: NodeId,
        suppress_delta: bool,
    ) -> i32 {
        if !suppress_delta && self.should_add_delta(file, line, kind, container) {
            self.indentation + self.get_delta(file, line_starts, options, Some(container))
        } else {
            self.indentation
        }
    }

    /// The indentation a leading comment should receive.
    // Go: internal/format/span.go:dynamicIndenter.getIndentationForComment
    fn get_indentation_for_comment<A: Borrow<NodeArena>>(
        &self,
        file: &NavEngine<A>,
        line_starts: &[TextPos],
        options: &FormatCodeSettings,
        kind: Kind,
        token_indentation: i32,
        container: NodeId,
    ) -> i32 {
        match kind {
            Kind::CloseBraceToken | Kind::CloseBracketToken | Kind::CloseParenToken => {
                self.indentation + self.get_delta(file, line_starts, options, Some(container))
            }
            _ => {
                if token_indentation != -1 {
                    token_indentation
                } else {
                    self.indentation
                }
            }
        }
    }

    /// Whether the token's indentation should include the node's delta.
    // Go: internal/format/span.go:dynamicIndenter.shouldAddDelta
    fn should_add_delta<A: Borrow<NodeArena>>(
        &self,
        file: &NavEngine<A>,
        line: i32,
        kind: Kind,
        container: NodeId,
    ) -> bool {
        match kind {
            Kind::OpenBraceToken
            | Kind::CloseBraceToken
            | Kind::CloseParenToken
            | Kind::ElseKeyword
            | Kind::WhileKeyword
            | Kind::AtToken => return false,
            Kind::SlashToken | Kind::GreaterThanToken => {
                if matches!(
                    file.kind(container),
                    Kind::JsxOpeningElement | Kind::JsxClosingElement | Kind::JsxSelfClosingElement
                ) {
                    return false;
                }
            }
            Kind::OpenBracketToken | Kind::CloseBracketToken => {
                if file.kind(container) != Kind::MappedType {
                    return false;
                }
            }
            _ => {}
        }
        // The token is on a later line than the node start (so it is not the
        // node's first token). DEFER(phase-7): the decorator-first-token
        // suppression (`!(HasDecorators(node) && kind == firstNonDecoratorToken)`)
        // is omitted — `has_decorators` is false across the reachable set.
        self.node_start_line != line
    }

    /// Re-derives indentation after a rule moved the next token onto a new (or
    /// previous) line.
    // Go: internal/format/span.go:dynamicIndenter.recomputeIndentation
    fn recompute_indentation<A: Borrow<NodeArena>>(
        &mut self,
        file: &NavEngine<A>,
        line_starts: &[TextPos],
        options: &FormatCodeSettings,
        line_added: bool,
        parent: NodeId,
    ) {
        if should_indent_child_node(
            options,
            file,
            line_starts,
            true,
            parent,
            Some(self.node),
            false,
        ) {
            if line_added {
                self.indentation += options.editor.indent_size;
            } else {
                self.indentation -= options.editor.indent_size;
            }
            if node_will_indent_child(options, file, line_starts, false, self.node, None, false) {
                self.delta = options.editor.indent_size;
            } else {
                self.delta = 0;
            }
        }
    }
}

/// Collects the flat children of `node` (JSDoc first), mirroring how Go's
/// visitor reaches every child (single + list elements in source order).
///
/// Side effects: none beyond allocating the result.
fn children<A: Borrow<NodeArena>>(file: &NavEngine<A>, node: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    visit_each_child_and_jsdoc(file, node, &mut |c| out.push(c));
    out
}

/// Builds the rescan context (`n.Kind` + `n.Parent.Kind`) for a node.
///
/// Side effects: none (reads the arena).
fn rescan_ctx<A: Borrow<NodeArena>>(file: &NavEngine<A>, node: NodeId) -> TokenRescanContext {
    TokenRescanContext {
        node_kind: file.kind(node),
        node_parent_kind: file.arena().parent(node).map(|p| file.kind(p)),
    }
}

/// Reports whether `kind` is a comment trivia.
///
/// Side effects: none (pure).
// Go: internal/format/span.go:isComment
fn is_comment(kind: Kind) -> bool {
    kind == Kind::SingleLineCommentTrivia || kind == Kind::MultiLineCommentTrivia
}

/// Reports whether `kind` is a string/regex/template literal.
///
/// Side effects: none (pure).
// Go: internal/format/span.go:isStringOrRegularExpressionOrTemplateLiteral
fn is_string_or_regular_expression_or_template_literal(kind: Kind) -> bool {
    kind == Kind::StringLiteral
        || kind == Kind::RegularExpressionLiteral
        || matches!(
            kind,
            Kind::NoSubstitutionTemplateLiteral
                | Kind::TemplateHead
                | Kind::TemplateMiddle
                | Kind::TemplateTail
        )
}

/// Finds the node that fully contains `r`.
///
/// Mirrors Go's `findEnclosingNode`.
///
/// Side effects: none (reads the nav context).
// Go: internal/format/span.go:findEnclosingNode
pub fn find_enclosing_node<A: Borrow<NodeArena>>(file: &NavEngine<A>, r: TextRange) -> NodeId {
    fn find<A: Borrow<NodeArena>>(file: &NavEngine<A>, r: TextRange, n: NodeId) -> NodeId {
        let mut candidate: Option<NodeId> = None;
        for c in children(file, n) {
            if file.arena().flags(c).contains(NodeFlags::REPARSED) {
                continue;
            }
            if r.contained_by(with_token_start(file, c)) {
                candidate = Some(c);
                break;
            }
        }
        if let Some(cand) = candidate {
            return find(file, r, cand);
        }
        n
    }
    find(file, r, file.root())
}

/// Computes the scanner start position for a span (Go's `getScanStartPosition`).
///
/// Side effects: may synthesize tokens via the nav context.
// Go: internal/format/span.go:getScanStartPosition
pub fn get_scan_start_position<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    enclosing_node: NodeId,
    original_range: TextRange,
) -> i32 {
    let adjusted = with_token_start(file, enclosing_node);
    let start = adjusted.pos();
    if start == original_range.pos() && file.end(enclosing_node) == original_range.end() {
        return start;
    }
    match file.find_preceding_token(original_range.pos()) {
        None => file.pos(enclosing_node),
        Some(preceding_token) => {
            if file.end(preceding_token) >= original_range.pos() {
                file.pos(enclosing_node)
            } else {
                file.end(preceding_token)
            }
        }
    }
}

/// Computes the own-or-inherited indentation delta for a span's enclosing node.
///
/// Mirrors Go's `getOwnOrInheritedDelta`.
///
/// Side effects: none (reads the nav context).
// Go: internal/format/span.go:getOwnOrInheritedDelta
pub fn get_own_or_inherited_delta<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    options: &FormatCodeSettings,
    n: NodeId,
) -> i32 {
    let mut previous_line = -1i32;
    let mut child: Option<NodeId> = None;
    let mut cur = Some(n);
    while let Some(node) = cur {
        let line = compute_line_of_position(line_starts, get_start_of_node(file, node, false));
        if previous_line != -1 && line != previous_line {
            break;
        }
        if should_indent_child_node(options, file, line_starts, true, node, child, false) {
            return options.editor.indent_size;
        }
        previous_line = line;
        child = Some(node);
        cur = file.arena().parent(node);
    }
    0
}

/// The language variant of `file`'s source file (read off the root node).
///
/// Side effects: none (reads the arena).
pub fn language_variant_of<A: Borrow<NodeArena>>(file: &NavEngine<A>) -> LanguageVariant {
    match file.arena().data(file.root()) {
        NodeData::SourceFile(d) => d.language_variant,
        _ => LanguageVariant::Standard,
    }
}

/// The AST-walking formatting worker.
///
/// Side effects: scans tokens, reads the nav context, accumulates edits.
// Go: internal/format/span.go:formatSpanWorker
pub struct FormatSpanWorker<'c, 'a, A: Borrow<NodeArena>> {
    ctx: &'c FormatContext<'a, A>,
    original_range: TextRange,
    enclosing_node: NodeId,
    initial_indentation: i32,
    delta: i32,

    scanner: FormattingScanner,
    fcx: FormattingContext,

    edits: Vec<TextChange>,
    previous_range: TextRangeWithKind,
    previous_range_trivia_end: i32,
    previous_parent: Option<NodeId>,
    previous_range_start_line: i32,

    child_context_node: Option<NodeId>,
    last_indented_line: i32,
    indentation_on_last_indented_line: i32,
}

impl<'c, 'a, A: Borrow<NodeArena>> FormatSpanWorker<'c, 'a, A> {
    /// Builds a worker over `original_range`, scanning from `scan_start`.
    ///
    /// Side effects: allocates the formatting scanner.
    // Go: internal/format/span.go:newFormatSpanWorker
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ctx: &'c FormatContext<'a, A>,
        original_range: TextRange,
        enclosing_node: NodeId,
        initial_indentation: i32,
        delta: i32,
        scan_start: i32,
    ) -> FormatSpanWorker<'c, 'a, A> {
        let scanner = FormattingScanner::new(
            ctx.text,
            language_variant_of(ctx.file),
            scan_start,
            original_range.end(),
        );
        let fcx = FormattingContext::new(ctx.options.clone(), ctx.request_kind);
        FormatSpanWorker {
            ctx,
            original_range,
            enclosing_node,
            initial_indentation,
            delta,
            scanner,
            fcx,
            edits: Vec::new(),
            previous_range: TextRangeWithKind::empty(),
            previous_range_trivia_end: 0,
            previous_parent: None,
            previous_range_start_line: 0,
            child_context_node: None,
            last_indented_line: -1,
            indentation_on_last_indented_line: -1,
        }
    }

    /// Returns the 0-based line containing `pos`.
    fn line_of(&self, pos: i32) -> i32 {
        compute_line_of_position(&self.ctx.line_starts, pos)
    }

    /// The diagnostics-overlap predicate.
    ///
    /// DEFER(phase-7): the nav context carries no diagnostics, so every range is
    /// treated as error-free (well-formed input has no errors).
    /// blocked-by: diagnostics threading through the shared nav context.
    fn range_contains_error(&self, _r: TextRange) -> bool {
        false
    }

    /// Runs the worker, returning the accumulated edits.
    ///
    /// Mirrors Go's `formatSpanWorker.execute` (the `formattingScanner` setup is
    /// folded into the worker; see the [`crate::scanner`] note).
    ///
    /// Side effects: scans tokens; accumulates edits.
    // Go: internal/format/span.go:formatSpanWorker.execute
    pub fn run(&mut self) -> Vec<TextChange> {
        let ctx = self.ctx;
        self.indentation_on_last_indented_line = -1;
        self.last_indented_line = -1;

        self.scanner.advance();

        if self.scanner.is_on_token() {
            let start_line = self.line_of(get_start_of_node(ctx.file, self.enclosing_node, false));
            // DEFER(phase-7): undecorated start line == start line (no decorators).
            let undecorated_start_line = start_line;
            self.process_node(
                self.enclosing_node,
                self.enclosing_node,
                start_line,
                undecorated_start_line,
                self.initial_indentation,
                self.delta,
            );
        }

        // DEFER(phase-7): the format-selection "remaining leading trivia" tail
        // (only relevant when a selection ends mid-trivia).

        if self.previous_range != TextRangeWithKind::empty()
            && self.scanner.get_token_full_start() >= self.original_range.end()
        {
            let token_info: Option<TextRangeWithKind> = if self.scanner.is_on_eof() {
                Some(self.scanner.read_eof_token_range())
            } else if self.scanner.is_on_token() {
                let rc = rescan_ctx(ctx.file, self.enclosing_node);
                Some(self.scanner.read_token_info(rc).token)
            } else {
                None
            };

            if let Some(token_info) = token_info {
                if token_info.loc.pos() == self.previous_range_trivia_end {
                    let mut parent = ctx
                        .file
                        .find_preceding_token(token_info.loc.end())
                        .and_then(|pt| node_parent(ctx.file, pt));
                    if parent.is_none() {
                        parent = self.previous_parent;
                    }
                    if let (Some(parent), Some(previous_parent)) = (parent, self.previous_parent) {
                        let line = self.line_of(token_info.loc.pos());
                        self.process_pair(
                            token_info,
                            line,
                            parent,
                            self.previous_range,
                            self.previous_range_start_line,
                            previous_parent,
                            parent,
                            None,
                        );
                    }
                }
            }
        }

        std::mem::take(&mut self.edits)
    }

    /// Processes `node` and the tokens it owns (Go's `processNode`).
    ///
    /// Side effects: scans tokens; accumulates edits.
    // Go: internal/format/span.go:formatSpanWorker.processNode
    fn process_node(
        &mut self,
        node: NodeId,
        context_node: NodeId,
        node_start_line: i32,
        undecorated_node_start_line: i32,
        indentation: i32,
        delta: i32,
    ) {
        let ctx = self.ctx;
        if !self
            .original_range
            .overlaps(with_token_start(ctx.file, node))
        {
            return;
        }

        let mut node_dynamic_indentation = DynamicIndenter {
            node,
            node_start_line,
            indentation,
            delta,
        };

        self.child_context_node = Some(context_node);

        for child in children(ctx.file, node) {
            self.process_child_node(
                node,
                child,
                -1,
                node,
                &mut node_dynamic_indentation,
                node_start_line,
                undecorated_node_start_line,
                false,
                false,
            );
        }

        let node_end = ctx.file.end(node);
        while self.scanner.is_on_token()
            && self.scanner.get_token_full_start() < self.original_range.end()
        {
            let rc = rescan_ctx(ctx.file, node);
            let token_info = self.scanner.read_token_info(rc);
            if token_info.token.loc.end() > node_end.min(self.original_range.end()) {
                break;
            }
            self.consume_token_and_advance_scanner(
                token_info,
                node,
                &mut node_dynamic_indentation,
                node,
                false,
            );
        }
    }

    /// Processes a single child node (Go's `processChildNode`, with the
    /// list-scope handling collapsed away; see the module docs).
    ///
    /// Side effects: scans tokens; accumulates edits.
    // Go: internal/format/span.go:formatSpanWorker.processChildNode
    #[allow(clippy::too_many_arguments)]
    fn process_child_node(
        &mut self,
        node: NodeId,
        child: NodeId,
        mut inherited_indentation: i32,
        parent: NodeId,
        parent_dynamic_indentation: &mut DynamicIndenter,
        _parent_start_line: i32,
        undecorated_parent_start_line: i32,
        _is_list_item: bool,
        is_first_list_item: bool,
    ) -> i32 {
        let ctx = self.ctx;

        if node_is_missing(ctx.file.arena(), child)
            || is_grammar_error(ctx.file, parent, child)
            || ctx.file.arena().flags(child).contains(NodeFlags::REPARSED)
        {
            return inherited_indentation;
        }

        let child_start_pos = get_start_of_node(ctx.file, child, false);
        let child_start_line = self.line_of(child_start_pos);
        // DEFER(phase-7): undecorated child start line == child start line.
        let undecorated_child_start_line = child_start_line;

        // DEFER(phase-7): list-item indentation (`tryComputeIndentationForListItem`)
        // is part of the deferred multi-line list-scope handling.
        let child_indentation_amount = -1;

        let child_loc = ctx.file.arena().loc(child);
        if !self.original_range.overlaps(child_loc) {
            if ctx.file.end(child) < self.original_range.pos() {
                self.scanner.skip_to_end_of(child_loc);
            }
            return inherited_indentation;
        }

        if child_loc.is_empty() {
            return inherited_indentation;
        }

        while self.scanner.is_on_token()
            && self.scanner.get_token_full_start() < self.original_range.end()
        {
            let rc = rescan_ctx(ctx.file, node);
            let token_info = self.scanner.read_token_info(rc);
            if token_info.token.loc.end() > self.original_range.end() {
                return inherited_indentation;
            }
            if token_info.token.loc.end() > child_start_pos {
                if token_info.token.loc.pos() > child_start_pos {
                    self.scanner.skip_to_start_of(ctx.file.arena().loc(child));
                }
                break;
            }
            self.consume_token_and_advance_scanner(
                token_info,
                node,
                parent_dynamic_indentation,
                node,
                false,
            );
        }

        if !self.scanner.is_on_token()
            || self.scanner.get_token_full_start() >= self.original_range.end()
        {
            return inherited_indentation;
        }

        if tsgo_ast::utilities::is_token_kind(ctx.file.kind(child)) {
            let rc = rescan_ctx(ctx.file, child);
            let token_info = self.scanner.read_token_info(rc);
            if ctx.file.kind(child) != Kind::JsxText {
                debug_assert_eq!(
                    token_info.token.loc.end(),
                    ctx.file.end(child),
                    "Token end is child end"
                );
                self.consume_token_and_advance_scanner(
                    token_info,
                    node,
                    parent_dynamic_indentation,
                    child,
                    false,
                );
                return inherited_indentation;
            }
        }

        let effective_parent_start_line = if ctx.file.kind(child) == Kind::Decorator {
            child_start_line
        } else {
            undecorated_parent_start_line
        };
        let (child_indentation, delta) = self.compute_indentation(
            child,
            child_start_line,
            child_indentation_amount,
            node,
            parent_dynamic_indentation,
            effective_parent_start_line,
        );

        let child_context = self.child_context_node.unwrap_or(node);
        self.process_node(
            child,
            child_context,
            child_start_line,
            undecorated_child_start_line,
            child_indentation,
            delta,
        );

        self.child_context_node = Some(node);

        if is_first_list_item
            && ctx.file.kind(parent) == Kind::ArrayLiteralExpression
            && inherited_indentation == -1
        {
            inherited_indentation = child_indentation;
        }

        inherited_indentation
    }

    /// Computes a child's indentation + delta (Go's `computeIndentation`).
    ///
    /// Side effects: none (reads the nav context).
    // Go: internal/format/span.go:formatSpanWorker.computeIndentation
    fn compute_indentation(
        &self,
        node: NodeId,
        start_line: i32,
        inherited_indentation: i32,
        parent: NodeId,
        parent_dynamic_indentation: &DynamicIndenter,
        effective_parent_start_line: i32,
    ) -> (i32, i32) {
        let ctx = self.ctx;
        let mut delta = 0;
        if should_indent_child_node(
            &ctx.options,
            ctx.file,
            &ctx.line_starts,
            false,
            node,
            None,
            false,
        ) {
            delta = ctx.options.editor.indent_size;
        }

        if effective_parent_start_line == start_line {
            let mut indentation = self.indentation_on_last_indented_line;
            if start_line != self.last_indented_line {
                indentation = parent_dynamic_indentation.get_indentation();
            }
            delta = ctx.options.editor.indent_size.min(
                parent_dynamic_indentation.get_delta(
                    ctx.file,
                    &ctx.line_starts,
                    &ctx.options,
                    Some(node),
                ) + delta,
            );
            return (indentation, delta);
        } else if inherited_indentation == -1 {
            if ctx.file.kind(node) == Kind::OpenParenToken && start_line == self.last_indented_line
            {
                return (
                    self.indentation_on_last_indented_line,
                    parent_dynamic_indentation.get_delta(
                        ctx.file,
                        &ctx.line_starts,
                        &ctx.options,
                        Some(node),
                    ),
                );
            } else if child_starts_on_the_same_line_with_else_in_if_statement(
                ctx.file,
                &ctx.line_starts,
                parent,
                node,
                start_line,
            ) || child_is_unindented_branch_of_conditional_expression(
                ctx.file,
                &ctx.line_starts,
                parent,
                node,
                start_line,
            ) || argument_starts_on_same_line_as_previous_argument(
                ctx.file,
                &ctx.line_starts,
                parent,
                node,
                start_line,
            ) {
                return (parent_dynamic_indentation.get_indentation(), delta);
            } else {
                let i = parent_dynamic_indentation.get_indentation();
                if i == -1 {
                    return (parent_dynamic_indentation.get_indentation(), delta);
                }
                return (
                    i + parent_dynamic_indentation.get_delta(
                        ctx.file,
                        &ctx.line_starts,
                        &ctx.options,
                        Some(node),
                    ),
                    delta,
                );
            }
        }

        (inherited_indentation, delta)
    }

    /// Updates the formatting context and applies the matching rules' edits to a
    /// pair of adjacent tokens (Go's `processPair`).
    ///
    /// Side effects: accumulates edits; may recompute `dynamic_indentation`.
    // Go: internal/format/span.go:formatSpanWorker.processPair
    #[allow(clippy::too_many_arguments)]
    fn process_pair(
        &mut self,
        current_item: TextRangeWithKind,
        current_start_line: i32,
        current_parent: NodeId,
        previous_item: TextRangeWithKind,
        previous_start_line: i32,
        previous_parent: NodeId,
        context_node: NodeId,
        mut dynamic_indentation: Option<&mut DynamicIndenter>,
    ) -> LineAction {
        let ctx = self.ctx;
        self.update_formatting_context(
            previous_item,
            previous_parent,
            current_item,
            current_parent,
            context_node,
        );

        let rules = get_rules(&self.fcx, Vec::new());

        let mut trim_trailing_whitespaces = !ctx.options.editor.trim_trailing_whitespace.is_false();
        let mut line_action = LineAction::None;

        if !rules.is_empty() {
            for i in (0..rules.len()).rev() {
                let rule: &RuleImpl = &rules[i];
                line_action = self.apply_rule_edits(
                    rule,
                    previous_item,
                    previous_start_line,
                    current_item,
                    current_start_line,
                );
                if let Some(di) = dynamic_indentation.as_deref_mut() {
                    match line_action {
                        LineAction::LineRemoved => {
                            if get_start_of_node(ctx.file, current_parent, false)
                                == current_item.loc.pos()
                            {
                                di.recompute_indentation(
                                    ctx.file,
                                    &ctx.line_starts,
                                    &ctx.options,
                                    false,
                                    context_node,
                                );
                            }
                        }
                        LineAction::LineAdded => {
                            if get_start_of_node(ctx.file, current_parent, false)
                                == current_item.loc.pos()
                            {
                                di.recompute_indentation(
                                    ctx.file,
                                    &ctx.line_starts,
                                    &ctx.options,
                                    true,
                                    context_node,
                                );
                            }
                        }
                        LineAction::None => {}
                    }
                }
                trim_trailing_whitespaces = trim_trailing_whitespaces
                    && !rule.action().intersects(RuleAction::DELETE_SPACE)
                    && rule.flags() != RuleFlags::CanDeleteNewLines;
            }
        } else {
            trim_trailing_whitespaces =
                trim_trailing_whitespaces && current_item.kind != Kind::EndOfFile;
        }

        if current_start_line != previous_start_line && trim_trailing_whitespaces {
            self.trim_trailing_whitespaces_for_lines(
                previous_start_line,
                current_start_line,
                previous_item,
            );
        }

        line_action
    }

    /// Records the edits a single rule produces (Go's `applyRuleEdits`).
    ///
    /// Side effects: accumulates edits.
    // Go: internal/format/span.go:formatSpanWorker.applyRuleEdits
    fn apply_rule_edits(
        &mut self,
        rule: &RuleImpl,
        previous_range: TextRangeWithKind,
        previous_start_line: i32,
        current_range: TextRangeWithKind,
        current_start_line: i32,
    ) -> LineAction {
        let ctx = self.ctx;
        let on_later_line = current_start_line != previous_start_line;
        let action = rule.action();
        if action.intersects(RuleAction::STOP_PROCESSING_SPACE_ACTIONS)
            || action.intersects(RuleAction::STOP_PROCESSING_TOKEN_ACTIONS)
        {
            // ruleActionStopProcessingSpaceActions: no edit.
            return LineAction::None;
        }
        if action.intersects(RuleAction::DELETE_SPACE) {
            if previous_range.loc.end() != current_range.loc.pos() {
                self.record_delete(
                    previous_range.loc.end(),
                    current_range.loc.pos() - previous_range.loc.end(),
                );
                if on_later_line {
                    return LineAction::LineRemoved;
                }
                return LineAction::None;
            }
        } else if action.intersects(RuleAction::DELETE_TOKEN) {
            self.record_delete(previous_range.loc.pos(), previous_range.loc.len());
        } else if action.intersects(RuleAction::INSERT_NEW_LINE) {
            if rule.flags() != RuleFlags::CanDeleteNewLines
                && previous_start_line != current_start_line
            {
                return LineAction::None;
            }
            let line_delta = current_start_line - previous_start_line;
            if line_delta != 1 {
                let newline = ctx.newline.clone();
                self.record_replace(
                    previous_range.loc.end(),
                    current_range.loc.pos() - previous_range.loc.end(),
                    &newline,
                );
                if on_later_line {
                    return LineAction::None;
                }
                return LineAction::LineAdded;
            }
        } else if action.intersects(RuleAction::INSERT_SPACE) {
            if rule.flags() != RuleFlags::CanDeleteNewLines
                && previous_start_line != current_start_line
            {
                return LineAction::None;
            }
            let pos_delta = current_range.loc.pos() - previous_range.loc.end();
            let already_single_space =
                pos_delta == 1 && ctx.text[previous_range.loc.end() as usize..].starts_with(' ');
            if !already_single_space {
                self.record_replace(previous_range.loc.end(), pos_delta, " ");
                if on_later_line {
                    return LineAction::LineRemoved;
                }
                return LineAction::None;
            }
        } else if action.intersects(RuleAction::INSERT_TRAILING_SEMICOLON) {
            self.record_insert(previous_range.loc.end(), ";");
        }
        LineAction::None
    }

    /// Processes a token/comment range (Go's `processRange`).
    ///
    /// Side effects: accumulates edits.
    // Go: internal/format/span.go:formatSpanWorker.processRange
    fn process_range(
        &mut self,
        r: TextRangeWithKind,
        range_start_line: i32,
        _range_start_character: i32,
        parent: NodeId,
        context_node: NodeId,
        dynamic_indentation: Option<&mut DynamicIndenter>,
    ) -> LineAction {
        let range_has_error = self.range_contains_error(r.loc);
        let mut line_action = LineAction::None;
        if !range_has_error {
            if self.previous_range == TextRangeWithKind::empty() {
                let original_start_line = self.line_of(self.original_range.pos());
                self.trim_trailing_whitespaces_for_lines(
                    original_start_line,
                    range_start_line,
                    TextRangeWithKind::empty(),
                );
            } else {
                let previous_parent = self.previous_parent.expect("previous parent set");
                line_action = self.process_pair(
                    r,
                    range_start_line,
                    parent,
                    self.previous_range,
                    self.previous_range_start_line,
                    previous_parent,
                    context_node,
                    dynamic_indentation,
                );
            }
        }

        self.previous_range = r;
        self.previous_range_trivia_end = r.loc.end();
        self.previous_parent = Some(parent);
        self.previous_range_start_line = range_start_line;

        line_action
    }

    /// Processes comment trivia in range (Go's `processTrivia`).
    ///
    /// Side effects: accumulates edits.
    // Go: internal/format/span.go:formatSpanWorker.processTrivia
    fn process_trivia(
        &mut self,
        trivia: &[TextRangeWithKind],
        parent: NodeId,
        context_node: Option<NodeId>,
        mut dynamic_indentation: Option<&mut DynamicIndenter>,
    ) {
        for item in trivia {
            if is_comment(item.kind) && item.loc.contained_by(self.original_range) {
                let pos = item.loc.pos();
                let line = self.line_of(pos);
                let character = pos - self.ctx.line_starts[line as usize].0;
                let cn = context_node.unwrap_or(parent);
                self.process_range(
                    *item,
                    line,
                    character,
                    parent,
                    cn,
                    dynamic_indentation.as_deref_mut(),
                );
            }
        }
    }

    /// Consumes the current token, processing it, its trivia, and indentation
    /// (Go's `consumeTokenAndAdvanceScanner`).
    ///
    /// Side effects: scans tokens; accumulates edits.
    // Go: internal/format/span.go:formatSpanWorker.consumeTokenAndAdvanceScanner
    fn consume_token_and_advance_scanner(
        &mut self,
        current_token_info: TokenInfo,
        parent: NodeId,
        dynamic_indentation: &mut DynamicIndenter,
        container: NodeId,
        is_list_end_token: bool,
    ) {
        let ctx = self.ctx;
        let last_trivia_was_new_line = self.scanner.last_trailing_trivia_was_new_line();
        let mut indent_token = false;

        if !current_token_info.leading_trivia.is_empty() {
            let cc = self.child_context_node;
            self.process_trivia(
                &current_token_info.leading_trivia,
                parent,
                cc,
                Some(&mut *dynamic_indentation),
            );
        }

        let mut line_action = LineAction::None;
        let is_token_in_range = current_token_info
            .token
            .loc
            .contained_by(self.original_range);

        let token_pos = current_token_info.token.loc.pos();
        let token_start_line = self.line_of(token_pos);
        let token_start_char = token_pos - ctx.line_starts[token_start_line as usize].0;

        if is_token_in_range {
            let range_has_error = self.range_contains_error(current_token_info.token.loc);
            let save_previous_range = self.previous_range;
            let cc = self.child_context_node.unwrap_or(parent);
            line_action = self.process_range(
                current_token_info.token,
                token_start_line,
                token_start_char,
                parent,
                cc,
                Some(&mut *dynamic_indentation),
            );
            if !range_has_error {
                if line_action == LineAction::None {
                    if save_previous_range != TextRangeWithKind::empty() {
                        let prev_end_line = self.line_of(save_previous_range.loc.end());
                        indent_token =
                            last_trivia_was_new_line && token_start_line != prev_end_line;
                    } else {
                        indent_token = last_trivia_was_new_line;
                    }
                } else {
                    indent_token = line_action == LineAction::LineAdded;
                }
            }
        }

        if !current_token_info.trailing_trivia.is_empty() {
            self.previous_range_trivia_end =
                current_token_info.trailing_trivia.last().unwrap().loc.end();
            let cc = self.child_context_node;
            self.process_trivia(
                &current_token_info.trailing_trivia,
                parent,
                cc,
                Some(&mut *dynamic_indentation),
            );
        }

        if indent_token {
            let mut token_indentation = -1;
            if is_token_in_range && !self.range_contains_error(current_token_info.token.loc) {
                token_indentation = dynamic_indentation.get_indentation_for_token(
                    ctx.file,
                    &ctx.line_starts,
                    &ctx.options,
                    token_start_line,
                    current_token_info.token.kind,
                    container,
                    is_list_end_token,
                );
            }
            let mut indent_next_token_or_trivia = true;
            if !current_token_info.leading_trivia.is_empty() {
                let comment_indentation = dynamic_indentation.get_indentation_for_comment(
                    ctx.file,
                    &ctx.line_starts,
                    &ctx.options,
                    current_token_info.token.kind,
                    token_indentation,
                    container,
                );
                indent_next_token_or_trivia = self.indent_trivia_items_for_token(
                    &current_token_info.leading_trivia,
                    comment_indentation,
                    indent_next_token_or_trivia,
                );
            }

            if token_indentation != -1 && indent_next_token_or_trivia {
                self.insert_indentation(
                    current_token_info.token.loc.pos(),
                    token_indentation,
                    line_action == LineAction::LineAdded,
                );
                self.last_indented_line = token_start_line;
                self.indentation_on_last_indented_line = token_indentation;
            }
        }

        self.scanner.advance();
        self.child_context_node = Some(parent);
    }

    /// Indents the single-line comments among leading trivia (the consume-token
    /// call site of Go's `indentTriviaItems`).
    ///
    /// Side effects: accumulates edits.
    // Go: internal/format/span.go:formatSpanWorker.indentTriviaItems
    fn indent_trivia_items_for_token(
        &mut self,
        trivia: &[TextRangeWithKind],
        comment_indentation: i32,
        mut indent_next: bool,
    ) -> bool {
        for item in trivia {
            let in_range = item.loc.contained_by(self.original_range);
            match item.kind {
                Kind::MultiLineCommentTrivia => {
                    // DEFER(phase-7): indentMultilineComment (multi-line comment re-indent).
                    indent_next = false;
                }
                Kind::SingleLineCommentTrivia => {
                    if indent_next && in_range {
                        self.insert_indentation(item.loc.pos(), comment_indentation, false);
                    }
                    indent_next = false;
                }
                Kind::NewLineTrivia => indent_next = true,
                _ => {}
            }
        }
        indent_next
    }

    /// Inserts/normalizes indentation at `pos` (Go's `insertIndentation`).
    ///
    /// Side effects: accumulates edits.
    // Go: internal/format/span.go:formatSpanWorker.insertIndentation
    fn insert_indentation(&mut self, pos: i32, indentation: i32, line_added: bool) {
        let ctx = self.ctx;
        let indentation_string = get_indentation_string(indentation, &ctx.options);
        if line_added {
            self.record_replace(pos, 0, &indentation_string);
        } else {
            let token_start_line = self.line_of(pos);
            let token_start_character = pos - ctx.line_starts[token_start_line as usize].0;
            let start_line_position = ctx.line_starts[token_start_line as usize].0;
            if indentation != self.character_to_column(start_line_position, token_start_character)
                || self.indentation_is_different(&indentation_string, start_line_position)
            {
                self.record_replace(
                    start_line_position,
                    token_start_character,
                    &indentation_string,
                );
            }
        }
    }

    /// Converts a byte offset within a line to a tab-expanded column.
    ///
    /// Side effects: none.
    // Go: internal/format/span.go:formatSpanWorker.characterToColumn
    fn character_to_column(&self, start_line_position: i32, character_in_line: i32) -> i32 {
        let ctx = self.ctx;
        let bytes = ctx.text.as_bytes();
        let mut column = 0i32;
        for i in 0..character_in_line {
            if bytes[(start_line_position + i) as usize] == b'\t' {
                if ctx.options.editor.tab_size > 0 {
                    column += ctx.options.editor.tab_size - (column % ctx.options.editor.tab_size);
                }
            } else {
                column += 1;
            }
        }
        column
    }

    /// Reports whether the existing indentation differs from `indentation_string`.
    ///
    /// Side effects: none.
    // Go: internal/format/span.go:formatSpanWorker.indentationIsDifferent
    fn indentation_is_different(&self, indentation_string: &str, start_line_position: i32) -> bool {
        let text = self.ctx.text;
        let end = start_line_position as usize + indentation_string.len();
        if end > text.len() {
            return true;
        }
        indentation_string != &text[start_line_position as usize..end]
    }

    /// Trims trailing whitespace on lines `[line1, line2)` (Go's
    /// `trimTrailingWhitespacesForLines`).
    ///
    /// Side effects: accumulates edits.
    // Go: internal/format/span.go:formatSpanWorker.trimTrailingWhitespacesForLines
    fn trim_trailing_whitespaces_for_lines(
        &mut self,
        line1: i32,
        line2: i32,
        r: TextRangeWithKind,
    ) {
        let ctx = self.ctx;
        for line in line1..line2 {
            let line_start_position = ctx.line_starts[line as usize].0;
            let line_end_position = self.get_ecma_end_line_position(line);

            if r != TextRangeWithKind::empty()
                && (is_comment(r.kind)
                    || is_string_or_regular_expression_or_template_literal(r.kind))
                && r.loc.pos() <= line_end_position
                && r.loc.end() > line_end_position
            {
                continue;
            }

            let whitespace_start =
                self.get_trailing_whitespace_start_position(line_start_position, line_end_position);
            if whitespace_start != -1 {
                self.record_delete(whitespace_start, line_end_position + 1 - whitespace_start);
            }
        }
    }

    /// The byte position of the last non-line-break char of `line` (Go's
    /// `GetECMAEndLinePosition`).
    ///
    /// Side effects: none.
    // Go: internal/scanner/scanner.go:GetECMAEndLinePosition
    fn get_ecma_end_line_position(&self, line: i32) -> i32 {
        let ctx = self.ctx;
        let mut pos = ctx.line_starts[line as usize].0;
        loop {
            match ctx.text[pos as usize..].chars().next() {
                None => return pos - 1,
                Some(ch) => {
                    if tsgo_stringutil::is_line_break(ch) {
                        return pos - 1;
                    }
                    pos += ch.len_utf8() as i32;
                }
            }
        }
    }

    /// The start position of the run of trailing whitespace ending at `end`, or
    /// `-1` (Go's `getTrailingWhitespaceStartPosition`).
    ///
    /// Side effects: none.
    // Go: internal/format/span.go:formatSpanWorker.getTrailingWhitespaceStartPosition
    fn get_trailing_whitespace_start_position(&self, start: i32, end: i32) -> i32 {
        let text = self.ctx.text;
        let mut pos = end;
        while pos >= start {
            let idx = pos as usize;
            if idx >= text.len() || !text.is_char_boundary(idx) {
                pos -= 1;
                continue;
            }
            match text[idx..].chars().next() {
                None => {
                    pos -= 1;
                    continue;
                }
                Some(ch) => {
                    if !tsgo_stringutil::is_white_space_single_line(ch) {
                        break;
                    }
                    pos -= 1;
                }
            }
        }
        if pos != end {
            pos + 1
        } else {
            -1
        }
    }

    /// Records a deletion of `length` bytes at `start`.
    // Go: internal/format/span.go:formatSpanWorker.recordDelete
    fn record_delete(&mut self, start: i32, length: i32) {
        if length != 0 {
            self.edits
                .push(TextChange::new(TextRange::new(start, start + length), ""));
        }
    }

    /// Records a replacement of `length` bytes at `start` with `new_text`.
    // Go: internal/format/span.go:formatSpanWorker.recordReplace
    fn record_replace(&mut self, start: i32, length: i32, new_text: &str) {
        if length != 0 || !new_text.is_empty() {
            self.edits.push(TextChange::new(
                TextRange::new(start, start + length),
                new_text,
            ));
        }
    }

    /// Records an insertion of `text` at `start`.
    // Go: internal/format/span.go:formatSpanWorker.recordInsert
    fn record_insert(&mut self, start: i32, text: &str) {
        if !text.is_empty() {
            self.edits
                .push(TextChange::new(TextRange::new(start, start), text));
        }
    }

    /// Populates the formatting-context projection from real nodes (Go's
    /// `FormattingContext.UpdateContext`, computing the projection fields the
    /// round-1 predicates read).
    ///
    /// Side effects: mutates `self.fcx`.
    // Go: internal/format/context.go:FormattingContext.UpdateContext
    fn update_formatting_context(
        &mut self,
        cur: TextRangeWithKind,
        cur_parent: NodeId,
        next: TextRangeWithKind,
        next_parent: NodeId,
        common_parent: NodeId,
    ) {
        let ctx = self.ctx;
        let file = ctx.file;
        let ls = &ctx.line_starts;

        self.fcx.current_token_kind = cur.kind;
        self.fcx.next_token_kind = next.kind;
        self.fcx.context_node_kind = file.kind(common_parent);
        self.fcx.current_token_parent_kind = file.kind(cur_parent);
        self.fcx.next_token_parent_kind = file.kind(next_parent);
        self.fcx.current_token_parent_parent_kind =
            file.arena().parent(cur_parent).map(|p| file.kind(p));
        self.fcx.next_token_parent_parent_kind =
            file.arena().parent(next_parent).map(|p| file.kind(p));

        self.fcx.binary_operator_token_kind = if file.kind(common_parent) == Kind::BinaryExpression
        {
            match file.arena().data(common_parent) {
                NodeData::BinaryExpression(d) => file.kind(d.operator_token),
                _ => Kind::Unknown,
            }
        } else {
            Kind::Unknown
        };

        // DEFER(phase-7): these projection fields drive predicates unreachable in
        // this round and default to safe values (no decorators / question token /
        // yield operand / integer-literal property access in the reachable set).
        self.fcx.context_node_has_question_token = false;
        self.fcx.context_node_has_expression = false;
        self.fcx.context_node_has_decorators = false;
        self.fcx.context_node_is_property_access_on_integer_literal = false;

        self.fcx.current_token_is_start_of_variable_declaration_list = file.kind(cur_parent)
            == Kind::VariableDeclarationList
            && get_start_of_node(file, cur_parent, false) == cur.loc.pos();

        self.fcx.tokens_are_on_same_line =
            range_is_on_one_line(ls, TextRange::new(cur.loc.pos(), next.loc.end()));
        self.fcx.context_node_all_on_same_line = self.node_is_on_one_line(common_parent);
        self.fcx.next_node_all_on_same_line = self.node_is_on_one_line(next_parent);
        self.fcx.context_node_block_is_on_one_line = self.block_is_on_one_line(common_parent);
        self.fcx.next_node_block_is_on_one_line = self.block_is_on_one_line(next_parent);
    }

    /// Whether `node`'s token-start range is on one line.
    // Go: internal/format/context.go:FormattingContext.nodeIsOnOneLine
    fn node_is_on_one_line(&self, node: NodeId) -> bool {
        range_is_on_one_line(&self.ctx.line_starts, with_token_start(self.ctx.file, node))
    }

    /// Whether `node`'s `{ ... }` block is on one line.
    // Go: internal/format/context.go:FormattingContext.blockIsOnOneLine
    fn block_is_on_one_line(&self, node: NodeId) -> bool {
        let file = self.ctx.file;
        let open = file.find_child_of_kind(node, Kind::OpenBraceToken);
        let close = file.find_child_of_kind(node, Kind::CloseBraceToken);
        if let (Some(open), Some(close)) = (open, close) {
            let close_brace_start = get_start_of_node(file, close, false);
            return range_is_on_one_line(
                &self.ctx.line_starts,
                TextRange::new(file.end(open), close_brace_start),
            );
        }
        false
    }
}

#[cfg(test)]
#[path = "span_test.rs"]
mod tests;
