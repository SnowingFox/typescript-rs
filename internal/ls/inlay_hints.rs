//! Port of Go `internal/ls/inlay_hints.go`: the `textDocument/inlayHint`
//! provider that renders inline hints (parameter names, inferred variable /
//! parameter / return types, and enum member values) inside a requested range.
//!
//! # Reachable subset
//!
//! This round lands the **enum-member-value** hint kind end-to-end plus the
//! shared scaffolding every kind shares: the request gate
//! ([`is_any_inlay_hint_enabled`]), the range-pruned source walk (Go's
//! `inlayHintState.visit`, including the reparsed-node / zero-width /
//! span-intersection / type-node skips), and the byte → UTF-16 position
//! conversion. An enum member with no initializer renders `= <value>` after its
//! name via the checker's constant folder ([`EmitResolver::get_constant_value`]).
//!
//! [`LanguageService::provide_inlay_hints`] returns `Some(hints)` (Go's
//! non-null array, possibly empty) when any hint kind is enabled, or `None`
//! (Go's `null`) when none is.
//!
//! DEFER(phase-7-ls): the remaining hint kinds, each blocked on a checker
//! surface not yet ported:
//! - **parameter-name hints** (`visitCallOrNewExpression` +
//!   `addParameterHints`, with the `isHintableLiteral` / name-match suppression
//!   rules, `getParameterIdentifierInfoAtPosition`, and the leading-`...` rest
//!   handling). blocked-by: a public `getResolvedSignature` (call / overload
//!   resolution) — only a private contextual-argument resolver exists today, so
//!   an arbitrary call site cannot be mapped to its signature's parameters.
//! - **variable-type / property-declaration-type hints**
//!   (`visitVariableLikeDeclaration`, with the annotation /
//!   `…WhenTypeMatchesName` suppression). blocked-by: `getTypeAtLocation` +
//!   initializer-inferred types — the checker yields `any` for an un-annotated
//!   `const x = 1`, so the rendered type would diverge from Go's `number`.
//! - **function parameter-type / return-type hints**
//!   (`visitFunctionLikeForParameterType` /
//!   `visitFunctionDeclarationLikeForReturnType`). blocked-by: a public
//!   `getSignatureFromDeclaration` / `getReturnTypeOfSignature` /
//!   `getTypePredicateOfSignature`, plus the type-node → label-parts renderer
//!   (`getInlayHintLabelParts`, which walks `TypeToTypeNode` with the
//!   identifier→symbol side map).
//! - the `context.Context` cancellation checks in `visit` (the LS has no
//!   cancellation token plumbing yet, matching the sibling providers).

use tsgo_ast::{Kind, NodeData, NodeFlags, NodeId};
use tsgo_checker::EmitResolver;
use tsgo_core::text::{TextPos, TextRange};
use tsgo_evaluator::{any_to_string, EvalValue};
use tsgo_ls_lsutil::{IncludeInlayParameterNameHints, InlayHintsPreferences};
use tsgo_lsproto::{InlayHint, InlayHintKind, Range, StringOrInlayHintLabelParts};

use crate::languageservice::{FileCheckContext, LanguageService};

impl LanguageService {
    /// Returns the inlay hints inside `range` of `file_name`, or `None` (Go's
    /// `null`) when no hint kind is enabled in `preferences` / there is no such
    /// file.
    ///
    /// Mirrors Go's `ProvideInlayHint`: bail out unless some hint kind is
    /// enabled, build a checker for the file, walk the requested range, and
    /// convert each hint's byte position to a UTF-16 [`Position`](tsgo_lsproto::Position).
    ///
    /// Side effects: binds every program file and allocates a checker
    /// (idempotent; via [`LanguageService::file_check_context`]).
    // Go: internal/ls/inlay_hints.go:LanguageService.ProvideInlayHint
    pub fn provide_inlay_hints(
        &mut self,
        file_name: &str,
        range: Range,
        preferences: &InlayHintsPreferences,
    ) -> Option<Vec<InlayHint>> {
        if !is_any_inlay_hint_enabled(preferences) {
            return None;
        }
        // Convert the LSP range to a byte span first (immutable borrows), so the
        // checking context can take `&mut self` afterwards.
        let script = self.document_script(file_name)?;
        let span_start = self
            .converters()
            .line_and_character_to_position(&script, range.start)
            .0;
        let span_end = self
            .converters()
            .line_and_character_to_position(&script, range.end)
            .0;
        let span = TextRange::new(span_start, span_end);

        let ctx = self.file_check_context(file_name)?;
        let raw = collect_inlay_hints(&ctx, span, preferences);
        let hints = raw
            .into_iter()
            .map(|r| InlayHint {
                position: self
                    .converters()
                    .position_to_line_and_character(&script, TextPos(r.position)),
                label: r.label,
                kind: r.kind,
                text_edits: None,
                tooltip: None,
                padding_left: r.padding_left,
                padding_right: r.padding_right,
                data: None,
            })
            .collect();
        Some(hints)
    }
}

/// An inlay hint collected during the walk, before its byte position is
/// converted to a UTF-16 [`Position`](tsgo_lsproto::Position).
///
/// Carries everything [`InlayHint`] needs except the converted position (the
/// converters live on the [`LanguageService`], outside the checking-context
/// borrow held by the walk).
struct RawInlayHint {
    position: i32,
    label: StringOrInlayHintLabelParts,
    kind: Option<InlayHintKind>,
    padding_left: Option<bool>,
    padding_right: Option<bool>,
}

/// Walks the file in `ctx`, collecting one [`RawInlayHint`] per reachable hint
/// inside `span` (Go's `inlayHintState.visit` driven from the source file).
///
/// Side effects: reads the checker's constant folder (no mutation).
// Go: internal/ls/inlay_hints.go:LanguageService.ProvideInlayHint (visit driver)
fn collect_inlay_hints(
    ctx: &FileCheckContext,
    span: TextRange,
    preferences: &InlayHintsPreferences,
) -> Vec<RawInlayHint> {
    let mut out = Vec::new();
    let resolver = ctx.checker.get_emit_resolver();
    visit(ctx, &resolver, ctx.root, span, preferences, &mut out);
    out
}

/// Recursive visitor mirroring Go's `inlayHintState.visit`: prune by
/// reparsed-flag / zero width / span intersection / type-node kind, dispatch
/// the (reachable) hint kinds, then recurse into children in source order.
///
/// Side effects: reads the checker's constant folder (no mutation).
// Go: internal/ls/inlay_hints.go:inlayHintState.visit
fn visit(
    ctx: &FileCheckContext,
    resolver: &EmitResolver,
    node: NodeId,
    span: TextRange,
    preferences: &InlayHintsPreferences,
    out: &mut Vec<RawInlayHint>,
) {
    let arena = ctx.view.arena();
    let loc = arena.loc(node);
    let kind = arena.kind(node);

    // Zero-width or reparsed nodes are not visited (Go's first guard).
    if loc.end() - loc.pos() == 0 || arena.flags(node).contains(NodeFlags::REPARSED) {
        return;
    }

    // DEFER(phase-7-ls): the `ctx.Err()` cancellation checks Go runs at module /
    // class / interface / function / arrow boundaries — the LS has no
    // cancellation-token plumbing yet (matching the sibling providers).

    // Prune subtrees that do not overlap the requested span.
    if !span.intersects(loc) {
        return;
    }

    // Do not descend into type nodes (except `ExpressionWithTypeArguments`,
    // whose type arguments are visited).
    if is_type_node_kind(kind) && kind != Kind::ExpressionWithTypeArguments {
        return;
    }

    // Reachable dispatch: enum-member-value hints. The variable/property type,
    // parameter-name, and function parameter-type / return-type branches are
    // DEFERRED (see the module docs); they are skipped here, not stubbed, so the
    // walk produces no diverging hints in the interim.
    if preferences.include_inlay_enum_member_value_hints.is_true() && kind == Kind::EnumMember {
        visit_enum_member(ctx, resolver, node, out);
    }

    // Recurse into children in source order (so collected hints stay
    // position-sorted), mirroring Go's `node.ForEachChild(s.visit)`.
    let mut children = Vec::new();
    arena.for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    for child in children {
        visit(ctx, resolver, child, span, preferences, out);
    }
}

/// Emits a `= <value>` hint for an enum member with no initializer, anchored at
/// the member's end (Go's `visitEnumMember`).
///
/// A member with an explicit initializer renders nothing (the value is already
/// in the source); a member whose value does not fold to a constant likewise
/// renders nothing.
///
/// Side effects: reads the checker's constant folder (no mutation).
// Go: internal/ls/inlay_hints.go:inlayHintState.visitEnumMember
fn visit_enum_member(
    ctx: &FileCheckContext,
    resolver: &EmitResolver,
    member: NodeId,
    out: &mut Vec<RawInlayHint>,
) {
    let arena = ctx.view.arena();
    let initializer = match arena.data(member) {
        NodeData::EnumMember(d) => d.initializer,
        _ => None,
    };
    if initializer.is_some() {
        return;
    }

    let enum_value = resolver.get_constant_value(ctx.view.as_ref(), member);
    if enum_value != EvalValue::None {
        let position = arena.loc(member).end();
        add_enum_member_value_hints(out, &any_to_string(&enum_value), position);
    }
}

/// Appends an enum-member-value hint (`= <text>`, left padding, no kind) at
/// `position` (Go's `addEnumMemberValueHints`).
///
/// Side effects: pushes onto `out`.
// Go: internal/ls/inlay_hints.go:inlayHintState.addEnumMemberValueHints
fn add_enum_member_value_hints(out: &mut Vec<RawInlayHint>, text: &str, position: i32) {
    out.push(RawInlayHint {
        position,
        label: StringOrInlayHintLabelParts {
            string: Some(format!("= {text}")),
            inlay_hint_label_parts: None,
        },
        kind: None,
        padding_left: Some(true),
        padding_right: None,
    });
}

/// Reports whether `preferences` enables any inlay-hint kind (Go's
/// `isAnyInlayHintEnabled`); when false, the provider returns Go's `null`.
///
/// Side effects: none (pure).
// Go: internal/ls/inlay_hints.go:isAnyInlayHintEnabled
fn is_any_inlay_hint_enabled(preferences: &InlayHintsPreferences) -> bool {
    preferences.include_inlay_parameter_name_hints != IncludeInlayParameterNameHints::None
        || preferences
            .include_inlay_function_parameter_type_hints
            .is_true()
        || preferences.include_inlay_variable_type_hints.is_true()
        || preferences
            .include_inlay_property_declaration_type_hints
            .is_true()
        || preferences
            .include_inlay_function_like_return_type_hints
            .is_true()
        || preferences.include_inlay_enum_member_value_hints.is_true()
}

/// Reports whether `kind` is a type-node kind (Go's `IsTypeNodeKind`): the walk
/// does not descend into type annotations (except `ExpressionWithTypeArguments`,
/// handled by the caller).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsTypeNodeKind
fn is_type_node_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::AnyKeyword
            | Kind::UnknownKeyword
            | Kind::NumberKeyword
            | Kind::BigIntKeyword
            | Kind::ObjectKeyword
            | Kind::BooleanKeyword
            | Kind::StringKeyword
            | Kind::SymbolKeyword
            | Kind::VoidKeyword
            | Kind::UndefinedKeyword
            | Kind::NeverKeyword
            | Kind::IntrinsicKeyword
            | Kind::ExpressionWithTypeArguments
            | Kind::JSDocAllType
            | Kind::JSDocNullableType
            | Kind::JSDocNonNullableType
            | Kind::JSDocOptionalType
            | Kind::JSDocVariadicType
    ) || (kind >= Kind::FIRST_TYPE_NODE && kind <= Kind::LAST_TYPE_NODE)
}

#[cfg(test)]
#[path = "inlay_hints_test.rs"]
mod tests;
