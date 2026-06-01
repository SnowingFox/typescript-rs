//! Port of Go `internal/ls/signaturehelp.go`: the signature-help feature (the
//! parameter-hints popup shown inside a call's argument list).
//!
//! Go's `ProvideSignatureHelp` → `GetSignatureHelpItems` finds the call / `new`
//! expression whose argument list contains the position
//! (`getContainingArgumentInfo` → `getImmediatelyContainingArgumentInfo`), gets
//! its candidate signatures (`getCandidateOrTypeInfo` →
//! `GetResolvedSignatureForSignatureHelp` / `getSignaturesOfType`), determines
//! the active parameter index (counting completed arguments / commas before the
//! position), and builds an `lsproto.SignatureHelp` (signatures + active
//! signature + active parameter) where each signature's label is rendered by
//! `createSignatureHelpItems` / `itemInfoForParameters` (the call-target name,
//! the `(p1: T1, p2: T2)` parameter list, and the `: R` return type).
//!
//! # Reachable subset
//!
//! [`LanguageService::provide_signature_help`] ports the tracer through that
//! pipeline for the common case — a non-overloaded `f(...)` call:
//!
//! - **Find the enclosing call**: from the token preceding the position, climb
//!   the arena parents to the nearest [`CallExpression`](tsgo_ast::Kind::CallExpression)
//!   / [`NewExpression`](tsgo_ast::Kind::NewExpression) whose argument list spans
//!   the position (the reachable subset of `getContainingArgumentInfo`).
//! - **Resolve the signatures**: type the callee
//!   ([`get_symbol_at_location`] + [`get_type_of_symbol`], as `hover` /
//!   `completions` do) and read its call signatures
//!   ([`Checker::get_signatures_of_type`](tsgo_checker::Checker::get_signatures_of_type)).
//! - **Render each signature**: the call-target name ([`symbol_to_string`]) plus
//!   the `(name: type, …)` parameter list and the `: returnType`
//!   ([`type_to_string`]), mirroring `createSignatureHelpItems` /
//!   `itemInfoForParameters` for the reachable subset. Each parameter becomes a
//!   [`ParameterInformation`] whose label is the `name: type` substring (Go's
//!   string-form `signatureHelpParameter`).
//! - **Active parameter**: the number of completed arguments before the position
//!   (the reachable subset of `getArgumentIndex`'s comma counting).
//!
//! Because `tsgo_lsproto` does not yet generate `SignatureHelp` /
//! `SignatureInformation` / `ParameterInformation`, this module defines the
//! LSP-shaped types locally (mirroring `lsproto.SignatureHelp` et al.), the same
//! way [`crate::documenthighlights`] / [`crate::symbols`] define their feature
//! types.
//!
//! DEFER(phase-7-ls): overloaded-call signature selection (multiple candidates +
//! active-signature picking beyond the first/resolved one — Go's
//! `createSignatureHelpItems` `selectedItemIndex` arity loop), generic signature
//! instantiation display, type-argument signature help (`f<|>`),
//! JSX-attribute / tagged-template signature help, the contextual signatures for
//! callback parameters (`tryGetParameterInfo`), the documentation / JSDoc-tag
//! rendering, the per-signature / null-active-parameter client-capability
//! handling, the variadic active-parameter clamping
//! (`computeActiveParameter`), and the trailing-comma / whitespace
//! active-parameter edge cases beyond the reachable subset. Also DEFER:
//! **constructor (`new C(...)`) signatures** — a class value symbol's type is its
//! instance type (no call signatures), and the only public checker API
//! ([`Checker::get_signatures_of_type`](tsgo_checker::Checker::get_signatures_of_type))
//! returns call signatures, not construct signatures, so `new C(|)` yields no
//! help yet.
//! blocked-by: `GetResolvedSignatureForSignatureHelp` (overload resolution),
//! generic call-site inference, the type-argument / JSX / tagged-template
//! argument-info cases, contextual typing (`GetContextualType`), the JSDoc
//! reparser, the `GetClientCapabilities` signature-help surface, class
//! construct-signature collection + the static-side class type, and an
//! `lsproto`-generated `SignatureHelp`.

use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, SymbolTable};
use tsgo_astnav::NavSourceFile;
use tsgo_checker::{
    get_symbol_at_location, get_type_of_symbol, symbol_to_string, type_to_string, SignatureId,
};
use tsgo_lsproto::Position;

use crate::languageservice::{FileCheckContext, LanguageService};

/// The reachable subset of Go's `lsproto.SignatureHelp`: the signatures for a
/// call plus the active signature and active parameter indices.
///
/// Side effects: none (plain data).
// Go: internal/lsp/lsproto/lsp_generated.go:SignatureHelp (reachable subset)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelp {
    /// The candidate signatures (Go's `Signatures`).
    pub signatures: Vec<SignatureInformation>,
    /// The active signature index (Go's `ActiveSignature`); always `Some(0)` in
    /// the reachable subset (overload selection is deferred).
    pub active_signature: Option<u32>,
    /// The active parameter index of the active signature (Go's
    /// `ActiveParameter`): the number of completed arguments before the cursor.
    pub active_parameter: Option<u32>,
}

/// The reachable subset of Go's `lsproto.SignatureInformation`: one callable
/// signature's rendered label and its parameters.
///
/// Side effects: none (plain data).
// Go: internal/lsp/lsproto/lsp_generated.go:SignatureInformation (reachable subset)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureInformation {
    /// The full signature label shown in the UI, e.g. `f(a: number): void`
    /// (Go's `Label`).
    pub label: String,
    /// The signature's parameters, in order (Go's `Parameters`).
    pub parameters: Vec<ParameterInformation>,
}

/// The reachable subset of Go's `lsproto.ParameterInformation`: one parameter's
/// label.
///
/// Go's `Label` is a `StringOrTuple`; the reachable subset always uses the
/// string form (Go's `createSignatureHelpParameterFromLabel`), the `name: type`
/// substring of the signature label. The label-offset tuple form and the
/// per-parameter documentation are deferred.
///
/// Side effects: none (plain data).
// Go: internal/lsp/lsproto/lsp_generated.go:ParameterInformation (reachable subset)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterInformation {
    /// The parameter's label, the `name: type` substring of the signature label
    /// (Go's `Label.String`).
    pub label: String,
}

impl LanguageService {
    /// Returns the [`SignatureHelp`] for the call / `new` expression whose
    /// argument list contains `position` in `file_name`, or `None` when there is
    /// no such file, the position is not inside a call's argument list, or the
    /// callee resolves to no call signatures.
    ///
    /// Mirrors the reachable subset of Go's `ProvideSignatureHelp` →
    /// `GetSignatureHelpItems`: convert the LSP position to a byte offset, find
    /// the enclosing call, resolve and render its signatures, and compute the
    /// active parameter.
    ///
    /// Side effects: binds every program file and allocates a checker
    /// (idempotent; via [`LanguageService::file_check_context`]).
    // Go: internal/ls/signaturehelp.go:LanguageService.ProvideSignatureHelp / GetSignatureHelpItems
    pub fn provide_signature_help(
        &mut self,
        file_name: &str,
        position: Position,
    ) -> Option<SignatureHelp> {
        // Convert the LSP `(line, character)` to a byte offset first (immutable
        // borrows), so the checking context can take `&mut self` afterwards.
        let script = self.document_script(file_name)?;
        let byte_position = self
            .converters()
            .line_and_character_to_position(&script, position)
            .0;
        let mut ctx = self.file_check_context(file_name)?;
        signature_help_at(&mut ctx, byte_position)
    }
}

/// Resolves the signature help for `position` (a byte offset) in `ctx`.
///
/// Finds the enclosing call under a navigation borrow of the view's arena
/// (extracting only the call's node id), then resolves and renders its
/// signatures through the checker — the same borrow discipline `hover` /
/// `completions` use.
///
/// Side effects: resolves symbols/types through the checker (may cache).
// Go: internal/ls/signaturehelp.go:LanguageService.GetSignatureHelpItems (body)
fn signature_help_at(ctx: &mut FileCheckContext, position: i32) -> Option<SignatureHelp> {
    // Find the enclosing call under a navigation borrow; the nav view is dropped
    // before the checker is used (as in `hover` / `completions`). The returned
    // node id is valid in the view's shared arena.
    let call = {
        let nav = NavSourceFile::from_borrowed_arena(ctx.view.arena(), ctx.root, ctx.text.clone());
        find_enclosing_call(&nav, position)?
    };
    build_signature_help(ctx, call, position)
}

/// Climbs from the token preceding `position` to the nearest call / `new`
/// expression whose argument list contains `position`.
///
/// Mirrors the reachable subset of Go's `getContainingArgumentInfo` walk: the
/// starting token's enclosing call. Because a `(` / `,` token is *synthesized*
/// by `astnav` (no arena parent — unlike Go's `n.Parent`), the walk anchors on
/// the nearest real node at/before the position and climbs its arena parents.
///
/// Side effects: may synthesize navigation tokens (interior mutability).
// Go: internal/ls/signaturehelp.go:getContainingArgumentInfo / getImmediatelyContainingArgumentInfo
fn find_enclosing_call(nav: &NavSourceFile, position: i32) -> Option<NodeId> {
    let starting = nav.find_preceding_token(position)?;
    // A synthesized `(` / `,` token has no arena parent, so step left to the
    // real token before it (the callee, or the previous argument).
    let anchor = if is_synthesized_token(starting) {
        let before = nav.find_preceding_token(nav.pos(starting))?;
        if is_synthesized_token(before) {
            return None;
        }
        before
    } else {
        starting
    };

    let arena = nav.arena();
    let mut node = Some(anchor);
    while let Some(n) = node {
        if matches!(arena.kind(n), Kind::CallExpression | Kind::NewExpression) {
            if let Some((callee, _args)) = call_parts(arena, n) {
                let callee_end = arena.loc(callee).end();
                let call_end = arena.loc(n).end();
                // The position must sit inside the argument-list region: after
                // the callee (and the `(`), up to the end of the call. This
                // excludes a cursor on the callee itself (Go's
                // `findContainingList` returning nil for the call target).
                if position > callee_end && position <= call_end {
                    return Some(n);
                }
            }
        }
        node = arena.parent(n);
    }
    None
}

/// Returns the callee expression and argument list of a call / `new` expression,
/// or `None` for a `new X` with no argument list (no `(...)`).
///
/// Side effects: none (pure).
// Go: internal/ls/signaturehelp.go:getExpressionFromInvocation + getChildListThatStartsWithOpenerToken
fn call_parts(arena: &NodeArena, call: NodeId) -> Option<(NodeId, tsgo_ast::NodeList)> {
    match arena.data(call) {
        NodeData::CallExpression(d) => Some((d.expression, d.arguments.clone())),
        NodeData::NewExpression(d) => d.arguments.clone().map(|args| (d.expression, args)),
        _ => None,
    }
}

/// Resolves and renders the [`SignatureHelp`] for the call expression `call`.
///
/// Mirrors the reachable subset of Go's `createSignatureHelpItems`: type the
/// callee, read its call signatures, and render each as a [`SignatureInformation`].
/// Returns `None` when the callee has no resolvable symbol/type or no call
/// signatures (e.g. a `new C(...)` on a class — construct signatures are
/// deferred; see the module note).
///
/// Side effects: resolves symbols/types through the checker (may cache).
// Go: internal/ls/signaturehelp.go:LanguageService.createSignatureHelpItems
fn build_signature_help(
    ctx: &mut FileCheckContext,
    call: NodeId,
    position: i32,
) -> Option<SignatureHelp> {
    let (callee, args) = call_parts(ctx.view.arena(), call)?;
    let active_parameter = active_parameter_index(ctx.view.arena(), &args.nodes, position);
    let globals = ctx.view.globals().cloned();
    let callee_symbol = get_symbol_at_location(
        &mut ctx.checker,
        ctx.view.as_ref(),
        callee,
        globals.as_ref(),
    )?;
    let callee_type = get_type_of_symbol(
        &mut ctx.checker,
        ctx.view.as_ref(),
        callee_symbol,
        globals.as_ref(),
    );
    let signature_ids = ctx.checker.get_signatures_of_type(callee_type);
    if signature_ids.is_empty() {
        return None;
    }
    let callee_name = symbol_to_string(ctx.view.as_ref(), callee_symbol);
    let signatures = signature_ids
        .iter()
        .map(|&sid| render_signature(ctx, &callee_name, sid, globals.as_ref()))
        .collect();
    Some(SignatureHelp {
        signatures,
        // Overload selection is deferred; the first signature is active.
        active_signature: Some(0),
        active_parameter: Some(active_parameter),
    })
}

/// Returns the active parameter index for `position` within an argument list:
/// the number of arguments fully completed before the cursor.
///
/// Mirrors the reachable subset of Go's `getArgumentIndex` / comma counting: the
/// cursor is "in" the first argument whose end is at or after the position; an
/// argument that ends before the position is a completed argument that advances
/// the index (a trailing comma's empty slot leaves the cursor past every
/// argument, so the index is the argument count).
///
/// The spread-element / skip-comma refinements of `getArgumentIndexOrCount` and
/// the trailing-comma / whitespace edge cases are deferred (see the module
/// note).
///
/// Side effects: none (pure).
// Go: internal/ls/signaturehelp.go:getArgumentIndexOrCount
fn active_parameter_index(arena: &NodeArena, args: &[NodeId], position: i32) -> u32 {
    let mut active: u32 = 0;
    for &arg in args {
        if position <= arena.loc(arg).end() {
            return active;
        }
        active += 1;
    }
    active
}

/// Renders one call signature as a [`SignatureInformation`]: the call-target
/// name, the `(name: type, …)` parameter list, and the `: returnType` suffix.
///
/// Mirrors the reachable subset of Go's `getSignatureHelpItem` /
/// `itemInfoForParameters` / `returnTypeToDisplayParts`: each parameter is
/// rendered as `name: type` (the deferred `SymbolToParameterDeclaration`
/// modifier / optional `?` / rest `...` / binding-pattern rendering is noted in
/// the module DEFER list), and the return type via [`type_to_string`].
///
/// Side effects: resolves parameter / return types through the checker (may cache).
// Go: internal/ls/signaturehelp.go:LanguageService.getSignatureHelpItem / itemInfoForParameters
fn render_signature(
    ctx: &mut FileCheckContext,
    callee_name: &str,
    signature: SignatureId,
    globals: Option<&SymbolTable>,
) -> SignatureInformation {
    let params = ctx.checker.signature(signature).parameters.clone();
    let return_type = ctx.checker.signature(signature).resolved_return_type;

    let mut parameters = Vec::with_capacity(params.len());
    let mut param_labels = Vec::with_capacity(params.len());
    for param in params {
        let name = ctx.view.symbol(param).name.clone();
        let ty = get_type_of_symbol(&mut ctx.checker, ctx.view.as_ref(), param, globals);
        let type_str = type_to_string(&mut ctx.checker, ctx.view.as_ref(), ty);
        let label = format!("{name}: {type_str}");
        param_labels.push(label.clone());
        parameters.push(ParameterInformation { label });
    }

    let return_str = match return_type {
        Some(t) => type_to_string(&mut ctx.checker, ctx.view.as_ref(), t),
        None => "any".to_string(),
    };
    let label = format!("{callee_name}({}): {return_str}", param_labels.join(", "));
    SignatureInformation { label, parameters }
}

/// Reports whether `node` is an `astnav`-synthesized token id (a token the AST
/// does not store, e.g. a `(` or `,`), which has no parent in the parsed arena.
///
/// Mirrors `completions.rs`'s `is_synthesized_token`; duplicated because the tag
/// is `astnav`-internal, not part of its public API.
///
/// Side effects: none (pure).
fn is_synthesized_token(node: NodeId) -> bool {
    node.0 & SYNTHESIZED_NODE_TAG != 0
}

/// The high-bit tag `astnav` sets on synthesized token ids (mirrors
/// `internal/astnav/lib.rs`'s private `SYNTHESIZED_NODE_TAG`).
const SYNTHESIZED_NODE_TAG: u32 = 1 << 31;

#[cfg(test)]
#[path = "signaturehelp_test.rs"]
mod tests;
