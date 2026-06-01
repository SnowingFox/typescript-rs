//! Port of Go `internal/ls/documenthighlights.go`: the document-highlights
//! feature (`textDocument/documentHighlight`).
//!
//! Go's `ProvideDocumentHighlights` resolves the token at a position and, for an
//! identifier bound to a symbol, returns one `lsproto.DocumentHighlight` per
//! same-symbol occurrence *in the current file* (the find-all-references
//! machinery, `getSemanticDocumentHighlights`), each tagged with a
//! `DocumentHighlightKind` (`Write` for an assignment target / write-access
//! declaration, `Read` otherwise; `Text` for non-node ranges).
//!
//! # Reachable subset
//!
//! This round ports the single-file semantic-highlight path:
//! [`LanguageService::provide_document_highlights`] reuses
//! [`same_symbol_reference_nodes`](crate::references::same_symbol_reference_nodes)
//! to collect the same-symbol identifier nodes in the file and classifies each
//! as [`DocumentHighlightKind::Write`] or [`DocumentHighlightKind::Read`] via the
//! reachable subset of `ast.IsWriteAccessForReference`.
//!
//! DEFER(phase-7-ls): the syntactic highlights (`getSyntacticDocumentHighlights`
//! — `if`/`else`, `return`/`throw`, `try`/`catch`/`finally`, loops, `switch`,
//! accessors, `async`/`await`/`yield`, modifier occurrences) and the JSX
//! opening/closing-tag highlights; cross-file / multi-document highlights
//! (`ProvideMultiDocumentHighlights`); and the [`DocumentHighlightKind::Text`]
//! range-entry kind (string-literal references).
//! blocked-by: a `compiler.Program`-level multi-file reference search and the
//! string-literal reference machinery. NOTE: `tsgo_lsproto` does not yet carry a
//! generated `DocumentHighlight`/`DocumentHighlightKind`; this module defines the
//! LSP-shaped types locally (matching `lsproto.DocumentHighlightKind`'s wire
//! values) until that crate gains them.

use tsgo_ast::utilities::is_assignment_operator;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId};
use tsgo_astnav::{get_start_of_node, NavSourceFile};
use tsgo_core::text::TextRange;
use tsgo_lsproto::{Position, Range};

use crate::definition::name_of_declaration;
use crate::languageservice::LanguageService;
use crate::references::same_symbol_reference_nodes;

/// The kind of a document highlight, mirroring LSP `DocumentHighlightKind`
/// (Go's `lsproto.DocumentHighlightKind`): the integer wire values are
/// `Text = 1`, `Read = 2`, `Write = 3`.
///
/// Side effects: none (a plain enum).
// Go: internal/lsp/lsproto/lsp_generated.go:DocumentHighlightKind
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum DocumentHighlightKind {
    /// A textual occurrence (used for ranges with no resolved symbol node).
    Text = 1,
    /// A read access of the symbol.
    Read = 2,
    /// A write access of the symbol (an assignment target or write-access
    /// declaration).
    Write = 3,
}

/// A document highlight (LSP `DocumentHighlight`): a [`Range`] in the current
/// document plus the access [`DocumentHighlightKind`].
///
/// Side effects: none (a plain data record).
// Go: internal/lsp/lsproto/lsp_generated.go:DocumentHighlight
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentHighlight {
    /// The highlighted range in the document.
    pub range: Range,
    /// The access kind of this occurrence.
    pub kind: DocumentHighlightKind,
}

impl LanguageService {
    /// Returns the document highlights for the symbol of the token touching
    /// `position` in `file_name`: one [`DocumentHighlight`] per same-symbol
    /// occurrence in the file, tagged [`DocumentHighlightKind::Write`] for an
    /// assignment target / write-access declaration and
    /// [`DocumentHighlightKind::Read`] otherwise.
    ///
    /// Empty when there is no such file or the token is not a resolvable
    /// identifier.
    ///
    /// Side effects: binds every program file and allocates a checker (idempotent;
    /// via [`LanguageService::file_check_context`]).
    // Go: internal/ls/documenthighlights.go:LanguageService.ProvideDocumentHighlights
    pub fn provide_document_highlights(
        &mut self,
        file_name: &str,
        position: Position,
    ) -> Vec<DocumentHighlight> {
        let Some(script) = self.document_script(file_name) else {
            return Vec::new();
        };
        let byte_position = self
            .converters()
            .line_and_character_to_position(&script, position)
            .0;
        let Some(mut ctx) = self.file_check_context(file_name) else {
            return Vec::new();
        };
        // Reuse the find-all-references walk for the same-symbol occurrences in
        // the file (Go's `getSemanticDocumentHighlights` →
        // `getReferencedSymbolsForNode`).
        let nodes = same_symbol_reference_nodes(&mut ctx, byte_position);
        let nav = NavSourceFile::from_borrowed_arena(ctx.view.arena(), ctx.root, ctx.text.clone());
        let arena = nav.arena();
        // Compute each occurrence's range + access kind (Go's
        // `toDocumentHighlight`: Write when `IsWriteAccessForReference`, else
        // Read) while the view's arena is borrowed.
        let classified: Vec<(TextRange, DocumentHighlightKind)> = nodes
            .into_iter()
            .map(|node| {
                let range = TextRange::new(get_start_of_node(&nav, node, false), nav.end(node));
                let kind = if is_write_access_for_reference(arena, node) {
                    DocumentHighlightKind::Write
                } else {
                    DocumentHighlightKind::Read
                };
                (range, kind)
            })
            .collect();
        let converters = self.converters();
        classified
            .into_iter()
            .map(|(range, kind)| DocumentHighlight {
                range: converters.to_lsp_range(&script, range),
                kind,
            })
            .collect()
    }
}

/// The read/write/read-write access of a reference (Go's `ast.AccessKind`).
///
/// Side effects: none (a plain enum).
// Go: internal/ast/ast.go:AccessKind
#[derive(Clone, Copy, PartialEq, Eq)]
enum AccessKind {
    /// Only reads the reference (`x`).
    Read,
    /// Only writes the reference, never reading it (`x = 1`).
    Write,
    /// Both reads and writes (`x++`, `x += 1`).
    ReadWrite,
}

/// Reports whether `node` is a write access *as a reference* (the highlight
/// `Write` predicate): a write-access declaration name, the `default` keyword,
/// or a syntactic write access.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:IsWriteAccessForReference (reachable subset)
fn is_write_access_for_reference(arena: &NodeArena, node: NodeId) -> bool {
    if let Some(decl) = get_declaration_from_name(arena, node) {
        if declaration_is_write_access(arena, decl) {
            return true;
        }
    }
    // Go also treats `KindDefaultKeyword` as a write; that keyword path is not
    // reachable from the identifier walk.
    is_write_access(arena, node)
}

/// Returns the declaration `name` names, if `name` is the name of a declaration.
///
/// The reachable subset covers an identifier whose parent declaration's name is
/// `name` (reusing [`name_of_declaration`]); Go additionally handles qualified
/// names, computed property names, and JSDoc tags.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:GetDeclarationFromName (reachable subset)
fn get_declaration_from_name(arena: &NodeArena, name: NodeId) -> Option<NodeId> {
    let parent = arena.parent(name)?;
    if name_of_declaration(arena, parent) == Some(name) {
        Some(parent)
    } else {
        None
    }
}

/// Reports whether a declaration is considered a write access (it provides a
/// value), for the declaration kinds the reachable subset resolves to.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:declarationIsWriteAccess (reachable subset)
fn declaration_is_write_access(arena: &NodeArena, decl: NodeId) -> bool {
    match arena.data(decl) {
        // Always-write declaration kinds.
        NodeData::ParameterDeclaration(_)
        | NodeData::ClassDeclaration(_)
        | NodeData::ClassExpression(_)
        | NodeData::InterfaceDeclaration(_)
        | NodeData::EnumDeclaration(_)
        | NodeData::TypeAliasDeclaration(_)
        | NodeData::TypeParameterDeclaration(_) => true,
        // A variable / function declaration is a write only when it provides a
        // value (an initializer / a body). The catch-clause variable case is
        // deferred.
        NodeData::VariableDeclaration(d) => d.initializer.is_some(),
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.body.is_some(),
        // Other declaration kinds (signatures, accessors, properties, binding
        // elements, modules, enum members, ...) are deferred; Go panics on a
        // truly-unhandled kind, the reachable subset treats them as non-write.
        _ => false,
    }
}

/// Reports whether `node` is a syntactic write access (Go's `IsWriteAccess`:
/// `accessKind(node) != AccessKindRead`).
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:IsWriteAccess
fn is_write_access(arena: &NodeArena, node: NodeId) -> bool {
    access_kind(arena, node) != AccessKind::Read
}

/// The [`AccessKind`] of `node` based on its parent context.
///
/// The reachable subset covers assignment targets (`=` → write, compound `+=`
/// etc. → read-write) and prefix/postfix `++`/`--` (read-write). The
/// destructuring / property-access / for-in-of cases are deferred and treated
/// as reads.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:accessKind (reachable subset)
fn access_kind(arena: &NodeArena, node: NodeId) -> AccessKind {
    let Some(parent) = arena.parent(node) else {
        return AccessKind::Read;
    };
    match arena.data(parent) {
        NodeData::PrefixUnaryExpression(d) | NodeData::PostfixUnaryExpression(d) => {
            if matches!(d.operator, Kind::PlusPlusToken | Kind::MinusMinusToken) {
                AccessKind::ReadWrite
            } else {
                AccessKind::Read
            }
        }
        NodeData::BinaryExpression(d) if d.left == node => {
            let operator = arena.kind(d.operator_token);
            if is_assignment_operator(operator) {
                if operator == Kind::EqualsToken {
                    AccessKind::Write
                } else {
                    AccessKind::ReadWrite
                }
            } else {
                AccessKind::Read
            }
        }
        _ => AccessKind::Read,
    }
}

#[cfg(test)]
#[path = "documenthighlights_test.rs"]
mod tests;
