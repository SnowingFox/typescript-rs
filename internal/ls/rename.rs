//! Port of Go `internal/ls/rename.go`: the rename + prepare-rename feature.
//!
//! Go's `ProvideRename` resolves the symbol at a position, walks the program for
//! every same-symbol reference (the find-all-references machinery), and returns
//! one `lsproto.TextEdit` per reference grouped into a `WorkspaceEdit`. Go's
//! `GetRenameInfo` (the `textDocument/prepareRename` handler) validates that the
//! position is renamable and returns the trigger span + display name, or a
//! "cannot rename" result.
//!
//! # Reachable subset
//!
//! This round ports the single-file rename path:
//! [`LanguageService::provide_rename_locations`] returns the set of identifier
//! ranges to rename (the same-symbol references, reusing
//! [`same_symbol_reference_nodes`](crate::references::same_symbol_reference_nodes)),
//! and [`LanguageService::get_rename_info`] reports whether a position is
//! renamable (an identifier bound to a symbol with declarations) plus its
//! trigger span and display name, rejecting non-renamable positions (keywords,
//! punctuation) with a localized "cannot rename" result.
//!
//! DEFER(phase-7-ls): the full `lsproto.WorkspaceEdit`/`TextEdit` assembly
//! (grouping edits by document URI into a `Changes` map and computing the
//! replacement text via `getTextForRename`) — the reachable subset returns the
//! location list, and `WorkspaceEdit` is not yet present in `tsgo_lsproto`;
//! cross-file rename (the program-wide reference search); the shorthand-property
//! / import-export prefix/suffix rename text (`getTextForRename`); string-literal
//! and module-specifier rename (`getRenameInfoForModule`); the library-file /
//! `node_modules` / `default`-import rename-blocked reasons
//! (`renameBlockedReason`); and the keyword/modifier trigger adjustment
//! (`getAdjustedLocation`).
//! blocked-by: a `compiler.Program`-level multi-file symbol resolver, a
//! `tsgo_lsproto::WorkspaceEdit`, `GetContextualType`/`ResolveAlias`, and the
//! `UserPreferences`/quote-preference surface.

use tsgo_ast::{Kind, NodeId};
use tsgo_astnav::{get_start_of_node, NavSourceFile};
use tsgo_checker::{get_symbol_at_location, symbol_to_string};
use tsgo_core::text::TextRange;
use tsgo_diagnostics::YOU_CANNOT_RENAME_THIS_ELEMENT;
use tsgo_locale::Locale;
use tsgo_lsproto::{Location, Position, Range};

use crate::languageservice::{FileCheckContext, LanguageService};
use crate::references::reference_ranges;

/// The result of a rename validation check (Go's `ls.RenameInfo`), returned by
/// [`LanguageService::get_rename_info`] for the `textDocument/prepareRename`
/// handler.
///
/// `can_rename` is `false` together with a `localized_error_message` when the
/// position is not renamable; otherwise `display_name` is the symbol's printed
/// name and `trigger_span` is the editor range the rename UI highlights.
///
/// Side effects: none (a plain data record).
// Go: internal/ls/rename.go:RenameInfo
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RenameInfo {
    /// Whether the element at the queried position can be renamed.
    pub can_rename: bool,
    /// The localized "cannot rename" message when `can_rename` is `false`.
    pub localized_error_message: String,
    /// The printed name of the symbol being renamed (Go's `symbolToString`).
    pub display_name: String,
    /// The editor range the rename UI highlights for the trigger token.
    pub trigger_span: Range,
}

impl LanguageService {
    /// Returns the rename locations for the symbol of the token touching
    /// `position` in `file_name`: one [`lsproto::Location`] per identifier in the
    /// file that resolves to the same symbol (the declaration plus every use).
    ///
    /// Empty when there is no such file or the position is not renamable.
    ///
    /// Side effects: binds every program file and allocates a checker (idempotent;
    /// via [`LanguageService::file_check_context`]).
    // Go: internal/ls/rename.go:LanguageService.ProvideRename / symbolAndEntriesToRename
    pub fn provide_rename_locations(
        &mut self,
        file_name: &str,
        position: Position,
    ) -> Vec<Location> {
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
        // Go's `symbolAndEntriesToRename` only produces edits when the node is
        // eligible for rename and `getRenameInfoForNode` reports `CanRename`;
        // otherwise it returns an empty (null) workspace edit.
        if rename_target(&mut ctx, byte_position).is_none() {
            return Vec::new();
        }
        // The rename locations are exactly the same-symbol references (Go's
        // `getRenameLocations` flat-maps `getReferencedSymbolsForNode`).
        let ranges = reference_ranges(&mut ctx, byte_position);
        let converters = self.converters();
        ranges
            .into_iter()
            .map(|range| converters.to_lsp_location(&script, range))
            .collect()
    }

    /// Reports whether the token touching `position` in `file_name` can be
    /// renamed (an identifier bound to a symbol with declarations), returning the
    /// trigger span + display name, or a localized "cannot rename" result.
    ///
    /// Mirrors the reachable subset of Go's `GetRenameInfo`: resolve the touching
    /// property name, check `nodeIsEligibleForRename`, then `getRenameInfoForNode`
    /// (a symbol with at least one declaration, not blocked) and build a
    /// `RenameInfo` with the node's UTF-16 trigger span and the symbol's printed
    /// name.
    ///
    /// Go also takes the proposed `newName`, but it only feeds the deferred
    /// module-specifier rename path (`getRenameInfoForModule`); the reachable
    /// subset needs only the position.
    ///
    /// Side effects: binds every program file and allocates a checker (idempotent;
    /// via [`LanguageService::file_check_context`]).
    // Go: internal/ls/rename.go:LanguageService.GetRenameInfo / getRenameInfoForNode
    pub fn get_rename_info(&mut self, file_name: &str, position: Position) -> RenameInfo {
        let Some(script) = self.document_script(file_name) else {
            return rename_info_error();
        };
        let byte_position = self
            .converters()
            .line_and_character_to_position(&script, position)
            .0;
        let Some(mut ctx) = self.file_check_context(file_name) else {
            return rename_info_error();
        };
        let Some((node, display_name)) = rename_target(&mut ctx, byte_position) else {
            return rename_info_error();
        };
        // Go's `getRenameInfoSuccess`: the trigger span is the node's start..end
        // (the quote-excluding string-literal case is deferred).
        let nav = NavSourceFile::from_borrowed_arena(ctx.view.arena(), ctx.root, ctx.text.clone());
        let span = TextRange::new(get_start_of_node(&nav, node, false), nav.end(node));
        let converters = self.converters();
        RenameInfo {
            can_rename: true,
            localized_error_message: String::new(),
            display_name,
            trigger_span: converters.to_lsp_range(&script, span),
        }
    }
}

/// The localized "you cannot rename this element" result (Go's
/// `getRenameInfoError(ctx, You_cannot_rename_this_element)`).
///
/// Side effects: none (pure).
// Go: internal/ls/rename.go:getRenameInfoError
fn rename_info_error() -> RenameInfo {
    let locale = Locale::default();
    RenameInfo {
        can_rename: false,
        localized_error_message: YOU_CANNOT_RENAME_THIS_ELEMENT.localize(&locale, &[]),
        display_name: String::new(),
        trigger_span: Range::default(),
    }
}

/// Resolves the token at `position` (a byte offset) to the renamable identifier
/// node and its display name, or `None` if the position is not renamable.
///
/// Mirrors the reachable subset of `nodeIsEligibleForRename` +
/// `getRenameInfoForNode`: the token must be an identifier bound to a symbol
/// with at least one declaration. The library-file / `node_modules` / `default`
/// rename-blocked reasons are not reachable single-file, so a renamable symbol
/// is always allowed here.
///
/// Side effects: resolves the symbol through the checker (may cache).
// Go: internal/ls/rename.go:getRenameInfoForNode (reachable subset)
fn rename_target(ctx: &mut FileCheckContext, position: i32) -> Option<(NodeId, String)> {
    let nav = NavSourceFile::from_borrowed_arena(ctx.view.arena(), ctx.root, ctx.text.clone());
    let node = nav.get_touching_property_name(position);
    if !node_is_eligible_for_rename(nav.kind(node)) {
        return None;
    }
    let globals = ctx.view.globals().cloned();
    let symbol =
        get_symbol_at_location(&mut ctx.checker, ctx.view.as_ref(), node, globals.as_ref())?;
    // Only allow a symbol to be renamed if it actually has a declaration.
    if ctx.view.symbol(symbol).declarations.is_empty() {
        return None;
    }
    let display_name = symbol_to_string(ctx.view.as_ref(), symbol);
    Some((node, display_name))
}

/// Reports whether a node of `kind` is a position the rename feature accepts.
///
/// The reachable subset accepts identifiers; Go additionally accepts string /
/// no-substitution-template literals, `this`, and property-name numeric
/// literals, which are deferred with the string-literal / module-rename paths.
///
/// Side effects: none (pure).
// Go: internal/ls/rename.go:nodeIsEligibleForRename (reachable subset)
fn node_is_eligible_for_rename(kind: Kind) -> bool {
    matches!(kind, Kind::Identifier | Kind::PrivateIdentifier)
}

#[cfg(test)]
#[path = "rename_test.rs"]
mod tests;
