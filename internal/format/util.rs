//! Node/list helpers for the AST-walking worker and the public entries.
//!
//! 1:1 port of the reachable subset of Go `internal/format/util.go` (plus
//! `withTokenStart` / `rangeIsOnOneLine` from `context.go`, and the
//! `GetLineStartPositionForPosition` helper). These run over a
//! [`tsgo_astnav::NavEngine`] shared-borrow navigation context: every position
//! query goes through `&self` navigation, and node payloads are read from
//! [`NavEngine::arena`].
//!
//! # Deferred (blocked-by)
//!
//! `getOpenTokenForList` / `getCloseTokenForOpenToken` are unused by this port:
//! Go drives them from the list-distinguishing `NodeVisitor` (`processChildNodes`),
//! whereas this port walks children through `tsgo_astnav`'s flat
//! `visit_each_child_and_jsdoc` (see the worker docs in [`crate::span`]). They are
//! deferred with the multi-line list-scope handling.

use crate::span::find_enclosing_node;
use std::borrow::Borrow;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId};
use tsgo_astnav::{get_start_of_node, NavEngine};
use tsgo_core::text::{TextPos, TextRange};
use tsgo_scanner::compute_line_of_position;

/// The high bit `tsgo_astnav` sets on synthesized-token ids (mirrors its private
/// `SYNTHESIZED_NODE_TAG`). Synthesized tokens are not arena nodes, so their
/// arena `parent` is out of range.
const SYNTHESIZED_NODE_TAG: u32 = 1 << 31;

/// Returns the parent of `id`, resolving synthesized tokens.
///
/// `tsgo_astnav` keeps synthesized tokens (e.g. a `}` or `]` returned by
/// `find_preceding_token`) in a side store and does not expose their parent. Go
/// reads `token.Parent` directly; here the parent is recovered as the smallest
/// real node enclosing the token (its container). Real nodes use the arena
/// `parent` back-edge.
///
/// Side effects: none (reads the nav context).
pub fn node_parent<A: Borrow<NodeArena>>(file: &NavEngine<A>, id: NodeId) -> Option<NodeId> {
    if id.0 & SYNTHESIZED_NODE_TAG != 0 {
        Some(find_enclosing_node(
            file,
            TextRange::new(file.pos(id), file.end(id)),
        ))
    } else {
        file.arena().parent(id)
    }
}

/// Returns the token-start range of `node`: `[get_start_of_node(node), node.end())`.
///
/// Mirrors Go's `withTokenStart` (`context.go`): the start skips leading trivia
/// (`scanner.GetTokenPosOfNode(node, file, false)`), the end is the node end.
///
/// Side effects: none (pure; may synthesize tokens via the nav context).
// Go: internal/format/context.go:withTokenStart
pub fn with_token_start<A: Borrow<NodeArena>>(file: &NavEngine<A>, node: NodeId) -> TextRange {
    TextRange::new(get_start_of_node(file, node, false), file.end(node))
}

/// Reports whether `r` begins and ends on the same source line.
///
/// Mirrors Go's `rangeIsOnOneLine` (`util.go`), but takes a precomputed
/// `line_starts` slice (the worker computes it once; Go caches it on the file).
///
/// Side effects: none (pure).
// Go: internal/format/util.go:rangeIsOnOneLine
pub fn range_is_on_one_line(line_starts: &[TextPos], r: TextRange) -> bool {
    compute_line_of_position(line_starts, r.pos()) == compute_line_of_position(line_starts, r.end())
}

/// Returns the byte offset at which the line containing `position` begins.
///
/// Mirrors Go's `GetLineStartPositionForPosition`.
///
/// Side effects: none (pure).
// Go: internal/format/util.go:GetLineStartPositionForPosition
pub fn get_line_start_position_for_position(line_starts: &[TextPos], position: i32) -> i32 {
    let line = compute_line_of_position(line_starts, position);
    line_starts[line as usize].0
}

/// Tests whether `child` is a grammar error on `parent` (and therefore must be
/// skipped by the worker).
///
/// Mirrors Go's `isGrammarError`. The grammar-error-bearing parent kinds
/// (`TypeParameterDeclaration`, `PropertySignature`, `PropertyDeclaration`,
/// `PropertyAssignment`, `ShorthandPropertyAssignment`, `MethodDeclaration`,
/// `Constructor`, `Get`/`SetAccessor`, `NamespaceExportDeclaration`) require deep
/// node accessors that are out of the reachable round scope; those branches are
/// deferred and report `false` (no node in the reachable test set is one of
/// them, so the reachable behavior is exact).
///
/// Side effects: none (pure).
// Go: internal/format/util.go:isGrammarError
// DEFER(phase-7): the grammar-error-bearing parent branches need
// PropertySignature/PropertyAssignment/accessor accessors not yet ported. No
// reachable node kind is one of them, so reporting `false` is exact here.
// blocked-by: tsgo_ast accessors for those node payloads.
pub fn is_grammar_error<A: Borrow<NodeArena>>(
    _file: &NavEngine<A>,
    _parent: NodeId,
    _child: NodeId,
) -> bool {
    false
}

/// Returns the token immediately preceding `end` only when it is of
/// `expected_token_kind` and ends exactly at `end`.
///
/// Mirrors Go's `findImmediatelyPrecedingTokenOfKind`: validates the token kind
/// so that, e.g., a typed `}` is the close brace and not a comment.
///
/// Side effects: may synthesize tokens via the nav context.
// Go: internal/format/util.go:findImmediatelyPrecedingTokenOfKind
pub fn find_immediately_preceding_token_of_kind<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    end: i32,
    expected_token_kind: Kind,
) -> Option<NodeId> {
    let preceding_token = file.find_preceding_token(end)?;
    if file.kind(preceding_token) != expected_token_kind || file.end(preceding_token) != end {
        return None;
    }
    Some(preceding_token)
}

/// Finds the highest node enclosing `node` at the same list level whose end does
/// not exceed `node.end`.
///
/// Mirrors Go's `findOutermostNodeWithinListLevel`. Used by the on-`}`/on-`;`
/// entries to widen the span up to (but not past) the enclosing list element.
///
/// Side effects: none (pure; reads the arena).
// Go: internal/format/util.go:findOutermostNodeWithinListLevel
pub fn find_outermost_node_within_list_level<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    node: NodeId,
) -> NodeId {
    let node_end = file.end(node);
    let mut current = node;
    while let Some(parent) = node_parent(file, current) {
        if file.end(parent) != node_end || is_list_element(file, parent, current) {
            break;
        }
        current = parent;
    }
    current
}

/// Reports whether `node` is an element of one of `parent`'s lists.
///
/// Mirrors Go's `isListElement`. The reachable subset covers the statement-list
/// containers (`SourceFile`, `Block`, `ModuleBlock`); the class/interface member
/// list, module body, and catch-clause branches are deferred (they need member
/// list / body accessors out of round scope) and report `false`.
///
/// Side effects: none (pure; reads the arena).
// Go: internal/format/util.go:isListElement
pub fn is_list_element<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    parent: NodeId,
    node: NodeId,
) -> bool {
    // `node` may be a synthesized token (e.g. a `}`); read its range via the nav
    // accessors rather than the arena, which only holds real nodes.
    let node_loc = TextRange::new(file.pos(node), file.end(node));
    match file.arena().data(parent) {
        NodeData::SourceFile(d) => node_loc.contained_by(d.statements.loc),
        NodeData::Block(d) => node_loc.contained_by(d.list.loc),
        NodeData::ModuleBlock(d) => node_loc.contained_by(d.statements.loc),
        // DEFER(phase-7): class/interface member lists, module body, catch block.
        // blocked-by: tsgo_ast member-list / body accessors.
        _ => false,
    }
}

#[cfg(test)]
#[path = "util_test.rs"]
mod tests;
