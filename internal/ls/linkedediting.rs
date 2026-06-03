//! Port of Go `internal/ls/linkedediting.go`: the linked-editing-ranges feature
//! (`textDocument/linkedEditingRange`).
//!
//! When the cursor sits inside a JSX tag name, Go's `ProvideLinkedEditingRange`
//! returns *both* the opening and closing tag-name ranges (so an editor renames
//! the pair together as the user types) plus a `wordPattern` regex describing
//! valid tag-name characters. It is **purely syntactic** (no checker): it reads
//! the program's already-parsed source file (its
//! [`NodeArena`](tsgo_ast::NodeArena) + root) through [`astnav`](tsgo_astnav)'s
//! shared-borrow [`NavSourceFile`] and converts byte offsets to UTF-16 LSP
//! positions with the project [`Converters`](tsgo_ls_lsconv::Converters) — the
//! same `&self` pattern as `selectionranges.rs` / `folding.rs`.
//!
//! # DEFER
//!
//! DEFER(phase-7-ls): the **JSX fragment branch** (`<>...</>`) and the
//! element-branch *boundary* positions that land on JSX punctuation (`<`, `>`,
//! `/`). Go reaches `token.Parent` for a `FindPrecedingToken` result that is a
//! synthesized punctuation/keyword token, but `tsgo_astnav` synthesizes those
//! tokens into a side store that does **not** carry a parent pointer (the parent
//! is only a cache key, never read back) and they are not real arena nodes — so
//! [`NodeArena::parent`](tsgo_ast::NodeArena::parent) cannot be called on them
//! at all. This module therefore guards with [`is_synthesized_token`] and treats
//! a synthesized preceding token as "no linked editing here" (`None`). The
//! fragment branch's only valid cursor positions (`openPos`/`closePos` sit
//! exactly on the `<` / `</` punctuation) always resolve to such synthesized
//! tokens, so the branch is **ported faithfully but inert**. The element branch
//! is fully reachable for any cursor strictly inside a tag name (which resolves
//! to the real tag-name identifier node). blocked-by: a parent-carrying
//! synthesized-token store in `tsgo_astnav` (same root cause as
//! `signaturehelp.rs`'s synthesized-`(`/`,` note).

use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId};
use tsgo_astnav::{get_start_of_node, NavSourceFile};
use tsgo_core::text::{TextPos, TextRange};
use tsgo_lsproto::{LinkedEditingRanges, Position, Range};

use crate::languageservice::LanguageService;

/// The JSX-tag-name word pattern Go ships verbatim as `jsxTagWordPattern`. It is
/// intentionally permissive (matches more than valid tag names) so linked
/// editing keeps working while the tag name is still being typed / incomplete.
///
/// Go's source literal `"[a-zA-Z0-9:\\-\\._$]*"` denotes the string
/// `[a-zA-Z0-9:\-\._$]*`; this raw string is byte-for-byte identical.
// Go: internal/ls/linkedediting.go:jsxTagWordPattern
const JSX_TAG_WORD_PATTERN: &str = r"[a-zA-Z0-9:\-\._$]*";

impl LanguageService {
    /// Returns the linked editing ranges for the JSX tag name touching
    /// `position` in `file_name`: the opening **and** closing tag-name
    /// [`Range`]s (so an editor renames them together) plus the
    /// [`JSX_TAG_WORD_PATTERN`] word pattern.
    ///
    /// `None` when there is no such file, the cursor is not inside a JSX element
    /// tag name, the tags are malformed, or the two tag names differ.
    ///
    /// # Examples
    ///
    /// A cursor inside the opening `div` of `<div></div>` returns the two
    /// `div` tag-name ranges (`0:1-0:4` and `0:7-0:10`) plus the word pattern.
    ///
    /// Side effects: none (reads the already-parsed file; no binding/checking).
    // Go: internal/ls/linkedediting.go:LanguageService.ProvideLinkedEditingRange
    pub fn provide_linked_editing_ranges(
        &self,
        file_name: &str,
        position: Position,
    ) -> Option<LinkedEditingRanges> {
        let script = self.document_script(file_name)?;
        let parsed = self.program().get_source_file(file_name)?;
        let nav = NavSourceFile::from_borrowed_arena(
            parsed.arena(),
            parsed.node(),
            parsed.text().to_string(),
        );
        let converters = self.converters();

        let position = converters
            .line_and_character_to_position(&script, position)
            .0;
        let token = nav.find_preceding_token(position)?;
        // A synthesized punctuation/keyword token is not a real arena node, so
        // `arena.parent` would panic on it (see the module DEFER). Go can read
        // its `Parent`; here we treat it as "no linked editing here".
        if is_synthesized_token(token) {
            return None;
        }
        let arena = nav.arena();

        // Go: `token == nil || token.Parent.Kind == ast.KindSourceFile`.
        let token_parent = arena.parent(token)?;
        if arena.kind(token_parent) == Kind::SourceFile {
            return None;
        }

        // Go: `if ast.IsJsxFragment(token.Parent.Parent)`.
        let grandparent = arena.parent(token_parent);
        if grandparent.is_some_and(|gp| arena.kind(gp) == Kind::JsxFragment) {
            return self.linked_editing_for_fragment(&nav, &script, position, grandparent.unwrap());
        }
        self.linked_editing_for_element(&nav, &script, position, token_parent)
    }

    /// The JSX-element branch: the cursor's nearest opening/closing element's
    /// pair of tag-name ranges (Go's `else` block).
    ///
    /// Side effects: none (pure read of the parsed file).
    // Go: internal/ls/linkedediting.go:ProvideLinkedEditingRange (element branch)
    fn linked_editing_for_element(
        &self,
        nav: &NavSourceFile<'_>,
        script: &crate::languageservice::DocumentScript,
        position: i32,
        token_parent: NodeId,
    ) -> Option<LinkedEditingRanges> {
        let arena = nav.arena();
        let converters = self.converters();

        // Go: `tag := ast.FindAncestor(token.Parent, isOpeningOrClosingElement)`.
        let tag = find_ancestor(arena, token_parent, |a, n| {
            matches!(a.kind(n), Kind::JsxOpeningElement | Kind::JsxClosingElement)
        })?;
        debug_assert!(matches!(
            arena.kind(tag),
            Kind::JsxOpeningElement | Kind::JsxClosingElement
        ));

        // Go: `jsxElement := tag.Parent.AsJsxElement()`.
        let jsx_element = arena.parent(tag)?;
        let (open_tag, close_tag) = match arena.data(jsx_element) {
            NodeData::JsxElement(d) => (d.opening, d.closing),
            _ => return None,
        };

        let open_tag_name = jsx_tag_name(arena, open_tag)?;
        let close_tag_name = jsx_tag_name(arena, close_tag)?;

        let open_tag_name_start = get_start_of_node(nav, open_tag_name, false);
        let open_tag_name_end = nav.end(open_tag_name);
        let close_tag_name_start = get_start_of_node(nav, close_tag_name, false);
        let close_tag_name_end = nav.end(close_tag_name);

        // Go: do not return linked cursors if tags are not well-formed.
        if open_tag_name_start == get_start_of_node(nav, open_tag, false)
            || close_tag_name_start == get_start_of_node(nav, close_tag, false)
            || open_tag_name_end == nav.end(open_tag)
            || close_tag_name_end == nav.end(close_tag)
        {
            return None;
        }

        // Go: only return linked cursors if the cursor is within a tag name.
        if !((open_tag_name_start..=open_tag_name_end).contains(&position)
            || (close_tag_name_start..=close_tag_name_end).contains(&position))
        {
            return None;
        }

        // Go: only return linked cursors if text in both tags is identical
        // (`scanner.GetTextOfNode` == the trivia-skipped tag-name source slice).
        let open_text = &nav.text()[open_tag_name_start as usize..open_tag_name_end as usize];
        let close_text = &nav.text()[close_tag_name_start as usize..close_tag_name_end as usize];
        if open_text != close_text {
            return None;
        }

        Some(LinkedEditingRanges {
            ranges: vec![
                converters.to_lsp_range(
                    script,
                    TextRange::new(open_tag_name_start, open_tag_name_end),
                ),
                converters.to_lsp_range(
                    script,
                    TextRange::new(close_tag_name_start, close_tag_name_end),
                ),
            ],
            word_pattern: Some(JSX_TAG_WORD_PATTERN.to_string()),
        })
    }

    /// The JSX-fragment branch (`<>...</>`): the `<` / `</` cursor stops.
    ///
    /// Faithful to Go but **inert** under the current `tsgo_astnav` (see the
    /// module DEFER): its only valid cursor positions land on synthesized
    /// punctuation tokens, which carry no arena parent, so control never reaches
    /// here in practice.
    ///
    /// Side effects: none (pure read of the parsed file).
    // Go: internal/ls/linkedediting.go:ProvideLinkedEditingRange (fragment branch)
    fn linked_editing_for_fragment(
        &self,
        nav: &NavSourceFile<'_>,
        script: &crate::languageservice::DocumentScript,
        position: i32,
        fragment: NodeId,
    ) -> Option<LinkedEditingRanges> {
        let arena = nav.arena();
        let converters = self.converters();

        let (open_fragment, close_fragment) = match arena.data(fragment) {
            NodeData::JsxFragment(d) => (d.opening, d.closing),
            _ => return None,
        };
        if arena
            .flags(open_fragment)
            .contains(NodeFlags::THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR)
            || arena
                .flags(close_fragment)
                .contains(NodeFlags::THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR)
        {
            return None;
        }

        // Go: `openPos = start(openFragment) + len("<")`, `closePos = start(closeFragment) + len("</")`.
        let open_pos = get_start_of_node(nav, open_fragment, false) + 1;
        let close_pos = get_start_of_node(nav, close_fragment, false) + 2;

        // Go: only allows linked editing right after opening bracket: `<| ></| >`.
        if position != open_pos && position != close_pos {
            return None;
        }

        // The fragment ranges are zero-width (only the start position is
        // returned): the length of a fragment tag is always fixed.
        let open_line_char = converters.position_to_line_and_character(script, TextPos(open_pos));
        let close_line_char = converters.position_to_line_and_character(script, TextPos(close_pos));
        Some(LinkedEditingRanges {
            ranges: vec![
                Range {
                    start: open_line_char.clone(),
                    end: open_line_char,
                },
                Range {
                    start: close_line_char.clone(),
                    end: close_line_char,
                },
            ],
            word_pattern: Some(JSX_TAG_WORD_PATTERN.to_string()),
        })
    }
}

/// Returns the tag-name node of a JSX opening / self-closing / closing element.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.TagName (JSX cases)
fn jsx_tag_name(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::JsxOpeningElement(d) | NodeData::JsxSelfClosingElement(d) => Some(d.tag_name),
        NodeData::JsxClosingElement(d) => Some(d.tag_name),
        _ => None,
    }
}

/// The high-bit tag `tsgo_astnav` sets on a synthesized-token [`NodeId`]
/// (mirrors `internal/astnav/lib.rs`'s private `SYNTHESIZED_NODE_TAG`).
const SYNTHESIZED_NODE_TAG: u32 = 1 << 31;

/// Reports whether `node` is a synthesized navigation token (a scanner-produced
/// punctuation/keyword token that lives in `astnav`'s side store, not the parsed
/// arena, and therefore has no arena parent).
///
/// Mirrors `signaturehelp.rs`'s `is_synthesized_token`; duplicated because the
/// tag is `astnav`-internal, not part of its public API.
///
/// Side effects: none (pure).
fn is_synthesized_token(node: NodeId) -> bool {
    node.0 & SYNTHESIZED_NODE_TAG != 0
}

/// Walks up the parent chain from `node` (inclusive) returning the first node
/// for which `callback` is true, or `None` at the root.
///
/// Side effects: none (pure walk over arena parent pointers).
// Go: internal/ast/utilities.go:FindAncestor
fn find_ancestor(
    arena: &NodeArena,
    node: NodeId,
    callback: impl Fn(&NodeArena, NodeId) -> bool,
) -> Option<NodeId> {
    let mut current = Some(node);
    while let Some(n) = current {
        if callback(arena, n) {
            return Some(n);
        }
        current = arena.parent(n);
    }
    None
}

#[cfg(test)]
#[path = "linkedediting_test.rs"]
mod tests;
