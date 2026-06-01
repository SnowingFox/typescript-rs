//! Port of Go `internal/ls/definition.go`: the go-to-definition feature.
//!
//! Go's `ProvideDefinition` resolves the token touching a position
//! ([`astnav.GetTouchingPropertyName`]), gets its symbol (the checker's
//! `getDeclarationsFromLocation` → `GetSymbolAtLocation`), and returns an
//! `lsproto.Location` per declaration — the range of each declaration's *name*
//! (`createDefinitionLocations` uses `core.OrElse(GetNameOfDeclaration(decl),
//! decl)` then `createRangeFromNode`).
//!
//! # Reachable subset
//!
//! This round ports the single-file value/declaration path:
//! [`LanguageService::provide_definition`] resolves the touched identifier to its
//! symbol, and returns one [`lsproto::Location`] per declaration, ranged on the
//! declaration's name (UTF-16 via the project [`Converters`](tsgo_ls_lsconv::Converters)),
//! deduped by `(file, range)`. A local variable / function / class / parameter
//! *use* jumps to its declaration's name.
//!
//! DEFER(phase-7-ls): cross-file / module-resolution definitions (Go's
//! `getReferenceAtPosition` triple-slash / module-specifier path and the
//! `LocationLink`/`clientSupportsLink` shape), and the keyword / special-case
//! targets — `override` member (`getSymbolForOverriddenMember`), jump-statement
//! labels (`IsJumpStatementTarget`), `case`/`default`, `return`/`yield`/`await`
//! to the enclosing function, the called-signature / constructor disambiguation
//! (`tryGetSignatureDeclaration`), shorthand-property / object-literal-element
//! contextual declarations, binding-pattern property declarations, alias
//! resolution, and index-signature targets.
//! blocked-by: a `compiler.Program`-level multi-file symbol/module resolver,
//! `GetResolvedSignature`/`GetContextualType`/`ResolveAlias`, and the
//! `GetClientCapabilities` link-support surface.

use tsgo_ast::{Kind, NodeArena, NodeData, NodeId};
use tsgo_astnav::{get_start_of_node, NavSourceFile};
use tsgo_checker::get_symbol_at_location;
use tsgo_core::text::TextRange;
use tsgo_lsproto::{Location, Position};

use crate::languageservice::{FileCheckContext, LanguageService};

impl LanguageService {
    /// Returns the definition locations for the token touching `position` in
    /// `file_name`: one [`lsproto::Location`] per declaration of the resolved
    /// symbol, ranged on the declaration's name.
    ///
    /// The list is empty when there is no such file, the position is on the
    /// source file as a whole (Go's `node.Kind == ast.KindSourceFile` guard),
    /// the token is not a resolvable identifier, or the symbol has no
    /// declarations.
    ///
    /// Mirrors the reachable subset of Go's `ProvideDefinition` →
    /// `getDefinitionAtPosition`: convert the LSP position to a byte offset,
    /// resolve the touching property name ([`astnav`](tsgo_astnav)), get its
    /// symbol, and map each declaration to the UTF-16 range of its name.
    ///
    /// Side effects: binds every program file and allocates a checker (idempotent;
    /// via [`LanguageService::file_check_context`]).
    // Go: internal/ls/definition.go:LanguageService.ProvideDefinition / provideDefinitionWorker
    pub fn provide_definition(&mut self, file_name: &str, position: Position) -> Vec<Location> {
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
        let ranges = definition_ranges(&mut ctx, byte_position);
        let converters = self.converters();
        ranges
            .into_iter()
            .map(|range| converters.to_lsp_location(&script, range))
            .collect()
    }
}

/// Resolves the token at `position` (a byte offset) in `ctx` to the source
/// ranges of its symbol's declaration names.
///
/// Side effects: resolves symbols through the checker (may cache).
// Go: internal/ls/definition.go:getDefinitionAtPosition (body)
fn definition_ranges(ctx: &mut FileCheckContext, position: i32) -> Vec<TextRange> {
    let nav = NavSourceFile::from_borrowed_arena(ctx.view.arena(), ctx.root, ctx.text.clone());
    let node = nav.get_touching_property_name(position);

    // Go returns an empty response for `node.Kind == ast.KindSourceFile`; the
    // reachable subset only resolves real identifier nodes (astnav synthesizes
    // keyword/punctuation tokens that are not in the checker's arena, so feeding
    // one to the checker would index out of bounds — see `hover.rs`).
    if !matches!(nav.kind(node), Kind::Identifier | Kind::PrivateIdentifier) {
        return Vec::new();
    }

    let globals = ctx.view.globals().cloned();
    let Some(symbol) =
        get_symbol_at_location(&mut ctx.checker, ctx.view.as_ref(), node, globals.as_ref())
    else {
        return Vec::new();
    };

    let declarations = ctx.view.symbol(symbol).declarations.clone();
    let arena = ctx.view.arena();
    let mut ranges: Vec<TextRange> = Vec::new();
    for decl in declarations {
        // `core.OrElse(ast.GetNameOfDeclaration(decl), decl)`: range the
        // declaration's name when it has one, else the declaration node itself.
        let name = name_of_declaration(arena, decl).unwrap_or(decl);
        let range = TextRange::new(get_start_of_node(&nav, name, false), nav.end(name));
        if !ranges.contains(&range) {
            ranges.push(range);
        }
    }
    ranges
}

/// Returns the "name" child of a declaration node, for the declaration kinds the
/// reachable subset resolves to (a value / type / parameter declaration). More
/// kinds are added as their definition paths land.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetNameOfDeclaration (reachable subset)
pub(crate) fn name_of_declaration(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::VariableDeclaration(d) => Some(d.name),
        NodeData::ParameterDeclaration(d) => Some(d.name),
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.name,
        NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d)
        | NodeData::InterfaceDeclaration(d) => d.name,
        NodeData::TypeAliasDeclaration(d) => Some(d.name),
        NodeData::EnumDeclaration(d) => Some(d.name),
        NodeData::MethodDeclaration(d) => Some(d.name),
        NodeData::PropertyDeclaration(d) | NodeData::PropertySignature(d) => Some(d.name),
        NodeData::TypeParameterDeclaration(d) => Some(d.name),
        _ => None,
    }
}

#[cfg(test)]
#[path = "definition_test.rs"]
mod tests;
