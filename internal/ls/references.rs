//! Port of Go `internal/ls/findallreferences.go`: the find-all-references feature.
//!
//! Go's `ProvideReferences` resolves the symbol at a position
//! (`getReferencedSymbolsForNode` → `GetSymbolAtLocation`) and walks the program
//! for identifiers that resolve to the same symbol (`getReferencesInContainer` →
//! `getPossibleSymbolReferencePositions` → `getReferencesAtLocation`, comparing
//! via `getRelatedSymbol`), returning one `lsproto.Location` per match —
//! including the declaration, when `IncludeDeclaration` is set (its default).
//!
//! # Reachable subset
//!
//! This round ports the single-file, single-symbol path:
//! [`LanguageService::provide_references`] resolves the touched identifier to its
//! symbol, then walks every identifier in the file (Go scans candidate name
//! positions; the reachable subset walks the parsed identifier nodes — the same
//! set), keeps those resolving to the *same* symbol, and returns their UTF-16
//! ranges (via the project [`Converters`](tsgo_ls_lsconv::Converters)), deduped
//! by range and ordered by source position. Scope-aware resolution
//! (`get_symbol_at_location` → `resolve_name`) makes the search shadowing-correct.
//! The declaration is included (Go's default `IncludeDeclaration`), because its
//! name identifier resolves to the same symbol.
//!
//! DEFER(phase-7-ls): cross-file references (the program-wide
//! `getReferencesInContainerOrFiles` global search), the reference special-cases
//! — string-literal references (`getReferencesForStringLiteral`), the
//! triple-slash / module-symbol path, label references, import/export specifier
//! references (`getReferencesAtExportSpecifier`/`getImportOrExportReferences`),
//! constructor / `super` / static-`this` references, and shorthand-property
//! references — and the read/write-access classification, the `LocationLink`
//! context ranges, and the `IncludeDeclaration == false` filtering.
//! blocked-by: a `compiler.Program`-level multi-file symbol resolver, the
//! `getRelatedSymbol`/root-symbol machinery, and `GetContextualType` for
//! string-literal references.

use tsgo_ast::{Kind, NodeArena, NodeId};
use tsgo_astnav::{get_start_of_node, NavSourceFile};
use tsgo_checker::get_symbol_at_location;
use tsgo_core::text::TextRange;
use tsgo_lsproto::{Location, Position};

use crate::languageservice::{FileCheckContext, LanguageService};

impl LanguageService {
    /// Returns the reference locations for the symbol of the token touching
    /// `position` in `file_name`: one [`lsproto::Location`] per identifier in the
    /// file that resolves to the same symbol, including the declaration.
    ///
    /// The list is empty when there is no such file, the position is on the
    /// source file as a whole, or the token is not a resolvable identifier.
    ///
    /// Mirrors the reachable subset of Go's `ProvideReferences` →
    /// `getReferencedSymbolsForNode`: resolve the symbol, then collect the
    /// same-symbol identifier ranges across the file (UTF-16), ordered by source
    /// position.
    ///
    /// Side effects: binds every program file and allocates a checker (idempotent;
    /// via [`LanguageService::file_check_context`]).
    // Go: internal/ls/findallreferences.go:LanguageService.ProvideReferences
    pub fn provide_references(&mut self, file_name: &str, position: Position) -> Vec<Location> {
        let Some(script) = self.document_script(file_name) else {
            return Vec::new();
        };
        // Convert the LSP `(line, character)` to a byte offset first (immutable
        // borrows), so the checking context can take `&mut self` afterwards.
        let byte_position = self
            .converters()
            .line_and_character_to_position(&script, position)
            .0;
        let Some(mut ctx) = self.file_check_context(file_name) else {
            return Vec::new();
        };
        let ranges = reference_ranges(&mut ctx, byte_position);
        let converters = self.converters();
        ranges
            .into_iter()
            .map(|range| converters.to_lsp_location(&script, range))
            .collect()
    }
}

/// Resolves the token at `position` (a byte offset) in `ctx` to the source
/// ranges of every same-symbol identifier in the file.
///
/// Side effects: resolves symbols through the checker (may cache).
// Go: internal/ls/findallreferences.go:getReferencedSymbolsForNode (single-file body)
fn reference_ranges(ctx: &mut FileCheckContext, position: i32) -> Vec<TextRange> {
    let nav = NavSourceFile::from_borrowed_arena(ctx.view.arena(), ctx.root, ctx.text.clone());
    let node = nav.get_touching_property_name(position);

    // As in definition/hover: only real identifier nodes resolve (astnav
    // synthesizes keyword/punctuation tokens not in the checker's arena, and the
    // source-file-as-a-whole resolves to no symbol).
    if !matches!(nav.kind(node), Kind::Identifier | Kind::PrivateIdentifier) {
        return Vec::new();
    }

    let globals = ctx.view.globals().cloned();
    let Some(search_symbol) =
        get_symbol_at_location(&mut ctx.checker, ctx.view.as_ref(), node, globals.as_ref())
    else {
        return Vec::new();
    };

    // Collect the candidate identifier nodes (same name as the searched symbol),
    // mirroring Go's `getPossibleSymbolReferencePositions` text scan + token
    // resolution, but over the parsed identifier nodes.
    let name = ctx.view.symbol(search_symbol).name.clone();
    let mut candidates: Vec<NodeId> = Vec::new();
    collect_named_identifiers(nav.arena(), nav.root(), &name, &mut candidates);

    let mut ranges: Vec<TextRange> = Vec::new();
    for candidate in candidates {
        let resolved = get_symbol_at_location(
            &mut ctx.checker,
            ctx.view.as_ref(),
            candidate,
            globals.as_ref(),
        );
        if resolved != Some(search_symbol) {
            continue;
        }
        let range = TextRange::new(
            get_start_of_node(&nav, candidate, false),
            nav.end(candidate),
        );
        if !ranges.contains(&range) {
            ranges.push(range);
        }
    }
    ranges
}

/// Appends every identifier / private-identifier node under `node` whose text
/// equals `name`, in source order.
///
/// Side effects: none (pushes onto `out`).
// Go: internal/ls/findallreferences.go:getPossibleSymbolReferenceNodes (reachable subset)
fn collect_named_identifiers(arena: &NodeArena, node: NodeId, name: &str, out: &mut Vec<NodeId>) {
    arena.for_each_child(node, &mut |child| {
        if matches!(
            arena.kind(child),
            Kind::Identifier | Kind::PrivateIdentifier
        ) && arena.text(child) == name
        {
            out.push(child);
        }
        collect_named_identifiers(arena, child, name, out);
        false
    });
}

#[cfg(test)]
#[path = "references_test.rs"]
mod tests;
