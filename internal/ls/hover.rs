//! Port of Go `internal/ls/hover.go`: the quick-info / hover feature.
//!
//! Go's `ProvideHover` resolves the token touching a position
//! ([`astnav.GetTouchingPropertyName`]), finds its symbol
//! (`getSymbolAtLocationForQuickInfo`), and builds classified display parts via
//! `getQuickInfoAndDeclarationAtLocation` (a large routine that prefixes the
//! symbol kind — `const x: `, `function f`, `(property) p:` … — and renders the
//! type, type parameters, signatures, and JSDoc documentation), then wraps the
//! result in an `lsproto.Hover` with the node's range.
//!
//! # Reachable subset
//!
//! This round ports the *tracer* through that pipeline: resolve the token, get
//! its symbol, get the symbol's type ([`get_type_of_symbol`]), and render the
//! type string ([`type_to_string`]). [`LanguageService::get_quick_info_at_position`]
//! returns that type string plus the node's span as a [`QuickInfo`];
//! [`LanguageService::provide_hover`] wraps it into an [`lsproto::Hover`] with a
//! plain-text [`MarkupContent`] and the UTF-16 range.
//!
//! DEFER(phase-7-ls): the full classified display parts (the `const`/`let`/
//! `function`/`(property)`/`(parameter)`/`type`/`interface`/`class`/… prefixes,
//! type-parameter and signature rendering, the `alias`/`enum member` cases), the
//! JSDoc documentation, the markdown code-fence formatting, and the
//! verbosity-level expansion. Also deferred: the `GetTypeAtLocation` fallback for
//! a `this`/expression node with no resolvable symbol (Go's `shouldGetType`),
//! and initializer-inferred types for an un-annotated `const x = 1` (the checker
//! yields `any` until initializer inference lands).
//! blocked-by: the checker's display-parts / `nodebuilder` classification
//! surface, the JSDoc reparser, and `get_type_at_location` / initializer
//! inference.

use tsgo_ast::Kind;
use tsgo_astnav::{get_start_of_node, NavSourceFile};
use tsgo_checker::{get_symbol_at_location, get_type_of_symbol, type_to_string};
use tsgo_core::text::TextRange;
use tsgo_lsproto::{
    Hover, MarkupContent, MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings,
    MarkupKind, Position,
};

use crate::languageservice::{FileCheckContext, LanguageService};

/// The reachable subset of Go's quick-info: the resolved type string for the
/// token at a position, plus that token's source span.
///
/// Go's quick info is a rich, classified display-parts string (the symbol-kind
/// prefix, the type, type parameters, signatures, …). This round carries only
/// the resolved type string (see the [`crate::hover`] module note).
///
/// Side effects: none (plain data).
// Go: internal/ls/hover.go:symbolDisplayInfo (reachable subset)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickInfo {
    /// The resolved type string for the token (e.g. `number`), the reachable
    /// subset of Go's classified display parts.
    pub text: String,
    /// The token's source span (internal UTF-8 byte offsets).
    pub text_range: TextRange,
}

impl LanguageService {
    /// Returns the quick info for the token touching `position` in `file_name`,
    /// or `None` if there is no such file, the position is on the source file as
    /// a whole, or the token has no resolvable symbol/type.
    ///
    /// Mirrors the tracer through Go's `ProvideHover`: convert the LSP position
    /// to a byte offset, resolve the touching property name
    /// ([`astnav`](tsgo_astnav)), get its symbol and the symbol's type, and
    /// render the type string ([`type_to_string`]).
    ///
    /// Side effects: binds every program file and allocates a checker (idempotent;
    /// via [`LanguageService::file_check_context`]).
    // Go: internal/ls/hover.go:LanguageService.ProvideHover (quick-info tracer)
    pub fn get_quick_info_at_position(
        &mut self,
        file_name: &str,
        position: Position,
    ) -> Option<QuickInfo> {
        // Convert the LSP `(line, character)` to a byte offset first (immutable
        // borrows), so the checking context can take `&mut self` afterwards.
        let script = self.document_script(file_name)?;
        let byte_position = self
            .converters()
            .line_and_character_to_position(&script, position)
            .0;
        let mut ctx = self.file_check_context(file_name)?;
        quick_info_at(&mut ctx, byte_position)
    }

    /// Returns an [`lsproto::Hover`] for the token touching `position`, wrapping
    /// the [`QuickInfo`] type string in a plain-text [`MarkupContent`] and the
    /// token's span as a UTF-16 [`Range`](tsgo_lsproto::Range).
    ///
    /// Side effects: as [`Self::get_quick_info_at_position`].
    // Go: internal/ls/hover.go:LanguageService.ProvideHover
    pub fn provide_hover(&mut self, file_name: &str, position: Position) -> Option<Hover> {
        let script = self.document_script(file_name)?;
        let quick_info = self.get_quick_info_at_position(file_name, position)?;
        let range = self
            .converters()
            .to_lsp_range(&script, quick_info.text_range);
        Some(Hover {
            contents: MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings {
                markup_content: Some(MarkupContent {
                    kind: MarkupKind::PLAIN_TEXT,
                    value: quick_info.text,
                }),
                ..Default::default()
            },
            range: Some(range),
            can_increase_verbosity: None,
        })
    }
}

/// Resolves the token at `position` (a byte offset) in `ctx` to its
/// [`QuickInfo`].
///
/// Side effects: resolves symbols/types through the checker (may cache).
// Go: internal/ls/hover.go:LanguageService.ProvideHover (body)
fn quick_info_at(ctx: &mut FileCheckContext, position: i32) -> Option<QuickInfo> {
    // Find the touching token and its span while the navigation borrow of the
    // view's arena is held; the resulting node id is valid in that arena.
    let (node, text_range) = {
        let nav = NavSourceFile::from_borrowed_arena(ctx.view.arena(), ctx.root, ctx.text.clone());
        let node = nav.get_touching_property_name(position);
        // The reachable quick-info resolves only real identifier nodes. Keyword
        // and punctuation tokens that the parser does not keep as nodes are
        // *synthesized* by astnav into its own side store (a high-bit-tagged id
        // that is not in the checker's arena), so feeding one to the checker
        // would index out of bounds; the source-file-as-a-whole (Go's
        // `IsSourceFile` guard) and other non-identifier kinds resolve to no
        // symbol anyway.
        // DEFER(phase-7-ls): `this`/`super`/meta-property quick info via
        // `GetTypeAtLocation` (Go's `shouldGetType`), and property-access /
        // qualified-name resolution.
        if !matches!(nav.kind(node), Kind::Identifier | Kind::PrivateIdentifier) {
            return None;
        }
        let start = get_start_of_node(&nav, node, true);
        let end = nav.end(node);
        (node, TextRange::new(start, end))
    };

    let globals = ctx.view.globals().cloned();
    let symbol =
        get_symbol_at_location(&mut ctx.checker, ctx.view.as_ref(), node, globals.as_ref())?;
    let ty = get_type_of_symbol(
        &mut ctx.checker,
        ctx.view.as_ref(),
        symbol,
        globals.as_ref(),
    );
    let text = type_to_string(&mut ctx.checker, ctx.view.as_ref(), ty);
    Some(QuickInfo { text, text_range })
}

#[cfg(test)]
#[path = "hover_test.rs"]
mod tests;
