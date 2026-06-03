//! Port of Go `internal/ls/autoinsert.go`: the on-type auto-insert provider
//! (`textDocument/_vs_onAutoInsert`).
//!
//! When the user types `>`, Go's `ProvideOnAutoInsert` auto-closes a JSX
//! element or fragment: it finds the token preceding the cursor and, if that
//! token sits in an *unclosed* JSX element/fragment, returns a snippet
//! [`TextEdit`](tsgo_lsproto::TextEdit) that inserts `$0</tag>` (or `$0</>`) at
//! the cursor. It is **purely syntactic** (no checker): it reads the program's
//! already-parsed source file (its [`NodeArena`](tsgo_ast::NodeArena) + root)
//! through [`astnav`](tsgo_astnav)'s shared-borrow [`NavSourceFile`] and
//! converts positions with the project [`Converters`](tsgo_ls_lsconv::Converters)
//! — the same `&self` pattern as `linkedediting.rs` / `selectionranges.rs`.
//!
//! # DEFER
//!
//! DEFER(phase-7-ls): the two **`>`-token** branches (Go's
//! `token.Kind == ast.KindGreaterThanToken && ast.IsJsxOpeningElement/Fragment(token.Parent)`).
//! Those branches handle the common "cursor right after the just-typed `>`"
//! case (`<div>|` -> `</div>`), but they reach `token.Parent` on a
//! `FindPrecedingToken` result that is the synthesized `>` punctuation token.
//! `tsgo_astnav` synthesizes such tokens into a side store that carries **no
//! parent pointer** (so [`NodeArena::parent`](tsgo_ast::NodeArena::parent)
//! cannot be called on them — the same root cause as `linkedediting.rs`'s
//! fragment branch and `signaturehelp.rs`'s synthesized-`(`/`,` note). This
//! module therefore guards with [`is_synthesized_token`] and treats a
//! synthesized preceding token as "no auto-insert here" (`None`); the two
//! `>`-token branches are **ported faithfully but inert**. The two
//! **`IsJsxText`** branches are fully reachable: when the cursor sits in the
//! JSX text/children of an unclosed element/fragment (`<div> text|`,
//! `<> text|`), the preceding token is the real `JsxText` node, whose arena
//! parent is the enclosing element/fragment. blocked-by: a parent-carrying
//! synthesized-token store in `tsgo_astnav`.

use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId};
use tsgo_astnav::NavSourceFile;
use tsgo_lsproto::{InsertTextFormat, Position, Range, TextEdit, VsOnAutoInsertResponseItem};

use crate::languageservice::LanguageService;

impl LanguageService {
    /// Returns the JSX auto-close edit for typing `ch` at `position` in
    /// `file_name`: a snippet [`TextEdit`](tsgo_lsproto::TextEdit) inserting the
    /// matching closing tag (`</tag>`) or fragment (`</>`) at the cursor.
    ///
    /// `None` when `ch` is not `>`, there is no such file, the preceding token
    /// is not adjacent to an unclosed JSX element/fragment, or the tag is
    /// already closed (see the module DEFER for which trigger positions are
    /// reachable).
    ///
    /// # Examples
    ///
    /// Typing `>` with the cursor in the JSX text of an unclosed `<div>`
    /// returns a snippet edit inserting `$0</div>` at the cursor.
    ///
    /// Side effects: none (reads the already-parsed file; no binding/checking).
    // Go: internal/ls/autoinsert.go:LanguageService.ProvideOnAutoInsert
    pub fn provide_on_auto_insert(
        &self,
        file_name: &str,
        position: Position,
        ch: &str,
    ) -> Option<VsOnAutoInsertResponseItem> {
        // Go: `if params.VSCh != ">" { return empty }`.
        if ch != ">" {
            return None;
        }

        let script = self.document_script(file_name)?;
        let parsed = self.program().get_source_file(file_name)?;
        let nav = NavSourceFile::from_borrowed_arena(
            parsed.arena(),
            parsed.node(),
            parsed.text().to_string(),
        );
        let converters = self.converters();

        // Go: `position := l.converters.LineAndCharacterToPosition(sourceFile, params.VSPosition)`.
        let offset = converters
            .line_and_character_to_position(&script, position.clone())
            .0;

        // Go: `token := astnav.FindPrecedingToken(...); if token == nil { return empty }`.
        let token = nav.find_preceding_token(offset)?;
        // A synthesized `>` punctuation token has no arena parent (see module
        // DEFER), so the two `>`-token branches below are unreachable for it.
        if is_synthesized_token(token) {
            return None;
        }
        let arena = nav.arena();
        let token_kind = arena.kind(token);
        let token_parent = arena.parent(token);

        // Go: pick the enclosing JSX element (the `>`-token branch's
        // grandparent, or the `IsJsxText` branch's parent).
        let element = if token_kind == Kind::GreaterThanToken
            && token_parent.is_some_and(|p| arena.kind(p) == Kind::JsxOpeningElement)
        {
            arena.parent(token_parent.unwrap())
        } else if token_kind == Kind::JsxText
            && token_parent.is_some_and(|p| arena.kind(p) == Kind::JsxElement)
        {
            token_parent
        } else {
            None
        };

        let closing_text = if let Some(element) = element.filter(|&e| is_unclosed_tag(arena, e)) {
            // Go: `closingText = "</" + EntityNameToString(openingElement.TagName(), scanner.GetTextOfNode) + ">"`.
            // Slight divergence from Strada: the closing tag is rebuilt from the
            // opening tag-name rather than copied verbatim from the source.
            let (opening, _) = jsx_element_parts(arena, element)?;
            let tag_name = jsx_tag_name(arena, opening)?;
            format!("</{}>", entity_name_to_string(arena, tag_name))
        } else {
            // Go: the fragment branches (the element was closed or absent).
            let fragment = if token_kind == Kind::GreaterThanToken
                && token_parent.is_some_and(|p| arena.kind(p) == Kind::JsxOpeningFragment)
            {
                arena.parent(token_parent.unwrap())
            } else if token_kind == Kind::JsxText
                && token_parent.is_some_and(|p| arena.kind(p) == Kind::JsxFragment)
            {
                token_parent
            } else {
                None
            };

            if fragment.is_some_and(|f| is_unclosed_fragment(arena, f)) {
                "</>".to_string()
            } else {
                // Go: `if closingText == "" { return empty }`.
                return None;
            }
        };

        // Go: build the snippet text edit at a zero-width range at the cursor
        // (`params.VSPosition`). Tag names can contain `$` (a valid JSX
        // identifier character), so escape the closing text to avoid being
        // interpreted as a snippet placeholder/variable.
        Some(VsOnAutoInsertResponseItem {
            vs_text_edit_format: InsertTextFormat::SNIPPET,
            vs_text_edit: TextEdit {
                range: Range {
                    start: position.clone(),
                    end: position,
                },
                new_text: format!("$0{}", escape_snippet_text(&closing_text)),
            },
        })
    }
}

/// Reports whether the JSX element `node` (a [`NodeData::JsxElement`]) is
/// unclosed: its opening and closing tag names differ, or it is nested inside a
/// same-named parent element that is itself unclosed.
///
/// Side effects: none (pure walk over arena parent pointers).
// Go: internal/ls/autoinsert.go:isUnclosedTag
fn is_unclosed_tag(arena: &NodeArena, node: NodeId) -> bool {
    let Some((opening, closing)) = jsx_element_parts(arena, node) else {
        return false;
    };
    let (Some(opening_tag), Some(closing_tag)) =
        (jsx_tag_name(arena, opening), jsx_tag_name(arena, closing))
    else {
        return false;
    };
    if !tag_names_are_equivalent(arena, opening_tag, closing_tag) {
        return true;
    }

    // Go: if the parent is a JsxElement, this element is unclosed when its
    // opening tag matches the parent's and the parent is itself unclosed.
    if let Some(parent) = arena.parent(node) {
        if arena.kind(parent) == Kind::JsxElement {
            let Some((parent_opening, _)) = jsx_element_parts(arena, parent) else {
                return false;
            };
            let Some(parent_opening_tag) = jsx_tag_name(arena, parent_opening) else {
                return false;
            };
            return tag_names_are_equivalent(arena, opening_tag, parent_opening_tag)
                && is_unclosed_tag(arena, parent);
        }
    }
    false
}

/// Reports whether the JSX fragment `node` (a [`NodeData::JsxFragment`]) is
/// unclosed: its closing `</>` is missing (its node carries a parse error), or
/// it is nested inside an unclosed parent fragment.
///
/// Side effects: none (pure walk over arena parent pointers).
// Go: internal/ls/autoinsert.go:isUnclosedFragment
fn is_unclosed_fragment(arena: &NodeArena, node: NodeId) -> bool {
    let Some((_, closing_fragment)) = jsx_element_parts(arena, node) else {
        return false;
    };
    // Go: `closingFragment.Flags & ast.NodeFlagsThisNodeHasError != 0`.
    if arena
        .flags(closing_fragment)
        .contains(NodeFlags::THIS_NODE_HAS_ERROR)
    {
        return true;
    }

    if let Some(parent) = arena.parent(node) {
        if arena.kind(parent) == Kind::JsxFragment && is_unclosed_fragment(arena, parent) {
            return true;
        }
    }
    false
}

/// The `(opening, closing)` node ids of a JSX element / fragment (both share
/// [`NodeData::JsxElement`]'s `opening`/`closing` payload), or `None` if `node`
/// is neither.
///
/// Side effects: none (pure).
fn jsx_element_parts(arena: &NodeArena, node: NodeId) -> Option<(NodeId, NodeId)> {
    match arena.data(node) {
        NodeData::JsxElement(d) | NodeData::JsxFragment(d) => Some((d.opening, d.closing)),
        _ => None,
    }
}

/// The tag-name node of a JSX opening / self-closing / closing element, or
/// `None` for any other node.
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

/// Renders a JSX tag-name expression to its source text, reproducing
/// `ast.EntityNameToString` (called with `scanner.GetTextOfNode`): an
/// identifier yields its text, `this` yields `"this"`, a property access
/// yields `<object>.<name>`, and a JSX namespaced name yields `<ns>:<name>`.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:EntityNameToString
fn entity_name_to_string(arena: &NodeArena, name: NodeId) -> String {
    match arena.kind(name) {
        Kind::ThisKeyword => "this".to_string(),
        Kind::Identifier | Kind::PrivateIdentifier => arena.text(name).to_string(),
        Kind::QualifiedName => match arena.data(name) {
            NodeData::QualifiedName(d) => format!(
                "{}.{}",
                entity_name_to_string(arena, d.left),
                entity_name_to_string(arena, d.right)
            ),
            _ => String::new(),
        },
        Kind::PropertyAccessExpression => match arena.data(name) {
            NodeData::PropertyAccessExpression(d) => format!(
                "{}.{}",
                entity_name_to_string(arena, d.expression),
                entity_name_to_string(arena, d.name)
            ),
            _ => String::new(),
        },
        Kind::JsxNamespacedName => match arena.data(name) {
            NodeData::JsxNamespacedName(d) => format!(
                "{}:{}",
                entity_name_to_string(arena, d.namespace),
                entity_name_to_string(arena, d.name)
            ),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// Reports whether two JSX tag-name expressions denote the same tag (by
/// comparing identifier/namespace/member text recursively).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:TagNamesAreEquivalent
fn tag_names_are_equivalent(arena: &NodeArena, lhs: NodeId, rhs: NodeId) -> bool {
    let lhs_kind = arena.kind(lhs);
    if lhs_kind != arena.kind(rhs) {
        return false;
    }
    match lhs_kind {
        Kind::Identifier => arena.text(lhs) == arena.text(rhs),
        Kind::ThisKeyword => true,
        Kind::JsxNamespacedName => match (arena.data(lhs), arena.data(rhs)) {
            (NodeData::JsxNamespacedName(l), NodeData::JsxNamespacedName(r)) => {
                arena.text(l.namespace) == arena.text(r.namespace)
                    && arena.text(l.name) == arena.text(r.name)
            }
            _ => false,
        },
        Kind::PropertyAccessExpression => match (arena.data(lhs), arena.data(rhs)) {
            (NodeData::PropertyAccessExpression(l), NodeData::PropertyAccessExpression(r)) => {
                arena.text(l.name) == arena.text(r.name)
                    && tag_names_are_equivalent(arena, l.expression, r.expression)
            }
            _ => false,
        },
        _ => false,
    }
}

/// Escapes snippet metacharacters in `text` by backslash-escaping `$` (so a
/// tag name containing `$` is inserted literally rather than parsed as a
/// snippet placeholder/variable).
///
/// # Examples
///
/// `</$Foo>` becomes `</\$Foo>`; text without `$` is unchanged.
///
/// Side effects: none (pure).
// Go: internal/ls/completions.go:escapeSnippetText
fn escape_snippet_text(text: &str) -> String {
    text.replace('$', "\\$")
}

/// The high-bit tag `tsgo_astnav` sets on a synthesized-token [`NodeId`]
/// (mirrors `internal/astnav/lib.rs`'s private `SYNTHESIZED_NODE_TAG`).
const SYNTHESIZED_NODE_TAG: u32 = 1 << 31;

/// Reports whether `node` is a synthesized navigation token (a scanner-produced
/// punctuation/keyword token that lives in `astnav`'s side store, not the
/// parsed arena, and therefore has no arena parent).
///
/// Mirrors `linkedediting.rs`'s `is_synthesized_token`; duplicated because the
/// tag is `astnav`-internal, not part of its public API.
///
/// Side effects: none (pure).
fn is_synthesized_token(node: NodeId) -> bool {
    node.0 & SYNTHESIZED_NODE_TAG != 0
}

#[cfg(test)]
#[path = "autoinsert_test.rs"]
mod tests;
