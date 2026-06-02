//! Port of Go `internal/ls/inlay_hints.go`: the `textDocument/inlayHint`
//! provider that renders inline hints (parameter names, inferred variable /
//! parameter / return types, and enum member values) inside a requested range.
//!
//! # Reachable subset
//!
//! This round lands the **parameter-name**, **variable-type**,
//! **property-declaration-type**, and **enum-member-value** hint kinds
//! end-to-end, plus the shared scaffolding every kind shares: the request gate
//! ([`is_any_inlay_hint_enabled`]), the range-pruned source walk (Go's
//! `inlayHintState.visit`, including the reparsed-node / zero-width /
//! span-intersection / type-node skips), and the byte → UTF-16 position
//! conversion.
//!
//! - An enum member with no initializer renders `= <value>` after its name via
//!   the checker's constant folder ([`EmitResolver::get_constant_value`]).
//! - An un-annotated `let x = 1` / `const x = f()` / un-annotated class field
//!   renders `: <type>` after its name, using the checker's type-at-location
//!   query ([`get_type_at_location`]) — the keystone that makes the LS resolve
//!   the *inferred* type of a node (`number`, the call's return type, ...) — and
//!   the type string renderer ([`type_to_string`]). The Go `isHintableDeclaration`
//!   / annotation / `…WhenTypeMatchesName` / module-reference suppressions all
//!   apply, so a `const x = 1` (a hintable literal) and an annotated declaration
//!   render nothing.
//! - Each argument of a call / `new` expression renders `paramName:` before it,
//!   using the checker's call-resolution query
//!   ([`get_resolved_signature`]) — the keystone that maps a call site to the
//!   resolved [`Signature`](tsgo_checker::Signature)'s parameters — mapped to
//!   the parameter via [`get_parameter_identifier_info_at_position`]. Go's
//!   literals-only filter ([`is_hintable_literal`]), the
//!   argument-name-matches-parameter suppression (with `…WhenArgumentMatchesName`),
//!   the leading-comment suppression, and the leading-`...` rest handling all
//!   apply (a tuple rest's per-element labels are deferred, see below).
//!
//! [`LanguageService::provide_inlay_hints`] returns `Some(hints)` (Go's
//! non-null array, possibly empty) when any hint kind is enabled, or `None`
//! (Go's `null`) when none is.
//!
//! ## Divergence: type STRING label, not structured parts
//!
//! Go's `visitVariableLikeDeclaration` renders the type through
//! `typeToInlayHintParts` (`TypeToTypeNode` + `getInlayHintLabelParts`), which
//! produces a structured [`StringOrInlayHintLabelParts`] whose identifier parts
//! carry `Location` links to the type's declarations. This round renders the
//! plain type STRING ([`type_to_string`]) into the [`StringOrInlayHintLabelParts`]
//! `String` arm (which Go's `addTypeHints` also supports). The hint TEXT — and
//! the `…WhenTypeMatchesName` comparison text, which Go derives by concatenating
//! the same parts — is identical; only the clickable per-identifier `Location`
//! links are deferred, mirroring the hover provider's type-string-only rendering.
//! DEFER(phase-7-ls): the `getInlayHintLabelParts` structured renderer.
//! blocked-by: the type-node → label-parts walk over `TypeToTypeNode` with the
//! identifier→symbol side map.
//!
//! ## Divergence: parameter-name label parts without the `Location` link
//!
//! Go's `addParameterHints` builds the structured parts `[{name}, {":"}]` and
//! attaches a clickable [`Location`](tsgo_lsproto::Location) link to the name
//! part (`getNodeDisplayPart`, linking to the parameter's declaration). This
//! round produces the same two-part structure and the same text, but defers the
//! `Location` link (the [`Converters`](tsgo_ls_lsconv::Converters) are not
//! available inside the checking-context walk; positions are converted
//! afterward), mirroring the type-hint link deferral above.
//! DEFER(phase-7-ls): the parameter-name `Location` link.
//! blocked-by: threading the parameter node + source range through the walk.
//!
//! DEFER(phase-7-ls): the remaining hint kinds / paths, each blocked on a
//! checker surface not yet ported:
//! - **function parameter-type / return-type hints**
//!   (`visitFunctionLikeForParameterType` /
//!   `visitFunctionDeclarationLikeForReturnType`). blocked-by: a public
//!   `getSignatureFromDeclaration` / `getReturnTypeOfSignature` /
//!   `getTypePredicateOfSignature`, plus the type-node → label-parts renderer
//!   (`getInlayHintLabelParts`, which walks `TypeToTypeNode` with the
//!   identifier→symbol side map).
//! - the parameter-name **fixed-tuple `...spread` expansion** and the
//!   **rest-tuple labeled-element** path (a `...args: [a: number, b: string]`
//!   reads per-element labels). blocked-by: the tuple `ElementFlags` /
//!   `FixedLength` / labeled-declaration surface.
//! - construct signatures for `new` (the resolution path returns only call
//!   signatures), and the overloaded-call disambiguation. blocked-by: construct
//!   signatures + overload resolution.
//! - the `context.Context` cancellation checks in `visit` (the LS has no
//!   cancellation token plumbing yet, matching the sibling providers).

use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, SymbolId};
use tsgo_astnav::{get_start_of_node, NavSourceFile};
use tsgo_checker::{
    get_resolved_signature, get_type_at_location, get_type_of_symbol, type_to_string, EmitResolver,
    ObjectFlags, SignatureFlags, SignatureId, TypeFlags,
};
use tsgo_core::text::{TextPos, TextRange};
use tsgo_evaluator::{any_to_string, EvalValue};
use tsgo_ls_lsutil::{IncludeInlayParameterNameHints, InlayHintsPreferences};
use tsgo_lsproto::{
    InlayHint, InlayHintKind, InlayHintLabelPart, Range, StringOrInlayHintLabelParts,
};
use tsgo_scanner::{get_leading_comment_ranges, is_identifier_text};

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

        let mut ctx = self.file_check_context(file_name)?;
        let raw = collect_inlay_hints(&mut ctx, span, preferences);
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
/// Side effects: may build inferred types via the type-at-location query and
/// reads the checker's constant folder.
// Go: internal/ls/inlay_hints.go:LanguageService.ProvideInlayHint (visit driver)
fn collect_inlay_hints(
    ctx: &mut FileCheckContext,
    span: TextRange,
    preferences: &InlayHintsPreferences,
) -> Vec<RawInlayHint> {
    let mut out = Vec::new();
    // The emit resolver is a lightweight owned value handle (not a borrow of the
    // checker), so the walk can still take `&mut ctx.checker` for type queries.
    let resolver = ctx.checker.get_emit_resolver();
    visit(ctx, &resolver, ctx.root, span, preferences, &mut out);
    out
}

/// Recursive visitor mirroring Go's `inlayHintState.visit`: prune by
/// reparsed-flag / zero width / span intersection / type-node kind, dispatch
/// the (reachable) hint kinds, then recurse into children in source order.
///
/// Side effects: may build inferred types via the type-at-location query and
/// reads the checker's constant folder.
// Go: internal/ls/inlay_hints.go:inlayHintState.visit
fn visit(
    ctx: &mut FileCheckContext,
    resolver: &EmitResolver,
    node: NodeId,
    span: TextRange,
    preferences: &InlayHintsPreferences,
    out: &mut Vec<RawInlayHint>,
) {
    // Read the structural bits up front, before any `&mut checker` query.
    let (kind, loc, flags) = {
        let arena = ctx.view.arena();
        (arena.kind(node), arena.loc(node), arena.flags(node))
    };

    // Zero-width or reparsed nodes are not visited (Go's first guard).
    if loc.end() - loc.pos() == 0 || flags.contains(NodeFlags::REPARSED) {
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

    // Reachable dispatch, mirroring Go's if/else-if chain: variable-type,
    // property-declaration-type, enum-member-value, then parameter-name hints.
    // Go's first two branches (variable vs property declaration) have identical
    // bodies (`visitVariableLikeDeclaration`), so they are merged into one
    // condition here. The trailing function parameter-type / return-type branch
    // is DEFERRED (see the module docs); it is absent from the chain, not
    // stubbed, so the walk produces no diverging hints in the interim.
    let is_variable_type_hint = preferences.include_inlay_variable_type_hints.is_true()
        && kind == Kind::VariableDeclaration;
    let is_property_type_hint = preferences
        .include_inlay_property_declaration_type_hints
        .is_true()
        && kind == Kind::PropertyDeclaration;
    if is_variable_type_hint || is_property_type_hint {
        visit_variable_like_declaration(ctx, node, preferences, out);
    } else if preferences.include_inlay_enum_member_value_hints.is_true()
        && kind == Kind::EnumMember
    {
        visit_enum_member(ctx, resolver, node, out);
    } else if should_show_parameter_name_hints(preferences)
        && matches!(kind, Kind::CallExpression | Kind::NewExpression)
    {
        visit_call_or_new_expression(ctx, node, preferences, out);
    }

    // Recurse into children in source order (so collected hints stay
    // position-sorted), mirroring Go's `node.ForEachChild(s.visit)`.
    let mut children = Vec::new();
    ctx.view.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    for child in children {
        visit(ctx, resolver, child, span, preferences, out);
    }
}

/// Emits a `: <type>` hint for an un-annotated variable / class-property
/// declaration whose inferred type-at-location is renderable (Go's
/// `visitVariableLikeDeclaration`).
///
/// Suppressed (no hint) when, mirroring Go's gates:
/// - there is no initializer and the declaration is not a class property whose
///   type-at-location is non-`any` (so `let x;` / `class C { x; }` render
///   nothing, but `class C { x = 1 }` does);
/// - the name is a binding pattern (`const { a } = o`);
/// - it is a variable declaration that is not "hintable" (a `const` initialized
///   from a literal / `new` / object-literal / assertion — the type is already
///   obvious, so `const x = 1` renders nothing while `let x = 1` does);
/// - the declaration carries a type annotation;
/// - the inferred type is a module reference (`import * as ns`);
/// - the type text case-insensitively equals the declaration name and the
///   `…WhenTypeMatchesName` toggle is off.
///
/// Side effects: builds the declaration's inferred type via the checker
/// (caches) and renders its string.
// Go: internal/ls/inlay_hints.go:inlayHintState.visitVariableLikeDeclaration
fn visit_variable_like_declaration(
    ctx: &mut FileCheckContext,
    decl: NodeId,
    preferences: &InlayHintsPreferences,
    out: &mut Vec<RawInlayHint>,
) {
    // Read the declaration's structural bits up front (immutable arena borrow),
    // before any `&mut checker` type query.
    let (kind, initializer, name, type_node) = {
        let arena = ctx.view.arena();
        let kind = arena.kind(decl);
        let (initializer, name, type_node) = match arena.data(decl) {
            NodeData::VariableDeclaration(d) => (d.initializer, d.name, d.type_node),
            NodeData::PropertyDeclaration(d) => (d.initializer, d.name, d.type_node),
            _ => return,
        };
        (kind, initializer, name, type_node)
    };
    let is_property = kind == Kind::PropertyDeclaration;
    let is_variable = kind == Kind::VariableDeclaration;
    let name_kind = ctx.view.arena().kind(name);
    let name_is_binding_pattern = matches!(
        name_kind,
        Kind::ObjectBindingPattern | Kind::ArrayBindingPattern
    );

    // Go's compound early-return (operator precedence: `&&` binds tighter than
    // `||`):
    //   (decl.Initializer() == nil &&
    //      !(IsPropertyDeclaration && GetTypeAtLocation(decl).Flags()&Any == 0))
    //   || IsBindingPattern(decl.Name())
    //   || (IsVariableDeclaration && !isHintableDeclaration(decl))
    let no_initializer_suppresses = if initializer.is_none() {
        if is_property {
            // `!(type is non-any)` == `type IS any`: an un-initialized class
            // property whose type-at-location is `any` produces no hint, but one
            // that resolves to a concrete type (e.g. via a base class) does.
            let globals = ctx.view.globals().cloned();
            let t =
                get_type_at_location(&mut ctx.checker, ctx.view.as_ref(), decl, globals.as_ref());
            ctx.checker.get_type(t).flags().contains(TypeFlags::ANY)
        } else {
            true
        }
    } else {
        false
    };
    if no_initializer_suppresses
        || name_is_binding_pattern
        || (is_variable && !is_hintable_declaration(ctx.view.arena(), decl))
    {
        return;
    }

    // A declaration WITH a type annotation already shows its type in the source.
    if type_node.is_some() {
        return;
    }

    let globals = ctx.view.globals().cloned();
    let declaration_type =
        get_type_at_location(&mut ctx.checker, ctx.view.as_ref(), decl, globals.as_ref());
    if is_module_reference_type(ctx, declaration_type) {
        return;
    }
    let hint_text = type_to_string(&mut ctx.checker, ctx.view.as_ref(), declaration_type);

    // `…WhenTypeMatchesName` suppression: drop a hint whose (case-insensitive)
    // type text equals the declaration name, unless the toggle re-enables it or
    // the name is computed.
    if !preferences
        .include_inlay_variable_type_hints_when_type_matches_name
        .is_true()
        && name_kind != Kind::ComputedPropertyName
        && equate_string_case_insensitive(declaration_name_text(ctx.view.arena(), name), &hint_text)
    {
        return;
    }

    let position = ctx.view.arena().loc(name).end();
    add_type_hints(out, &hint_text, position);
}

/// Appends a `: <type>` type hint at `position` (Go's `addTypeHints`, the
/// `String`-label arm). Uses the [`InlayHintKind::TYPE`] kind with left padding.
///
/// Side effects: pushes onto `out`.
// Go: internal/ls/inlay_hints.go:inlayHintState.addTypeHints
fn add_type_hints(out: &mut Vec<RawInlayHint>, text: &str, position: i32) {
    out.push(RawInlayHint {
        position,
        label: StringOrInlayHintLabelParts {
            string: Some(format!(": {text}")),
            inlay_hint_label_parts: None,
        },
        kind: Some(InlayHintKind::TYPE),
        padding_left: Some(true),
        padding_right: None,
    });
}

/// Reports whether `t`'s symbol is a module (`import * as ns`), which Go's
/// `isModuleReferenceType` excludes from variable/return type hints.
///
/// Side effects: none (reads type + symbol tables).
// Go: internal/ls/inlay_hints.go:isModuleReferenceType
fn is_module_reference_type(ctx: &FileCheckContext, t: tsgo_checker::TypeId) -> bool {
    match ctx.checker.get_type(t).symbol {
        Some(symbol) => ctx
            .view
            .symbol(symbol)
            .flags
            .intersects(tsgo_ast::SymbolFlags::MODULE),
        None => false,
    }
}

/// Reports whether a variable / parameter declaration is "hintable" (Go's
/// `isHintableDeclaration`): a `const` (or parameter) initialized from a literal
/// / `new` / object-literal / assertion is NOT hintable (its type is obvious
/// from the initializer), so `const x = 1` renders no hint; everything else is.
///
/// The parameter case (`IsPartOfParameterDeclaration`) is reached only by the
/// DEFERRED parameter-type hints, so this reachable subset handles the variable
/// declaration arm.
///
/// Side effects: none (pure).
// Go: internal/ls/inlay_hints.go:isHintableDeclaration
fn is_hintable_declaration(arena: &NodeArena, node: NodeId) -> bool {
    let initializer = match arena.data(node) {
        NodeData::VariableDeclaration(d) => d.initializer,
        _ => return true,
    };
    if arena.kind(node) == Kind::VariableDeclaration && is_var_const(arena, node) {
        if let Some(init) = initializer {
            let init = skip_parentheses(arena, init);
            let init_kind = arena.kind(init);
            return !(is_hintable_literal(arena, init)
                || init_kind == Kind::NewExpression
                || init_kind == Kind::ObjectLiteralExpression
                || matches!(
                    init_kind,
                    Kind::AsExpression | Kind::TypeAssertionExpression
                ));
        }
    }
    true
}

/// Reports whether `node` is a "hintable literal" (Go's `isHintableLiteral`):
/// the literals whose inferred type is obvious enough that a `const` bound to
/// one needs no type hint (numeric/string/etc. literals, `true`/`false`/`null`,
/// templates, `undefined`/`Infinity`/`NaN`, and prefixed literals).
///
/// Side effects: none (pure).
// Go: internal/ls/inlay_hints.go:isHintableLiteral
fn is_hintable_literal(arena: &NodeArena, node: NodeId) -> bool {
    match arena.kind(node) {
        Kind::PrefixUnaryExpression => {
            let operand = match arena.data(node) {
                NodeData::PrefixUnaryExpression(d) => d.operand,
                _ => return false,
            };
            is_literal_expression(arena.kind(operand))
                || (arena.kind(operand) == Kind::Identifier
                    && is_infinity_or_nan_string(arena.text(operand)))
        }
        Kind::TrueKeyword
        | Kind::FalseKeyword
        | Kind::NullKeyword
        | Kind::NoSubstitutionTemplateLiteral
        | Kind::TemplateExpression => true,
        Kind::Identifier => {
            let name = arena.text(node);
            name == "undefined" || is_infinity_or_nan_string(name)
        }
        kind => is_literal_expression(kind),
    }
}

/// Reports whether `kind` is a literal token (Go's `ast.IsLiteralKind`):
/// `NumericLiteral..=NoSubstitutionTemplateLiteral`.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsLiteralKind
fn is_literal_expression(kind: Kind) -> bool {
    kind >= Kind::FIRST_LITERAL_TOKEN && kind <= Kind::LAST_LITERAL_TOKEN
}

/// Reports whether `name` is one of the special numeric-identifier names Go's
/// `ast.IsInfinityOrNaNString` recognizes.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsInfinityOrNaNString
fn is_infinity_or_nan_string(name: &str) -> bool {
    matches!(name, "Infinity" | "-Infinity" | "NaN")
}

/// Unwraps `(expr)` parentheses (Go's `ast.SkipParentheses`, the reachable
/// non-assertion subset).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:SkipParentheses
fn skip_parentheses(arena: &NodeArena, node: NodeId) -> NodeId {
    let mut node = node;
    while arena.kind(node) == Kind::ParenthesizedExpression {
        node = match arena.data(node) {
            NodeData::ParenthesizedExpression(d) => d.expression,
            _ => break,
        };
    }
    node
}

/// Reports whether a variable declaration's combined node flags mark it `const`
/// (Go's `ast.IsVarConst`: `combined & BlockScoped == Const`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsVarConst
fn is_var_const(arena: &NodeArena, node: NodeId) -> bool {
    (combined_node_flags(arena, node) & NodeFlags::BLOCK_SCOPED) == NodeFlags::CONST
}

/// Returns the combined node flags of a variable declaration, folding in the
/// enclosing declaration-list / statement flags (Go's `GetCombinedNodeFlags`);
/// this is how the `const`/`let` bit reaches a `VariableDeclaration`.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetCombinedNodeFlags
fn combined_node_flags(arena: &NodeArena, node: NodeId) -> NodeFlags {
    let mut current = node;
    let mut flags = arena.flags(current);
    if arena.kind(current) == Kind::VariableDeclaration {
        if let Some(p) = arena.parent(current) {
            current = p;
        }
    }
    if arena.kind(current) == Kind::VariableDeclarationList {
        flags |= arena.flags(current);
        if let Some(p) = arena.parent(current) {
            current = p;
        }
    }
    if arena.kind(current) == Kind::VariableStatement {
        flags |= arena.flags(current);
    }
    flags
}

/// The declaration name's text for the `…WhenTypeMatchesName` comparison (Go's
/// `decl.Name().Text()`); empty for a name kind that carries no text (so it
/// never spuriously matches).
///
/// Side effects: none (pure).
fn declaration_name_text(arena: &NodeArena, name: NodeId) -> &str {
    match arena.kind(name) {
        Kind::Identifier
        | Kind::PrivateIdentifier
        | Kind::StringLiteral
        | Kind::NumericLiteral
        | Kind::BigIntLiteral
        | Kind::NoSubstitutionTemplateLiteral => arena.text(name),
        _ => "",
    }
}

/// Case-insensitive string equality (Go's `stringutil.EquateStringCaseInsensitive`,
/// per-rune simple-fold). Inlined here so the LS crate keeps its dependency set
/// (`tsgo_stringutil` is not a dependency).
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:EquateStringCaseInsensitive
fn equate_string_case_insensitive(a: &str, b: &str) -> bool {
    let mut ai = a.chars();
    let mut bi = b.chars();
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return true,
            (Some(ca), Some(cb)) => {
                if ca != cb
                    && ca.to_lowercase().next().unwrap_or(ca)
                        != cb.to_lowercase().next().unwrap_or(cb)
                {
                    return false;
                }
            }
            _ => return false,
        }
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

/// Per-argument parameter info (Go's `parameterInfo`): the parameter's name and
/// whether it is the leading variadic (rest) parameter.
///
/// Go also carries the parameter's identifier *node* (for the clickable
/// [`Location`](tsgo_lsproto::Location) link `getNodeDisplayPart` attaches to
/// the name part). That link is DEFERRED here (see [`add_parameter_hints`]), so
/// only the text-bearing fields are kept.
struct ParameterInfo {
    /// The parameter name rendered before the argument.
    name: String,
    /// Whether this is the first variadic (rest) argument (prefixes `...`).
    is_rest_parameter: bool,
}

/// Emits one `name:` parameter-name hint per argument of a call / `new`
/// expression (Go's `visitCallOrNewExpression`): resolve the call's signature,
/// then for each argument map its position to the signature's parameter and add
/// the hint before the argument.
///
/// Mirrors Go's per-argument rules:
/// - in literals-only mode, a non-[`is_hintable_literal`] argument is skipped
///   (but still advances the parameter position);
/// - the argument → parameter mapping is [`get_parameter_identifier_info_at_position`];
///   a `None` mapping stops the walk (Go's `return`), matching its "stop at the
///   first unmappable position" behavior;
/// - the hint is suppressed when the argument's identifier / property-access
///   postfix matches the parameter name (unless `…WhenArgumentMatchesName` is on),
///   except for the leading variadic argument;
/// - the hint is suppressed when a leading comment already spells the parameter
///   name (`f(/*a*/ x)`).
///
/// DEFER(phase-7-ls): the spread-element tuple expansion — Go computes how many
/// fixed positions a `...tuple` argument fills from the tuple's element flags /
/// fixed length. A non-tuple spread advances one position (Go's fall-through,
/// which this matches); a fixed-tuple spread (rare) is deferred. blocked-by: the
/// tuple `ElementFlags` / `FixedLength` surface.
///
/// Side effects: resolves the call's signature through the checker (may allocate
/// types; diagnostics rolled back) and reads parameter symbols.
// Go: internal/ls/inlay_hints.go:inlayHintState.visitCallOrNewExpression
fn visit_call_or_new_expression(
    ctx: &mut FileCheckContext,
    expr: NodeId,
    preferences: &InlayHintsPreferences,
    out: &mut Vec<RawInlayHint>,
) {
    // Read the argument list up front (immutable arena borrow). A `new`
    // expression with no parentheses has no argument list.
    let args = {
        let arena = ctx.view.arena();
        match arena.data(expr) {
            NodeData::CallExpression(d) => d.arguments.nodes.clone(),
            NodeData::NewExpression(d) => d
                .arguments
                .as_ref()
                .map(|l| l.nodes.clone())
                .unwrap_or_default(),
            _ => return,
        }
    };
    if args.is_empty() {
        return;
    }

    let Some(signature) = get_resolved_signature(&mut ctx.checker, ctx.view.as_ref(), expr) else {
        return;
    };

    let literals_only = should_show_literal_parameter_name_hints_only(preferences);
    let when_argument_matches_name = preferences
        .include_inlay_parameter_name_hints_when_argument_matches_name
        .is_true();

    let mut signature_param_pos = 0usize;
    for &original_arg in &args {
        let arg = skip_parentheses(ctx.view.arena(), original_arg);
        if literals_only && !is_hintable_literal(ctx.view.arena(), arg) {
            signature_param_pos += 1;
            continue;
        }

        // DEFER(phase-7-ls): a fixed-tuple `...spread` argument fills multiple
        // parameter positions in Go; here a spread advances one position (Go's
        // non-tuple fall-through). blocked-by: tuple ElementFlags / FixedLength.
        let identifier_info =
            get_parameter_identifier_info_at_position(ctx, signature, signature_param_pos);
        signature_param_pos += 1;
        let Some(info) = identifier_info else {
            return;
        };

        // Suppress a hint whose parameter name matches the argument's own
        // identifier / property-access name (unless the toggle re-enables it),
        // except for the leading variadic argument (Go keeps `...name` there).
        let parameter_name_not_same_as_argument = when_argument_matches_name
            || !identifier_or_access_expression_postfix_matches_parameter_name(
                ctx.view.arena(),
                arg,
                &info.name,
            );
        if !parameter_name_not_same_as_argument && !info.is_rest_parameter {
            continue;
        }

        if leading_comments_contains_parameter_name(ctx, arg, &info.name) {
            continue;
        }

        // Anchor the hint at the (original, un-parenthesized) argument's start,
        // skipping leading trivia (Go's `astnav.GetStartOfNode(originalArg)`).
        let position = {
            let nav =
                NavSourceFile::from_borrowed_arena(ctx.view.arena(), ctx.root, ctx.text.clone());
            get_start_of_node(&nav, original_arg, false)
        };
        add_parameter_hints(out, &info.name, position, info.is_rest_parameter);
    }
}

/// Maps signature parameter position `pos` to its parameter name / rest-ness
/// (Go's `getParameterIdentifierInfoAtPosition`).
///
/// A position within the fixed parameters resolves to that parameter's
/// identifier name. A position at the rest parameter resolves to the rest
/// parameter (`isRestParameter = true`, so the hint prefixes `...`). A
/// parameter whose value declaration is not an `identifier` parameter, and any
/// position past a non-tuple rest parameter, yield `None`.
///
/// DEFER(phase-7-ls): the rest-tuple labeled-element path — when the rest type
/// is a fixed tuple, Go reads per-element labeled names (`...args: [a: number,
/// b: string]`). A tuple rest yields `None` here (no hint) rather than a wrong
/// one. blocked-by: tuple element infos / labeled declarations.
///
/// Side effects: resolves the rest parameter's type through the checker (only on
/// the rest path; may allocate types).
// Go: internal/ls/inlay_hints.go:inlayHintState.getParameterIdentifierInfoAtPosition
fn get_parameter_identifier_info_at_position(
    ctx: &mut FileCheckContext,
    signature: SignatureId,
    pos: usize,
) -> Option<ParameterInfo> {
    let (parameters, has_rest) = {
        let sig = ctx.checker.signature(signature);
        (
            sig.parameters.clone(),
            sig.flags.contains(SignatureFlags::HAS_REST_PARAMETER),
        )
    };
    let param_count = parameters.len() - usize::from(has_rest);

    if pos < param_count {
        let param_id = get_parameter_declaration_identifier(ctx.view.as_ref(), parameters[pos])?;
        let name = ctx.view.arena().text(param_id).to_string();
        return Some(ParameterInfo {
            name,
            is_rest_parameter: false,
        });
    }

    // Past the fixed parameters: only a rest parameter can supply more names.
    if param_count >= parameters.len() {
        return None;
    }
    let rest_parameter = parameters[param_count];
    // The rest parameter must be a plain identifier parameter to be hintable.
    get_parameter_declaration_identifier(ctx.view.as_ref(), rest_parameter)?;

    // A fixed-tuple rest (`...args: [a, b]`) carries per-element labels Go reads
    // here; that path is DEFERRED, so a tuple rest yields no hint.
    let globals = ctx.view.globals().cloned();
    let rest_type = get_type_of_symbol(
        &mut ctx.checker,
        ctx.view.as_ref(),
        rest_parameter,
        globals.as_ref(),
    );
    if ctx
        .checker
        .get_type(rest_type)
        .object_flags()
        .contains(ObjectFlags::TUPLE)
    {
        return None;
    }

    // A non-tuple rest array (`...xs: number[]`) labels only its first position
    // (`...xs`); further positions yield `None` (Go's trailing `return nil`).
    if pos == param_count {
        let name = ctx.view.symbol(rest_parameter).name.clone();
        return Some(ParameterInfo {
            name,
            is_rest_parameter: true,
        });
    }
    None
}

/// Returns the identifier name node of a parameter symbol whose value
/// declaration is a plain identifier parameter (Go's
/// `getParameterDeclarationIdentifier`); `None` for a binding-pattern parameter
/// or a symbol with no parameter value declaration.
///
/// Side effects: none (pure).
// Go: internal/ls/inlay_hints.go:getParameterDeclarationIdentifier
fn get_parameter_declaration_identifier(
    program: &dyn tsgo_checker::BoundProgram,
    symbol: SymbolId,
) -> Option<NodeId> {
    let value_declaration = program.symbol(symbol).value_declaration?;
    let arena = program.arena();
    if arena.kind(value_declaration) != Kind::Parameter {
        return None;
    }
    let name = match arena.data(value_declaration) {
        NodeData::ParameterDeclaration(d) => d.name,
        _ => return None,
    };
    if arena.kind(name) != Kind::Identifier {
        return None;
    }
    Some(name)
}

/// Reports whether `expr` is an identifier (or property access) whose (trailing)
/// name text equals `parameter_name` (Go's
/// `identifierOrAccessExpressionPostfixMatchesParameterName`), used to suppress a
/// redundant hint like `f(a)` where the parameter is also named `a`.
///
/// Side effects: none (pure).
// Go: internal/ls/inlay_hints.go:identifierOrAccessExpressionPostfixMatchesParameterName
fn identifier_or_access_expression_postfix_matches_parameter_name(
    arena: &NodeArena,
    expr: NodeId,
    parameter_name: &str,
) -> bool {
    match arena.kind(expr) {
        Kind::Identifier => arena.text(expr) == parameter_name,
        Kind::PropertyAccessExpression => match arena.data(expr) {
            NodeData::PropertyAccessExpression(d) => arena.text(d.name) == parameter_name,
            _ => false,
        },
        _ => false,
    }
}

/// Reports whether a leading comment of `node` is exactly `name` (Go's
/// `leadingCommentsContainsParameterName`): `f(/*a*/ x)` already labels the
/// argument, so the hint is suppressed. Only fires for a syntactically valid
/// identifier name; `JsxText` carries no comments.
///
/// Side effects: scans the file text's leading comment ranges (no mutation).
// Go: internal/ls/inlay_hints.go:inlayHintState.leadingCommentsContainsParameterName
fn leading_comments_contains_parameter_name(
    ctx: &FileCheckContext,
    node: NodeId,
    name: &str,
) -> bool {
    let language_variant = match ctx.view.arena().data(ctx.root) {
        NodeData::SourceFile(d) => d.language_variant,
        _ => return false,
    };
    if !is_identifier_text(name, language_variant) {
        return false;
    }
    // JsxText cannot actually contain comments (Go's getLeadingCommentRangesOfNode).
    if ctx.view.arena().kind(node) == Kind::JsxText {
        return false;
    }
    let pos = ctx.view.arena().loc(node).pos();
    for range in get_leading_comment_ranges(&ctx.text, pos) {
        let comment_text = ctx.text[range.loc.pos() as usize..range.loc.end() as usize]
            .trim_matches(|c: char| c.is_whitespace() || c == '/' || c == '*');
        if comment_text == name {
            return true;
        }
    }
    false
}

/// Appends a parameter-name hint at `position` (Go's `addParameterHints`): the
/// label is the structured parts `[{name-or-...name}, {":"}]`, with the
/// [`InlayHintKind::PARAMETER`] kind and right padding.
///
/// DEFER(phase-7-ls): the clickable [`Location`](tsgo_lsproto::Location) link Go
/// attaches to the name part (`getNodeDisplayPart`). Mirrors the type-hint link
/// deferral (the [`Converters`](tsgo_ls_lsconv::Converters) are not available
/// inside the checking-context walk; positions are converted afterward).
/// blocked-by: threading the parameter node + source range through the walk.
///
/// Side effects: pushes onto `out`.
// Go: internal/ls/inlay_hints.go:inlayHintState.addParameterHints
fn add_parameter_hints(
    out: &mut Vec<RawInlayHint>,
    text: &str,
    position: i32,
    is_first_variadic_argument: bool,
) {
    let hint_text = if is_first_variadic_argument {
        format!("...{text}")
    } else {
        text.to_string()
    };
    let parts = vec![
        InlayHintLabelPart {
            value: hint_text,
            ..Default::default()
        },
        InlayHintLabelPart {
            value: ":".to_string(),
            ..Default::default()
        },
    ];
    out.push(RawInlayHint {
        position,
        label: StringOrInlayHintLabelParts {
            string: None,
            inlay_hint_label_parts: Some(parts),
        },
        kind: Some(InlayHintKind::PARAMETER),
        padding_left: None,
        padding_right: Some(true),
    });
}

/// Reports whether `preferences` enables parameter-name hints (Go's
/// `shouldShowParameterNameHints`): `literals` or `all`.
///
/// Side effects: none (pure).
// Go: internal/ls/inlay_hints.go:shouldShowParameterNameHints
fn should_show_parameter_name_hints(preferences: &InlayHintsPreferences) -> bool {
    matches!(
        preferences.include_inlay_parameter_name_hints,
        IncludeInlayParameterNameHints::Literals | IncludeInlayParameterNameHints::All
    )
}

/// Reports whether `preferences` restricts parameter-name hints to literal
/// arguments (Go's `shouldShowLiteralParameterNameHintsOnly`): the `literals`
/// mode.
///
/// Side effects: none (pure).
// Go: internal/ls/inlay_hints.go:shouldShowLiteralParameterNameHintsOnly
fn should_show_literal_parameter_name_hints_only(preferences: &InlayHintsPreferences) -> bool {
    preferences.include_inlay_parameter_name_hints == IncludeInlayParameterNameHints::Literals
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
