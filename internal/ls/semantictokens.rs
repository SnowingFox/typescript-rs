//! Port of Go `internal/ls/semantictokens.go`: the `textDocument/semanticTokens`
//! provider.
//!
//! Go's `ProvideSemanticTokens` walks the source file, classifies every
//! identifier that has a symbol into a semantic token *type* (class / enum /
//! interface / namespace / type-parameter / type / parameter / variable /
//! property / function / method / ...) plus *modifiers* (declaration / static /
//! async / readonly / local / defaultLibrary), then encodes the tokens into the
//! LSP packed-integer delta format (`deltaLine`, `deltaStartChar`, `length`,
//! `tokenType`, `tokenModifiers`), filtered to the legend the client supports.
//!
//! # Reachable subset
//!
//! [`LanguageService::provide_semantic_tokens`] /
//! [`LanguageService::provide_semantic_tokens_range`] port the classification
//! walk + modifier computation + delta encoding faithfully:
//! `classify_symbol` / `token_from_declaration_mapping` (the symbol-flags +
//! declaration-kind → token-type mapping), the declaration / static / async /
//! readonly / local modifiers, the parameter → property reclassification in a
//! property-access context, the JSX / import-clause / `Infinity`/`NaN` guards,
//! and the exact Go index legend (`tokenType` 0..=22, `tokenModifier` bits) with
//! the relative delta encoding. [`semantic_tokens_legend`] ports the
//! client-capability legend filter.
//!
//! Which identifiers resolve is bounded by the reachable
//! [`get_symbol_at_location`](tsgo_checker::get_symbol_at_location): declaration
//! names (class / interface / variable / property / ...) and value-space
//! identifier uses (functions, variables, enums, parameters). Type-space
//! references (type aliases, type parameters in type position) do not resolve
//! yet, so their tokens are DEFERRED with the checker.
//!
//! DEFER(phase-7-ls):
//! - `reclassifyByType` (variables/properties/parameters whose *type* has call /
//!   construct signatures become function / method / class) — blocked-by the
//!   checker's `GetTypeAtLocation` + object-type property/union surface.
//! - the `defaultLibrary` modifier — blocked-by the compiler
//!   `IsSourceFileDefaultLibrary` program API (and lib.d.ts loading in the test
//!   harness).
//! - alias resolution (`GetAliasedSymbol` for `import`-bound names) — blocked-by
//!   the checker's alias surface; import-clause identifiers are already skipped.
//! - type-space references (type aliases / type parameters in type position) —
//!   blocked-by type-meaning name resolution in `get_symbol_at_location`.
//! - the per-client-capability legend filtering inside the encoder: the
//!   reachable encoder assumes the client supports the full legend (every token
//!   type + modifier), which is exactly what VS Code and the Go server tests
//!   request, so the encoded indices match Go's natural indices — blocked-by the
//!   LSP server's `GetClientCapabilities` context plumbing (P8).

use tsgo_ast::{
    Kind, ModifierFlags, ModifierList, NodeArena, NodeData, NodeFlags, NodeId, Symbol, SymbolFlags,
};
use tsgo_astnav::{get_start_of_node, NavSourceFile};
use tsgo_checker::get_symbol_at_location;
use tsgo_core::text::TextPos;
use tsgo_ls_lsconv::Converters;
use tsgo_lsproto::{
    Position, Range, ResolvedSemanticTokensClientCapabilities, SemanticTokens, SemanticTokensLegend,
};

use crate::languageservice::{DocumentScript, FileCheckContext, LanguageService};

/// The semantic token *types*, in the exact order Go's `tokenTypes` /
/// `tokenType` enum defines them (the encoded index of a token type is its
/// position in this list).
// Go: internal/ls/semantictokens.go:tokenTypes
const TOKEN_TYPE_NAMES: [&str; 23] = [
    "namespace",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "type",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "decorator",
    "event",
    "function",
    "method",
    "macro",
    "label",
    "comment",
    "string",
    "keyword",
    "number",
    "regexp",
    "operator",
];

/// The semantic token *modifiers*, in the exact order Go's `tokenModifiers` /
/// `tokenModifier` bit enum defines them (the bit index of a modifier is its
/// position in this list).
// Go: internal/ls/semantictokens.go:tokenModifiers
const TOKEN_MODIFIER_NAMES: [&str; 11] = [
    "declaration",
    "definition",
    "readonly",
    "static",
    "deprecated",
    "abstract",
    "async",
    "modification",
    "documentation",
    "defaultLibrary",
    "local",
];

/// A semantic token *type*, with the same encoded index as Go's `tokenType`
/// enum (`Namespace == 0`, `Class == 1`, ... `Operator == 22`).
///
/// The discriminant is the index into [`TOKEN_TYPE_NAMES`] and the value emitted
/// in the encoded `tokenType` field (under the full-legend assumption).
// Go: internal/ls/semantictokens.go:tokenType
//
// The full legend is kept (even the variants the reachable classification never
// produces — `Struct`, `Decorator`, ... `Operator`) so each discriminant equals
// Go's encoded index exactly; the unused variants are part of that index space.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum TokenType {
    Namespace = 0,
    Class = 1,
    Enum = 2,
    Interface = 3,
    Struct = 4,
    TypeParameter = 5,
    Type = 6,
    Parameter = 7,
    Variable = 8,
    Property = 9,
    EnumMember = 10,
    Decorator = 11,
    Event = 12,
    Function = 13,
    Method = 14,
    Macro = 15,
    Label = 16,
    Comment = 17,
    String = 18,
    Keyword = 19,
    Number = 20,
    Regexp = 21,
    Operator = 22,
}

// The semantic token *modifier* bits, matching Go's `tokenModifier = 1 << iota`.
// Only the reachable modifiers are defined (the `defaultLibrary` bit is DEFERRED
// with `IsSourceFileDefaultLibrary`).
// Go: internal/ls/semantictokens.go:tokenModifier
const MOD_DECLARATION: u32 = 1 << 0;
const MOD_READONLY: u32 = 1 << 2;
const MOD_STATIC: u32 = 1 << 3;
const MOD_ASYNC: u32 = 1 << 6;
const MOD_LOCAL: u32 = 1 << 10;

// A semantic "meaning" (value / type / namespace), matching Go's
// `ast.SemanticMeaning` bit set. Used only to gate the interface classification.
// Go: internal/ast/utilities.go:SemanticMeaning
const MEANING_VALUE: u32 = 1 << 0;
const MEANING_TYPE: u32 = 1 << 1;
const MEANING_NAMESPACE: u32 = 1 << 2;

/// A classified token before encoding: the identifier node plus its resolved
/// token type and modifier bit-set.
// Go: internal/ls/semantictokens.go:semanticToken
struct SemanticTokenItem {
    node: NodeId,
    token_type: TokenType,
    modifier: u32,
}

/// Returns the semantic-tokens legend filtered to the token types and modifiers
/// the client supports (Go's exported `SemanticTokensLegend`).
///
/// The legend preserves Go's canonical order, dropping any type/modifier the
/// client did not advertise in `client_capabilities`.
///
/// # Examples
/// ```
/// use tsgo_ls::semantictokens::semantic_tokens_legend;
/// use tsgo_lsproto::ResolvedSemanticTokensClientCapabilities;
/// let caps = ResolvedSemanticTokensClientCapabilities {
///     token_types: vec!["class".to_string(), "variable".to_string()],
///     token_modifiers: vec!["declaration".to_string()],
///     ..Default::default()
/// };
/// let legend = semantic_tokens_legend(&caps);
/// assert_eq!(legend.token_types, vec!["class".to_string(), "variable".to_string()]);
/// assert_eq!(legend.token_modifiers, vec!["declaration".to_string()]);
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/semantictokens.go:SemanticTokensLegend
pub fn semantic_tokens_legend(
    client_capabilities: &ResolvedSemanticTokensClientCapabilities,
) -> SemanticTokensLegend {
    let token_types = TOKEN_TYPE_NAMES
        .iter()
        .filter(|t| {
            client_capabilities
                .token_types
                .iter()
                .any(|s| s.as_str() == **t)
        })
        .map(|t| t.to_string())
        .collect();
    let token_modifiers = TOKEN_MODIFIER_NAMES
        .iter()
        .filter(|m| {
            client_capabilities
                .token_modifiers
                .iter()
                .any(|s| s.as_str() == **m)
        })
        .map(|m| m.to_string())
        .collect();
    SemanticTokensLegend {
        token_types,
        token_modifiers,
    }
}

impl LanguageService {
    /// Returns the full-document semantic tokens for `file_name` in the LSP
    /// packed-integer delta format, or `None` (LSP `null`) when there is no such
    /// file or no classifiable identifier.
    ///
    /// Mirrors Go's `ProvideSemanticTokens`: build a checker for the file, walk
    /// the whole file collecting classified identifier tokens, and delta-encode
    /// them (UTF-16 positions via the project
    /// [`Converters`](tsgo_ls_lsconv::Converters)).
    ///
    /// Side effects: binds every program file and allocates a checker
    /// (idempotent; via [`LanguageService::file_check_context`]).
    // Go: internal/ls/semantictokens.go:LanguageService.ProvideSemanticTokens
    pub fn provide_semantic_tokens(&mut self, file_name: &str) -> Option<SemanticTokens> {
        let script = self.document_script(file_name)?;
        let mut ctx = self.file_check_context(file_name)?;
        let (span_start, span_end) = {
            let loc = ctx.view.arena().loc(ctx.root);
            (loc.pos(), loc.end())
        };
        let ranged = self.collected_ranges(&mut ctx, span_start, span_end);
        self.finish_semantic_tokens(ranged, &script)
    }

    /// Returns the semantic tokens for the LSP `range` of `file_name`, the
    /// `textDocument/semanticTokens/range` request.
    ///
    /// Mirrors Go's `ProvideSemanticTokensRange`: convert the range to byte
    /// offsets, collect only the identifier tokens whose span overlaps it, and
    /// delta-encode.
    ///
    /// Side effects: as [`Self::provide_semantic_tokens`].
    // Go: internal/ls/semantictokens.go:LanguageService.ProvideSemanticTokensRange
    pub fn provide_semantic_tokens_range(
        &mut self,
        file_name: &str,
        range: Range,
    ) -> Option<SemanticTokens> {
        let script = self.document_script(file_name)?;
        let span_start = self
            .converters()
            .line_and_character_to_position(&script, range.start)
            .0;
        let span_end = self
            .converters()
            .line_and_character_to_position(&script, range.end)
            .0;
        let mut ctx = self.file_check_context(file_name)?;
        let ranged = self.collected_ranges(&mut ctx, span_start, span_end);
        self.finish_semantic_tokens(ranged, &script)
    }

    /// Collects the classified tokens in `[span_start, span_end)` and resolves
    /// each to its trivia-skipped `(start, end)` byte range (the input the
    /// encoder needs), while the checking context's arena borrow is held.
    ///
    /// Side effects: resolves symbols/types through the checker (may cache).
    fn collected_ranges(
        &self,
        ctx: &mut FileCheckContext,
        span_start: i32,
        span_end: i32,
    ) -> Vec<RangedToken> {
        let tokens = collect_semantic_tokens_in_range(ctx, span_start, span_end);
        let nav = NavSourceFile::from_borrowed_arena(ctx.view.arena(), ctx.root, ctx.text.clone());
        tokens
            .into_iter()
            .map(|t| RangedToken {
                start: get_start_of_node(&nav, t.node, false),
                end: nav.end(t.node),
                token_type: t.token_type,
                modifier: t.modifier,
            })
            .collect()
    }

    /// Encodes the collected ranges into the LSP delta format, or `None` when
    /// there are no tokens (Go returns `null`).
    ///
    /// Side effects: none (pure encoding).
    fn finish_semantic_tokens(
        &self,
        ranged: Vec<RangedToken>,
        script: &DocumentScript,
    ) -> Option<SemanticTokens> {
        if ranged.is_empty() {
            return None;
        }
        let data = encode_semantic_tokens(&ranged, self.converters(), script);
        Some(SemanticTokens {
            result_id: None,
            data,
        })
    }
}

/// A classified token resolved to its trivia-skipped byte range, ready to encode.
struct RangedToken {
    start: i32,
    end: i32,
    token_type: TokenType,
    modifier: u32,
}

/// Walks `[span_start, span_end)` of the file in `ctx`, collecting one
/// [`SemanticTokenItem`] per classifiable identifier (Go's
/// `collectSemanticTokensInRange`).
///
/// Side effects: resolves symbols through the checker (may cache).
// Go: internal/ls/semantictokens.go:LanguageService.collectSemanticTokensInRange
fn collect_semantic_tokens_in_range(
    ctx: &mut FileCheckContext,
    span_start: i32,
    span_end: i32,
) -> Vec<SemanticTokenItem> {
    let mut out = Vec::new();
    let root = ctx.root;
    visit_for_tokens(ctx, root, span_start, span_end, false, &mut out);
    out
}

/// Recursive visitor mirroring Go's `visit` closure: prune by span / reparsed
/// flag, track the enclosing-JSX state, classify identifiers, then recurse into
/// children (in source order, so collected tokens are position-sorted).
///
/// Side effects: resolves symbols through the checker (may cache).
// Go: internal/ls/semantictokens.go:LanguageService.collectSemanticTokensInRange (visit)
fn visit_for_tokens(
    ctx: &mut FileCheckContext,
    node: NodeId,
    span_start: i32,
    span_end: i32,
    in_jsx_element: bool,
    out: &mut Vec<SemanticTokenItem>,
) {
    let (kind, pos, end, reparsed) = {
        let arena = ctx.view.arena();
        let loc = arena.loc(node);
        (
            arena.kind(node),
            loc.pos(),
            loc.end(),
            arena.flags(node).contains(NodeFlags::REPARSED),
        )
    };
    if reparsed {
        return;
    }
    if pos >= span_end || end <= span_start {
        return;
    }

    // Update the enclosing-JSX state for this node (and its descendants). An
    // identifier node never matches these kinds, so this is equivalent to Go's
    // pre-classification update.
    let current_in_jsx = if matches!(kind, Kind::JsxElement | Kind::JsxSelfClosingElement) {
        true
    } else if kind == Kind::JsxExpression {
        false
    } else {
        in_jsx_element
    };

    if kind == Kind::Identifier {
        let text_ok = {
            let text = ctx.view.arena().text(node);
            !text.is_empty() && !is_infinity_or_nan_string(text)
        };
        let allowed = text_ok && !current_in_jsx && !is_in_import_clause(ctx.view.arena(), node);
        if allowed {
            if let Some(item) = classify_identifier(ctx, node) {
                out.push(item);
            }
        }
    }

    let mut children = Vec::new();
    ctx.view.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    for child in children {
        visit_for_tokens(ctx, child, span_start, span_end, current_in_jsx, out);
    }
}

/// Resolves the symbol of identifier `node`, classifies it into a token type +
/// modifier bit-set, or returns `None` when it has no symbol / no token type
/// (Go's per-identifier body of `collectSemanticTokensInRange`).
///
/// Side effects: resolves symbols through the checker (may cache).
// Go: internal/ls/semantictokens.go:LanguageService.collectSemanticTokensInRange (identifier body)
fn classify_identifier(ctx: &mut FileCheckContext, node: NodeId) -> Option<SemanticTokenItem> {
    let globals = ctx.view.globals().cloned();
    let meaning = get_meaning_from_location(ctx.view.arena(), node);
    // DEFER(phase-7-ls): alias resolution (`GetAliasedSymbol`) for import-bound
    // symbols; import-clause identifiers are already skipped by the caller.
    let symbol_id =
        get_symbol_at_location(&mut ctx.checker, ctx.view.as_ref(), node, globals.as_ref())?;

    let arena = ctx.view.arena();
    let symbol = ctx.view.symbol(symbol_id);
    let mut token_type = classify_symbol(arena, symbol, meaning)?;
    let mut modifier = 0u32;

    // declaration modifier: this identifier is the name of a declaration whose
    // declaration kind maps to this very token type (or a binding element).
    if let Some(parent) = arena.parent(node) {
        let parent_kind = arena.kind(parent);
        let parent_is_declaration = parent_kind == Kind::BindingElement
            || token_from_declaration_mapping(parent_kind) == Some(token_type);
        if parent_is_declaration && node_name(arena, parent) == Some(node) {
            modifier |= MOD_DECLARATION;
        }
    }

    // A parameter used on the right side of a property access is a property.
    if token_type == TokenType::Parameter
        && is_right_side_of_qualified_name_or_property_access(arena, node)
    {
        token_type = TokenType::Property;
    }

    // DEFER(phase-7-ls): `reclassifyByType` (call/construct-signature based
    // promotion to function/method/class) — needs `GetTypeAtLocation`.

    if let Some(decl) = symbol.value_declaration {
        let modifiers = combined_modifier_flags(arena, decl);
        let node_flags = combined_node_flags(arena, decl);

        if modifiers.intersects(ModifierFlags::STATIC) {
            modifier |= MOD_STATIC;
        }
        if modifiers.intersects(ModifierFlags::ASYNC) {
            modifier |= MOD_ASYNC;
        }
        if token_type != TokenType::Class && token_type != TokenType::Interface {
            let readonly = modifiers.intersects(ModifierFlags::READONLY)
                || node_flags.contains(NodeFlags::CONST)
                || symbol.flags.intersects(SymbolFlags::ENUM_MEMBER);
            if readonly {
                modifier |= MOD_READONLY;
            }
        }
        if (token_type == TokenType::Variable || token_type == TokenType::Function)
            && is_local_declaration(arena, decl, ctx.root)
        {
            modifier |= MOD_LOCAL;
        }
        // DEFER(phase-7-ls): the `defaultLibrary` modifier (the
        // `IsSourceFileDefaultLibrary` checks on the value declaration and on the
        // remaining declarations) — needs the compiler program API.
    }

    Some(SemanticTokenItem {
        node,
        token_type,
        modifier,
    })
}

/// Classifies a symbol into a token type from its flags + declaration kind,
/// using `meaning` to gate the interface case (Go's `classifySymbol`).
///
/// Side effects: none (pure).
// Go: internal/ls/semantictokens.go:classifySymbol
fn classify_symbol(arena: &NodeArena, symbol: &Symbol, meaning: u32) -> Option<TokenType> {
    let flags = symbol.flags;
    if flags.intersects(SymbolFlags::CLASS) {
        return Some(TokenType::Class);
    }
    if flags.intersects(SymbolFlags::ENUM) {
        return Some(TokenType::Enum);
    }
    if flags.intersects(SymbolFlags::TYPE_ALIAS) {
        return Some(TokenType::Type);
    }
    if flags.intersects(SymbolFlags::INTERFACE) && (meaning & MEANING_TYPE) != 0 {
        return Some(TokenType::Interface);
    }
    if flags.intersects(SymbolFlags::TYPE_PARAMETER) {
        return Some(TokenType::TypeParameter);
    }

    let mut decl = symbol
        .value_declaration
        .or_else(|| symbol.declarations.first().copied())?;
    if arena.kind(decl) == Kind::BindingElement {
        decl = get_declaration_for_binding_element(arena, decl);
    }
    token_from_declaration_mapping(arena.kind(decl))
}

/// Maps a declaration kind to its token type, or `None` for kinds that do not
/// classify (Go's `tokenFromDeclarationMapping`, which returns `-1`).
///
/// Side effects: none (pure).
// Go: internal/ls/semantictokens.go:tokenFromDeclarationMapping
fn token_from_declaration_mapping(kind: Kind) -> Option<TokenType> {
    match kind {
        Kind::VariableDeclaration => Some(TokenType::Variable),
        Kind::Parameter => Some(TokenType::Parameter),
        Kind::PropertyDeclaration => Some(TokenType::Property),
        Kind::ModuleDeclaration => Some(TokenType::Namespace),
        Kind::EnumDeclaration => Some(TokenType::Enum),
        Kind::EnumMember => Some(TokenType::EnumMember),
        Kind::ClassDeclaration | Kind::ClassExpression => Some(TokenType::Class),
        Kind::MethodDeclaration => Some(TokenType::Method),
        Kind::FunctionDeclaration | Kind::FunctionExpression => Some(TokenType::Function),
        Kind::MethodSignature => Some(TokenType::Method),
        Kind::GetAccessor | Kind::SetAccessor => Some(TokenType::Property),
        Kind::PropertySignature => Some(TokenType::Property),
        Kind::InterfaceDeclaration => Some(TokenType::Interface),
        Kind::TypeAliasDeclaration => Some(TokenType::Type),
        Kind::TypeParameter => Some(TokenType::TypeParameter),
        Kind::PropertyAssignment | Kind::ShorthandPropertyAssignment => Some(TokenType::Property),
        _ => None,
    }
}

/// Reports whether the declaration `decl` (of a variable / function symbol) is
/// local — declared inside a function/block rather than at the top of its
/// source file (Go's `isLocalDeclaration`).
///
/// Side effects: none (pure).
// Go: internal/ls/semantictokens.go:isLocalDeclaration
fn is_local_declaration(arena: &NodeArena, decl: NodeId, source_file: NodeId) -> bool {
    let decl = if arena.kind(decl) == Kind::BindingElement {
        get_declaration_for_binding_element(arena, decl)
    } else {
        decl
    };
    match arena.kind(decl) {
        Kind::VariableDeclaration => {
            let Some(parent) = arena.parent(decl) else {
                return false;
            };
            if arena.kind(parent) == Kind::CatchClause {
                return get_source_file_of_node(arena, decl) == Some(source_file);
            }
            if arena.kind(parent) == Kind::VariableDeclarationList {
                if let Some(grandparent) = arena.parent(parent) {
                    if let Some(great_grandparent) = arena.parent(grandparent) {
                        return (arena.kind(great_grandparent) != Kind::SourceFile
                            || arena.kind(grandparent) == Kind::CatchClause)
                            && get_source_file_of_node(arena, decl) == Some(source_file);
                    }
                }
            }
            false
        }
        Kind::FunctionDeclaration => match arena.parent(decl) {
            Some(parent) => {
                arena.kind(parent) != Kind::SourceFile
                    && get_source_file_of_node(arena, decl) == Some(source_file)
            }
            None => false,
        },
        _ => false,
    }
}

/// Walks a binding element up to the variable / parameter declaration that owns
/// its binding pattern (Go's `getDeclarationForBindingElement`).
///
/// Side effects: none (pure).
// Go: internal/ls/semantictokens.go:getDeclarationForBindingElement
fn get_declaration_for_binding_element(arena: &NodeArena, mut element: NodeId) -> NodeId {
    loop {
        let Some(parent) = arena.parent(element) else {
            return element;
        };
        if is_binding_pattern(arena.kind(parent)) {
            if let Some(grandparent) = arena.parent(parent) {
                if arena.kind(grandparent) == Kind::BindingElement {
                    element = grandparent;
                    continue;
                }
            }
            return arena.parent(parent).unwrap_or(element);
        }
        return element;
    }
}

/// Reports whether identifier `node` is the local name of an `import` clause /
/// specifier / namespace import (Go's `isInImportClause`).
///
/// Side effects: none (pure).
// Go: internal/ls/semantictokens.go:isInImportClause
fn is_in_import_clause(arena: &NodeArena, node: NodeId) -> bool {
    match arena.parent(node) {
        Some(parent) => matches!(
            arena.kind(parent),
            Kind::ImportClause | Kind::ImportSpecifier | Kind::NamespaceImport
        ),
        None => false,
    }
}

/// Reports whether `node` is the right side of a qualified name / property
/// access / meta-property (Go's `IsRightSideOfQualifiedNameOrPropertyAccess`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsRightSideOfQualifiedNameOrPropertyAccess
fn is_right_side_of_qualified_name_or_property_access(arena: &NodeArena, node: NodeId) -> bool {
    let Some(parent) = arena.parent(node) else {
        return false;
    };
    match arena.data(parent) {
        NodeData::QualifiedName(d) => d.right == node,
        NodeData::PropertyAccessExpression(d) => d.name == node,
        NodeData::MetaProperty(d) => d.name == node,
        _ => false,
    }
}

/// Reports whether `text` is the `Infinity` / `NaN` global identifier name,
/// which is never classified (Go's `isInfinityOrNaNString`).
///
/// Side effects: none (pure).
// Go: internal/ls/semantictokens.go:isInfinityOrNaNString
fn is_infinity_or_nan_string(text: &str) -> bool {
    text == "Infinity" || text == "NaN"
}

/// Reports whether `kind` is an object / array binding pattern.
// Go: internal/ast/utilities.go:IsBindingPattern
fn is_binding_pattern(kind: Kind) -> bool {
    matches!(kind, Kind::ObjectBindingPattern | Kind::ArrayBindingPattern)
}

/// Returns the `SourceFile` node that contains `node`, by walking parents (Go's
/// `GetSourceFileOfNode`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetSourceFileOfNode
fn get_source_file_of_node(arena: &NodeArena, mut node: NodeId) -> Option<NodeId> {
    loop {
        if arena.kind(node) == Kind::SourceFile {
            return Some(node);
        }
        node = arena.parent(node)?;
    }
}

/// The semantic meaning (value / type / namespace) of the location `node`, the
/// reachable subset of Go's `getMeaningFromLocation`.
///
/// Covers the cases that affect reachable classification: the source file
/// (value), a declaration name (the meaning of its declaration), and everything
/// else (value). The import-equals / type-reference / namespace-reference /
/// JSDoc / literal-type cases are DEFERRED with the type-space resolution that
/// would surface them.
///
/// Side effects: none (pure).
// Go: internal/ls/utilities.go:getMeaningFromLocation (reachable subset)
fn get_meaning_from_location(arena: &NodeArena, node: NodeId) -> u32 {
    if arena.kind(node) == Kind::SourceFile {
        return MEANING_VALUE;
    }
    if is_declaration_name(arena, node) {
        if let Some(parent) = arena.parent(node) {
            return get_meaning_from_declaration(arena, parent);
        }
    }
    MEANING_VALUE
}

/// The semantic meaning of a declaration node (Go's `getMeaningFromDeclaration`).
///
/// Side effects: none (pure).
// Go: internal/ls/utilities.go:getMeaningFromDeclaration
fn get_meaning_from_declaration(arena: &NodeArena, node: NodeId) -> u32 {
    match arena.kind(node) {
        Kind::VariableDeclaration
        | Kind::Parameter
        | Kind::BindingElement
        | Kind::PropertyDeclaration
        | Kind::PropertySignature
        | Kind::PropertyAssignment
        | Kind::ShorthandPropertyAssignment
        | Kind::MethodDeclaration
        | Kind::MethodSignature
        | Kind::Constructor
        | Kind::GetAccessor
        | Kind::SetAccessor
        | Kind::FunctionDeclaration
        | Kind::FunctionExpression
        | Kind::ArrowFunction
        | Kind::CatchClause
        | Kind::JsxAttribute => MEANING_VALUE,

        Kind::TypeParameter
        | Kind::InterfaceDeclaration
        | Kind::TypeAliasDeclaration
        | Kind::TypeLiteral => MEANING_TYPE,

        Kind::EnumMember | Kind::ClassDeclaration => MEANING_VALUE | MEANING_TYPE,

        // DEFER(phase-7-ls): the precise ambient / instantiated distinction
        // (`GetModuleInstanceState`). A module is always at least a namespace +
        // value here, which only the (DEFERRED) namespace-reference path reads.
        Kind::ModuleDeclaration => MEANING_NAMESPACE | MEANING_VALUE,

        Kind::EnumDeclaration
        | Kind::NamedImports
        | Kind::ImportSpecifier
        | Kind::ImportEqualsDeclaration
        | Kind::ImportDeclaration
        | Kind::ExportAssignment
        | Kind::ExportDeclaration => MEANING_VALUE | MEANING_TYPE | MEANING_NAMESPACE,

        Kind::SourceFile => MEANING_NAMESPACE | MEANING_VALUE,

        _ => MEANING_VALUE | MEANING_TYPE | MEANING_NAMESPACE,
    }
}

/// Reports whether identifier `node` is the name of its parent declaration (the
/// reachable subset of Go's `ast.IsDeclarationName`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsDeclarationName (reachable subset)
fn is_declaration_name(arena: &NodeArena, node: NodeId) -> bool {
    if arena.kind(node) != Kind::Identifier {
        return false;
    }
    match arena.parent(node) {
        Some(parent) => node_name(arena, parent) == Some(node),
        None => false,
    }
}

/// Returns the `name` child of a declaration node (Go's `Node.Name`), for the
/// declaration kinds reachable here.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.Name
fn node_name(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::VariableDeclaration(d) => Some(d.name),
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.name,
        NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d)
        | NodeData::InterfaceDeclaration(d) => d.name,
        NodeData::TypeAliasDeclaration(d) => Some(d.name),
        NodeData::TypeParameterDeclaration(d) => Some(d.name),
        NodeData::EnumDeclaration(d) => Some(d.name),
        NodeData::EnumMember(d) => Some(d.name),
        NodeData::ModuleDeclaration(d) => Some(d.name),
        NodeData::PropertyDeclaration(d) | NodeData::PropertySignature(d) => Some(d.name),
        NodeData::PropertyAssignment(d) => Some(d.name),
        NodeData::ShorthandPropertyAssignment(d) => Some(d.name),
        NodeData::MethodDeclaration(d) => Some(d.name),
        NodeData::MethodSignature(d) => Some(d.name),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => Some(d.name),
        NodeData::ParameterDeclaration(d) => Some(d.name),
        NodeData::BindingElement(d) => d.name,
        _ => None,
    }
}

/// Returns the modifier list a node carries, if any (Go's `Node.Modifiers`).
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.Modifiers
fn modifiers_of(arena: &NodeArena, node: NodeId) -> Option<&ModifierList> {
    match arena.data(node) {
        NodeData::VariableStatement(d) => d.modifiers.as_ref(),
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.modifiers.as_ref(),
        NodeData::ParameterDeclaration(d) => d.modifiers.as_ref(),
        NodeData::TypeParameterDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d)
        | NodeData::InterfaceDeclaration(d) => d.modifiers.as_ref(),
        NodeData::MethodDeclaration(d) => d.modifiers.as_ref(),
        NodeData::PropertyDeclaration(d) | NodeData::PropertySignature(d) => d.modifiers.as_ref(),
        NodeData::PropertyAssignment(d) => d.modifiers.as_ref(),
        NodeData::MethodSignature(d) => d.modifiers.as_ref(),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            d.modifiers.as_ref()
        }
        NodeData::ConstructorDeclaration(d) => d.modifiers.as_ref(),
        NodeData::IndexSignatureDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassStaticBlockDeclaration(d) => d.modifiers.as_ref(),
        NodeData::TypeAliasDeclaration(d) => d.modifiers.as_ref(),
        NodeData::EnumDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ModuleDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ImportDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ImportEqualsDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ExportDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ExportAssignment(d) => d.modifiers.as_ref(),
        NodeData::NamespaceExportDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ArrowFunction(d) => d.modifiers.as_ref(),
        NodeData::ShorthandPropertyAssignment(d) => d.modifiers.as_ref(),
        NodeData::FunctionType(d) | NodeData::ConstructorType(d) => d.modifiers.as_ref(),
        _ => None,
    }
}

/// Returns the syntactic modifier flags directly on `node` (Go's
/// `Node.ModifierFlags`).
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.ModifierFlags
fn node_modifier_flags(arena: &NodeArena, node: NodeId) -> ModifierFlags {
    modifiers_of(arena, node).map_or(ModifierFlags::NONE, |m| m.modifier_flags)
}

/// Walks a binding element to the declaration it belongs to (Go's
/// `GetRootDeclaration`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetRootDeclaration
fn get_root_declaration(arena: &NodeArena, mut node: NodeId) -> NodeId {
    while arena.kind(node) == Kind::BindingElement {
        match arena.parent(node).and_then(|p| arena.parent(p)) {
            Some(grand) => node = grand,
            None => break,
        }
    }
    node
}

/// Returns the combined modifier flags of a declaration, folding in modifiers of
/// an enclosing variable declaration list / statement (Go's
/// `GetCombinedModifierFlags`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetCombinedModifierFlags
fn combined_modifier_flags(arena: &NodeArena, node: NodeId) -> ModifierFlags {
    let mut node = get_root_declaration(arena, node);
    let mut flags = node_modifier_flags(arena, node);
    if arena.kind(node) == Kind::VariableDeclaration {
        if let Some(p) = arena.parent(node) {
            node = p;
        }
    }
    if arena.kind(node) == Kind::VariableDeclarationList {
        flags |= node_modifier_flags(arena, node);
        if let Some(p) = arena.parent(node) {
            node = p;
        }
    }
    if arena.kind(node) == Kind::VariableStatement {
        flags |= node_modifier_flags(arena, node);
    }
    flags
}

/// Returns the combined node flags of a declaration, folding in the flags of an
/// enclosing variable declaration list / statement (Go's
/// `GetCombinedNodeFlags`); this is how `const` reaches a variable declaration.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:GetCombinedNodeFlags
fn combined_node_flags(arena: &NodeArena, node: NodeId) -> NodeFlags {
    let mut node = get_root_declaration(arena, node);
    let mut flags = arena.flags(node);
    if arena.kind(node) == Kind::VariableDeclaration {
        if let Some(p) = arena.parent(node) {
            node = p;
        }
    }
    if arena.kind(node) == Kind::VariableDeclarationList {
        flags |= arena.flags(node);
        if let Some(p) = arena.parent(node) {
            node = p;
        }
    }
    if arena.kind(node) == Kind::VariableStatement {
        flags |= arena.flags(node);
    }
    flags
}

/// Encodes the classified tokens into the LSP packed-integer delta format
/// (`deltaLine`, `deltaStartChar`, `length`, `tokenType`, `tokenModifiers`),
/// assuming the client supports the full legend (Go's `encodeSemanticTokens`
/// with every type/modifier supported, the natural-index case).
///
/// # Panics
/// Panics if a token spans multiple lines or the tokens are not strictly
/// position-increasing, mirroring Go's invariants (identifiers never span lines
/// and the visitor walks in source order, so neither fires in practice).
///
/// Side effects: none (pure).
// Go: internal/ls/semantictokens.go:encodeSemanticTokens
fn encode_semantic_tokens(
    tokens: &[RangedToken],
    converters: &Converters,
    script: &DocumentScript,
) -> Vec<u32> {
    let mut encoded = Vec::with_capacity(tokens.len() * 5);
    let mut prev_line = 0u32;
    let mut prev_char = 0u32;

    for token in tokens {
        let start_pos: Position =
            converters.position_to_line_and_character(script, TextPos(token.start));
        let end_pos: Position =
            converters.position_to_line_and_character(script, TextPos(token.end));

        let token_length = if start_pos.line == end_pos.line {
            end_pos.character - start_pos.character
        } else {
            panic!(
                "semantic tokens: token spans multiple lines: start=({},{}) end=({},{})",
                start_pos.line, start_pos.character, end_pos.line, end_pos.character
            );
        };

        let line = start_pos.line;
        let char = start_pos.character;

        if !encoded.is_empty() && (line < prev_line || (line == prev_line && char <= prev_char)) {
            panic!(
                "semantic tokens: positions must be strictly increasing: prev=({prev_line},{prev_char}) current=({line},{char})"
            );
        }

        let delta_line = line - prev_line;
        let delta_char = if delta_line == 0 {
            char - prev_char
        } else {
            char
        };

        encoded.push(delta_line);
        encoded.push(delta_char);
        encoded.push(token_length);
        encoded.push(token.token_type as u32);
        encoded.push(token.modifier);

        prev_line = line;
        prev_char = char;
    }

    encoded
}

#[cfg(test)]
#[path = "semantictokens_test.rs"]
mod tests;
