//! Port of Go `internal/ls/completions.go`: the completions feature.
//!
//! Go's `ProvideCompletion` resolves the relevant tokens at a position
//! (`getRelevantTokens`), decides whether the cursor is *after a dot* on an
//! expression (`isRightOfDot`) or at a bare identifier position, then in
//! `getCompletionData` either enumerates the **members** of the dotted
//! expression's apparent type (`getTypeScriptMemberSymbols` →
//! `GetApparentProperties`) or the **symbols in scope**
//! (`getGlobalCompletions` → `GetSymbolsInScope`). Each candidate symbol becomes
//! an `lsproto.CompletionItem` whose `Kind` is `GetSymbolKind` →
//! `getCompletionsSymbolKind`, and the list is sorted by `CompareCompletionEntries`.
//!
//! # Reachable subset
//!
//! This round ports the two reachable cores:
//!
//! - **Member completions** ([`member_completions`]): after a `.` on an
//!   expression `e.`, resolve `e`'s symbol and type
//!   ([`get_symbol_at_location`] + [`get_type_of_symbol`], as `hover` does),
//!   enumerate its apparent type's properties ([`get_properties_of_type`], which
//!   applies the apparent type and unions intersection members), and return one
//!   [`lsproto::CompletionItem`] per property (label = property name, kind from
//!   the symbol flags). Reachable: `const o = { a: 1, b: "x" }; o.` → `a`, `b`.
//! - **Scope completions** ([`scope_completions`]): at a bare identifier
//!   position, walk the binder `locals` tables up the container chain plus the
//!   program `globals` (the reachable subset of `GetSymbolsInScope`), keeping the
//!   first-seen symbol per name (inner scopes shadow outer), and return one item
//!   per visible symbol. Reachable: `const x = 1; function f(p) { /*here*/ }` →
//!   `x`, `f`, `p`.
//! - **Kind mapping**: [`script_element_kind_for_symbol`] (the reachable subset
//!   of `lsutil.GetSymbolKind`) + [`completion_item_kind`] (the port of
//!   `getCompletionsSymbolKind`) map a symbol's flags to an
//!   [`lsproto::CompletionItemKind`].
//!
//! DEFER(phase-7-ls): **auto-import completions** (Go's `collectAutoImports` /
//! `autoImports`) — blocked-by: the concurrently-edited `tsgo_ls_autoimport`
//! crate (this lane must not depend on it). Completion **details/resolve**
//! (`getCompletionEntryDetails` — documentation / signature / additional edits).
//! **JSX / string-literal / import-path / JSDoc** completions
//! (`getStringLiteralCompletions`, `tryGetJsxCompletionSymbols`,
//! `tryGetImportCompletionSymbols`, the JSDoc-tag cases). **Object-literal /
//! contextual-type** property suggestions (`tryGetObjectLikeCompletionSymbols`,
//! `getContextualType`). **Keyword** completions (`getKeywordCompletions`).
//! **Snippets / replacement spans / commit characters / sort-text / preselect /
//! filter-text / `CompletionItemData`** (`createLSPCompletionItem`'s edit + sort
//! machinery). The `this.` / `super.` / optional-chain (`?.`) member paths, the
//! module/enum-member export enumeration (`GetExportsOfModule`), and the
//! `IsValidPropertyAccessForCompletions` accessibility filter (needs the
//! property-accessibility check). blocked-by: `tsgo_ls_autoimport`,
//! `getCompletionEntryDetails`, `GetContextualType`, the keyword tables, and the
//! `lsproto.CompletionList` / `CompletionItemData` surface.

use std::collections::HashSet;

use tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_THIS;
use tsgo_ast::utilities::is_keyword_kind;
use tsgo_ast::{Kind, NodeId, SymbolFlags, SymbolId};
use tsgo_astnav::NavSourceFile;
use tsgo_checker::{
    get_properties_of_type, get_symbol_at_location, get_type_of_symbol, BoundProgram,
};
use tsgo_ls_lsutil::ScriptElementKind;
use tsgo_lsproto::{CompletionItem, CompletionItemKind, Position};

use crate::languageservice::{FileCheckContext, LanguageService};

/// The reachable subset of Go's `lsproto.CompletionList`: the completion items
/// for a position plus the incompleteness flag.
///
/// Go's `lsproto.CompletionList` also carries `ItemDefaults` (the shared
/// commit-characters / edit-range / data defaults). The `lsproto` crate does not
/// yet generate `CompletionList`, and this lane may not edit it, so the list
/// type lives here; `is_incomplete` mirrors `CompletionList.IsIncomplete` and
/// `items` mirrors `CompletionList.Items`.
///
/// Side effects: none (plain data).
// Go: internal/lsp/lsproto/lsp_generated.go:CompletionList (reachable subset)
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompletionList {
    /// Whether the list is incomplete and should be recomputed as the user
    /// types (Go's `CompletionList.IsIncomplete`). Always `false` in the
    /// reachable subset.
    pub is_incomplete: bool,
    /// The completion items, sorted by label (the reachable analog of Go's
    /// `CompareCompletionEntries`, which sorts by sort-text then label; the
    /// reachable subset carries no per-item sort-text, so the tie-break is the
    /// label).
    pub items: Vec<CompletionItem>,
}

impl LanguageService {
    /// Returns the completion list for the position in `file_name`, or `None`
    /// when there is no such file or the position is not a completion location
    /// (Go's `ProvideCompletion` returning a null response).
    ///
    /// Mirrors the reachable subset of Go's `ProvideCompletion` →
    /// `getCompletionsAtPosition` → `getCompletionData`: convert the LSP position
    /// to a byte offset, classify the position as a member-access (right of a
    /// `.`) or a scope position, and enumerate the matching symbols as
    /// [`lsproto::CompletionItem`]s.
    ///
    /// A member completion on a receiver with no resolvable symbol/type returns
    /// an empty (non-`None`) list, mirroring Go's empty-`symbols`
    /// `completionInfoFromData`.
    ///
    /// Side effects: binds every program file and allocates a checker
    /// (idempotent; via [`LanguageService::file_check_context`]).
    // Go: internal/ls/completions.go:LanguageService.ProvideCompletion / getCompletionsAtPosition
    pub fn provide_completions(
        &mut self,
        file_name: &str,
        position: Position,
    ) -> Option<CompletionList> {
        // Convert the LSP `(line, character)` to a byte offset first (immutable
        // borrows), so the checking context can take `&mut self` afterwards.
        let script = self.document_script(file_name)?;
        let byte_position = self
            .converters()
            .line_and_character_to_position(&script, position)
            .0;
        let mut ctx = self.file_check_context(file_name)?;
        completions_at(&mut ctx, byte_position)
    }
}

/// How a position is classified for completions.
enum Classification {
    /// Right of a `.` on the expression node `e` (member completion on `e`).
    Member(NodeId),
    /// A bare identifier position; complete the symbols in scope at this node.
    Scope(NodeId),
    /// Not a completion location.
    None,
}

/// Resolves the completions for `position` (a byte offset) in `ctx`.
///
/// Side effects: resolves symbols/types through the checker (member path).
// Go: internal/ls/completions.go:getCompletionData (dispatch body)
fn completions_at(ctx: &mut FileCheckContext, position: i32) -> Option<CompletionList> {
    // Classify under a navigation borrow of the view's arena, extracting only
    // node ids; the nav view is dropped before the checker is used (as in
    // `hover`/`definition`).
    let classification = {
        let nav = NavSourceFile::from_borrowed_arena(ctx.view.arena(), ctx.root, ctx.text.clone());
        classify(&nav, position)
    };
    match classification {
        Classification::Member(expression) => Some(member_completions(ctx, expression)),
        Classification::Scope(location) => Some(scope_completions(ctx, location)),
        Classification::None => None,
    }
}

/// Classifies `position` as a member-access or a scope position.
///
/// Mirrors the reachable subset of Go's `getCompletionData` token analysis:
/// `getRelevantTokens` finds the `contextToken`; if it is a `.` the cursor is
/// right of a dot (`isRightOfDot`) and the dotted expression is recovered from
/// the property-access node, otherwise the cursor is a scope/identifier
/// position.
///
/// Side effects: may synthesize navigation tokens (interior mutability).
// Go: internal/ls/completions.go:getCompletionData (contextToken / isRightOfDot)
fn classify(nav: &NavSourceFile, position: i32) -> Classification {
    // Go's `getRelevantTokens`: the previous token, then — when the cursor sits
    // inside a member name / keyword — the token preceding *that*.
    let previous = nav.find_preceding_token(position);
    let context_token = match previous {
        Some(prev)
            if position <= nav.end(prev)
                && (is_member_name(nav.kind(prev)) || is_keyword_kind(nav.kind(prev))) =>
        {
            nav.find_preceding_token(nav.pos(prev))
        }
        _ => previous,
    };

    if let Some(ctx_tok) = context_token {
        // Go: `contextToken.Kind == ast.KindDotToken`. The `?.` optional-chain
        // form (`KindQuestionDotToken`) is deferred.
        if nav.kind(ctx_tok) == Kind::DotToken {
            return classify_after_dot(nav, ctx_tok);
        }
    }

    // Otherwise this is a bare identifier / scope position. Go uses
    // `astnav.GetTouchingPropertyName(file, position)` as the `location`.
    Classification::Scope(nav.get_touching_property_name(position))
}

/// Recovers the dotted expression `e` for a cursor right of the `.` token
/// `dot`.
///
/// Go reads `contextToken.Parent` (the `PropertyAccessExpression`) and takes its
/// `.Expression()`. The Rust `.` token is synthesized by `astnav` (the AST does
/// not store a dot child), so it has no arena parent; instead this climbs from
/// the token left of the dot to the largest node that *ends at the dot*, which
/// is exactly the property access's left operand (Go's `node`).
///
/// Side effects: may synthesize navigation tokens (interior mutability).
// Go: internal/ls/completions.go:getCompletionData (KindPropertyAccessExpression -> node = Expression())
fn classify_after_dot(nav: &NavSourceFile, dot: NodeId) -> Classification {
    let dot_pos = nav.pos(dot);
    let Some(left) = nav.find_preceding_token(dot_pos) else {
        return Classification::None;
    };
    // The dot's left operand must be a real (parsed) node so its arena parents
    // are walkable; a synthesized token (no parent) is not a reachable receiver.
    if is_synthesized_token(left) {
        return Classification::None;
    }
    let arena = nav.arena();
    let mut expr = left;
    while let Some(parent) = arena.parent(expr) {
        if arena.loc(parent).end() == dot_pos {
            expr = parent;
        } else {
            break;
        }
    }
    Classification::Member(expr)
}

/// Enumerates the member completions for the dotted expression `expression`.
///
/// Mirrors the reachable subset of Go's `getTypeScriptMemberSymbols` /
/// `addTypeProperties`: type the expression, then enumerate the apparent type's
/// properties. A receiver with no resolvable symbol/type yields an empty list
/// (Go's empty-`symbols` result), never a panic.
///
/// Side effects: resolves symbols/types through the checker (may cache).
// Go: internal/ls/completions.go:getTypeScriptMemberSymbols / addTypeProperties
fn member_completions(ctx: &mut FileCheckContext, expression: NodeId) -> CompletionList {
    let globals = ctx.view.globals().cloned();
    let Some(symbol) = get_symbol_at_location(
        &mut ctx.checker,
        ctx.view.as_ref(),
        expression,
        globals.as_ref(),
    ) else {
        return CompletionList::default();
    };
    let ty = get_type_of_symbol(
        &mut ctx.checker,
        ctx.view.as_ref(),
        symbol,
        globals.as_ref(),
    );
    let properties = get_properties_of_type(&ctx.checker, ty);

    let view = ctx.view.as_ref();
    let mut items: Vec<CompletionItem> = properties
        .into_iter()
        .map(|(name, property)| completion_item(name, member_completion_item_kind(view, property)))
        .collect();
    sort_items(&mut items);
    CompletionList {
        is_incomplete: false,
        items,
    }
}

/// Enumerates the scope (identifier) completions visible at `location`.
///
/// Mirrors the reachable subset of Go's `getGlobalCompletions` →
/// `GetSymbolsInScope`: walk the binder `locals` tables up the container chain
/// (a global source file's own top-level declarations come from `globals`, so
/// its `locals` are skipped — Go's `!IsGlobalSourceFile` guard), then the
/// program `globals`, keeping the first-seen symbol per name so an inner scope
/// shadows an outer one. The completion `meaning` is value + type + namespace +
/// alias (Go's non-type-only `symbolMeanings`).
///
/// Side effects: none beyond reading the bound view.
// Go: internal/ls/completions.go:getGlobalCompletions + internal/checker/services.go:getSymbolsInScope
fn scope_completions(ctx: &mut FileCheckContext, location: NodeId) -> CompletionList {
    let view = ctx.view.as_ref();
    let arena = view.arena();
    let meaning =
        SymbolFlags::VALUE | SymbolFlags::TYPE | SymbolFlags::NAMESPACE | SymbolFlags::ALIAS;

    let mut seen: HashSet<String> = HashSet::new();
    let mut symbols: Vec<SymbolId> = Vec::new();

    let mut node = Some(location);
    while let Some(n) = node {
        // Go: `canHaveLocals(location) && location.Locals() != nil && !IsGlobalSourceFile(location)`.
        // A (script) source file's top-level declarations are reached via
        // `globals` below, so its own `locals` are skipped here.
        if can_have_locals(arena.kind(n)) && arena.kind(n) != Kind::SourceFile {
            if let Some(locals) = view.locals(n) {
                copy_symbols(view, locals, meaning, &mut seen, &mut symbols);
            }
        }
        // DEFER(phase-7-ls): the module/enum export, class/interface type-
        // parameter, named-function-expression, and `arguments` special cases of
        // `getSymbolsInScope`.
        node = arena.parent(n);
    }

    // Go: `copySymbols(c.globals, meaning)` after the walk.
    if let Some(globals) = view.globals() {
        copy_symbols(view, globals, meaning, &mut seen, &mut symbols);
    }

    let mut items: Vec<CompletionItem> = symbols
        .iter()
        .filter_map(|&id| {
            let name = view.symbol(id).name.clone();
            // Go's `symbolsToArray` drops reserved member names; `getSymbolsInScope`
            // deletes the `this` keyword symbol.
            if is_reserved_member_name(&name) || name == INTERNAL_SYMBOL_NAME_THIS {
                return None;
            }
            Some(completion_item(
                name,
                completion_item_kind(script_element_kind_for_symbol(view, id)),
            ))
        })
        .collect();
    sort_items(&mut items);
    CompletionList {
        is_incomplete: false,
        items,
    }
}

/// Copies every symbol of `source` whose flags intersect `meaning` into `out`,
/// recording its name in `seen` so a later (outer-scope) symbol of the same name
/// is skipped (Go's `copySymbol` first-seen-wins).
///
/// Side effects: none beyond mutating `seen`/`out`.
// Go: internal/checker/services.go:getSymbolsInScope.copySymbols/copySymbol
fn copy_symbols(
    view: &dyn BoundProgram,
    source: &tsgo_ast::SymbolTable,
    meaning: SymbolFlags,
    seen: &mut HashSet<String>,
    out: &mut Vec<SymbolId>,
) {
    for (name, &id) in source {
        if view.symbol(id).flags.intersects(meaning) && !seen.contains(name) {
            seen.insert(name.clone());
            out.push(id);
        }
    }
}

/// Builds a minimal completion item: a label and a kind (the reachable subset of
/// Go's `createLSPCompletionItem`).
///
/// Side effects: none (pure).
// Go: internal/ls/completions.go:createLSPCompletionItem (label + kind)
fn completion_item(label: String, kind: CompletionItemKind) -> CompletionItem {
    CompletionItem {
        label,
        kind: Some(kind),
        ..Default::default()
    }
}

/// Sorts items by label, the reachable analog of Go's `CompareCompletionEntries`
/// (which sorts by sort-text then label; the reachable subset has no per-item
/// sort-text, so the order is the label).
///
/// Side effects: sorts `items` in place.
// Go: internal/ls/completions.go:CompareCompletionEntries (label tie-break)
fn sort_items(items: &mut [CompletionItem]) {
    items.sort_by(|a, b| a.label.cmp(&b.label));
}

/// The completion-item kind for a *member* property `symbol`.
///
/// A checker-synthesized (transient) property symbol — an object-literal or
/// union/intersection member — carries its flags in the checker's transient
/// arena, not the bound program (a `program.symbol(id)` lookup on its tagged id
/// would panic). In the reachable subset such a member is always a property, so
/// it maps to `MemberVariableElement` → `Field`; a program (interface/class)
/// member reads its flags normally.
///
/// Side effects: none (pure).
// Go: internal/ls/completions.go:createCompletionItem (elementKind = GetSymbolKind)
fn member_completion_item_kind(view: &dyn BoundProgram, symbol: SymbolId) -> CompletionItemKind {
    if is_synthesized_symbol(symbol) {
        return completion_item_kind(ScriptElementKind::MemberVariableElement);
    }
    completion_item_kind(script_element_kind_for_symbol(view, symbol))
}

/// The reachable subset of Go's `lsutil.GetSymbolKind`: maps a (program)
/// symbol's flags to a [`ScriptElementKind`].
///
/// Mirrors `getSymbolKindOfConstructorPropertyMethodAccessorFunctionOrVar`
/// (variable / function / accessor / method / constructor / signature /
/// property) followed by the `GetSymbolKind` tail (class / enum / type-alias /
/// interface / type-parameter / enum-member / alias / module). The checker-only
/// refinements — the mapped-type method call-signature check, the
/// `undefined`/`arguments`/`this` special cases, the union-property
/// (`Transient`+`Synthetic`) split, and the local-class-expression form — are
/// deferred (they need the checker's call-signature / root-symbol surface).
///
/// Side effects: none (pure read of the symbol record).
// Go: internal/ls/lsutil/symbol_display.go:GetSymbolKind (reachable subset)
fn script_element_kind_for_symbol(view: &dyn BoundProgram, symbol: SymbolId) -> ScriptElementKind {
    let flags = view.symbol(symbol).flags;
    // getSymbolKindOfConstructorPropertyMethodAccessorFunctionOrVar:
    if flags.intersects(SymbolFlags::VARIABLE) {
        return variable_element_kind(view, symbol);
    }
    if flags.intersects(SymbolFlags::FUNCTION) {
        // FunctionElement vs LocalFunctionElement both map to `Function`; the
        // local distinction is deferred.
        return ScriptElementKind::FunctionElement;
    }
    if flags.intersects(SymbolFlags::GET_ACCESSOR) {
        return ScriptElementKind::MemberGetAccessorElement;
    }
    if flags.intersects(SymbolFlags::SET_ACCESSOR) {
        return ScriptElementKind::MemberSetAccessorElement;
    }
    if flags.intersects(SymbolFlags::METHOD) {
        return ScriptElementKind::MemberFunctionElement;
    }
    if flags.intersects(SymbolFlags::CONSTRUCTOR) {
        return ScriptElementKind::ConstructorImplementationElement;
    }
    if flags.intersects(SymbolFlags::SIGNATURE) {
        return ScriptElementKind::IndexSignatureElement;
    }
    if flags.intersects(SymbolFlags::PROPERTY) {
        return ScriptElementKind::MemberVariableElement;
    }
    // GetSymbolKind tail:
    if flags.intersects(SymbolFlags::CLASS) {
        return ScriptElementKind::ClassElement;
    }
    if flags.intersects(SymbolFlags::ENUM) {
        return ScriptElementKind::EnumElement;
    }
    if flags.intersects(SymbolFlags::TYPE_ALIAS) {
        return ScriptElementKind::TypeElement;
    }
    if flags.intersects(SymbolFlags::INTERFACE) {
        return ScriptElementKind::InterfaceElement;
    }
    if flags.intersects(SymbolFlags::TYPE_PARAMETER) {
        return ScriptElementKind::TypeParameterElement;
    }
    if flags.intersects(SymbolFlags::ENUM_MEMBER) {
        return ScriptElementKind::EnumMemberElement;
    }
    if flags.intersects(SymbolFlags::ALIAS) {
        return ScriptElementKind::Alias;
    }
    if flags.intersects(SymbolFlags::MODULE) {
        return ScriptElementKind::ModuleElement;
    }
    ScriptElementKind::Unknown
}

/// The reachable subset of the variable branch of `GetSymbolKind`: a parameter
/// is `ParameterElement`, an other variable is `VariableElement`. (Both map to
/// `Variable`; the `const`/`let`/`using`/local-variable distinctions are
/// deferred because they do not change the completion kind.)
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/symbol_display.go:getSymbolKindOfConstructorPropertyMethodAccessorFunctionOrVar (Variable branch)
fn variable_element_kind(view: &dyn BoundProgram, symbol: SymbolId) -> ScriptElementKind {
    let s = view.symbol(symbol);
    let declaration = s
        .value_declaration
        .or_else(|| s.declarations.first().copied());
    if let Some(decl) = declaration {
        if view.arena().kind(decl) == Kind::Parameter {
            return ScriptElementKind::ParameterElement;
        }
    }
    ScriptElementKind::VariableElement
}

/// Port of Go's `getCompletionsSymbolKind`: maps a [`ScriptElementKind`] to an
/// [`lsproto::CompletionItemKind`].
///
/// Side effects: none (pure).
// Go: internal/ls/completions.go:getCompletionsSymbolKind
fn completion_item_kind(kind: ScriptElementKind) -> CompletionItemKind {
    use ScriptElementKind as K;
    match kind {
        K::PrimitiveType | K::Keyword => kinds::KEYWORD,
        K::ConstElement
        | K::LetElement
        | K::VariableElement
        | K::LocalVariableElement
        | K::Alias
        | K::ParameterElement => kinds::VARIABLE,
        K::MemberVariableElement | K::MemberGetAccessorElement | K::MemberSetAccessorElement => {
            kinds::FIELD
        }
        K::FunctionElement | K::LocalFunctionElement => kinds::FUNCTION,
        K::MemberFunctionElement
        | K::ConstructSignatureElement
        | K::CallSignatureElement
        | K::IndexSignatureElement => kinds::METHOD,
        K::EnumElement => kinds::ENUM,
        K::EnumMemberElement => kinds::ENUM_MEMBER,
        K::ModuleElement | K::ExternalModuleName => kinds::MODULE,
        K::ClassElement | K::TypeElement => kinds::CLASS,
        K::InterfaceElement => kinds::INTERFACE,
        K::Warning => kinds::TEXT,
        K::ScriptElement => kinds::FILE,
        K::Directory => kinds::FOLDER,
        K::String => kinds::CONSTANT,
        _ => kinds::PROPERTY,
    }
}

/// The LSP `CompletionItemKind` wire values used by [`completion_item_kind`].
///
/// The `lsproto` crate currently exposes only `CompletionItemKind::VARIABLE`;
/// the remaining kinds are constructed from their stable LSP integer values via
/// the public tuple field. (This lane may not edit `lsproto`.)
// Go: internal/lsp/lsproto/lsp_generated.go:CompletionItemKind (wire values)
mod kinds {
    use tsgo_lsproto::CompletionItemKind;

    pub const TEXT: CompletionItemKind = CompletionItemKind(1);
    pub const METHOD: CompletionItemKind = CompletionItemKind(2);
    pub const FUNCTION: CompletionItemKind = CompletionItemKind(3);
    pub const FIELD: CompletionItemKind = CompletionItemKind(5);
    pub const VARIABLE: CompletionItemKind = CompletionItemKind(6);
    pub const CLASS: CompletionItemKind = CompletionItemKind(7);
    pub const INTERFACE: CompletionItemKind = CompletionItemKind(8);
    pub const MODULE: CompletionItemKind = CompletionItemKind(9);
    pub const PROPERTY: CompletionItemKind = CompletionItemKind(10);
    pub const ENUM: CompletionItemKind = CompletionItemKind(13);
    pub const KEYWORD: CompletionItemKind = CompletionItemKind(14);
    pub const FILE: CompletionItemKind = CompletionItemKind(17);
    pub const FOLDER: CompletionItemKind = CompletionItemKind(19);
    pub const ENUM_MEMBER: CompletionItemKind = CompletionItemKind(20);
    pub const CONSTANT: CompletionItemKind = CompletionItemKind(21);
}

/// Reports whether `kind` is a member name (Go's `ast.IsMemberName`).
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:IsMemberName
fn is_member_name(kind: Kind) -> bool {
    matches!(kind, Kind::Identifier | Kind::PrivateIdentifier)
}

/// Reports whether `node` is a locals-bearing container (Go's
/// `checker.canHaveLocals`).
///
/// Side effects: none (pure).
// Go: internal/checker/utilities.go:canHaveLocals
fn can_have_locals(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::ArrowFunction
            | Kind::Block
            | Kind::CallSignature
            | Kind::CaseBlock
            | Kind::CatchClause
            | Kind::ClassStaticBlockDeclaration
            | Kind::ConditionalType
            | Kind::Constructor
            | Kind::ConstructorType
            | Kind::ConstructSignature
            | Kind::ForStatement
            | Kind::ForInStatement
            | Kind::ForOfStatement
            | Kind::FunctionDeclaration
            | Kind::FunctionExpression
            | Kind::FunctionType
            | Kind::GetAccessor
            | Kind::IndexSignature
            | Kind::JSDocSignature
            | Kind::MappedType
            | Kind::MethodDeclaration
            | Kind::MethodSignature
            | Kind::ModuleDeclaration
            | Kind::SetAccessor
            | Kind::SourceFile
            | Kind::TypeAliasDeclaration
            | Kind::JSTypeAliasDeclaration
    )
}

/// Reports whether `name` is a reserved internal member name the binder uses for
/// signatures (`__index` / `__call` / `__new`), which `symbolsToArray` and
/// `getNamedMembers` exclude. Well-known-symbol (`__@iterator`) and private
/// (`__#x`) names are real members.
///
/// Side effects: none (pure).
// Go: internal/checker/utilities.go:isReservedMemberName
fn is_reserved_member_name(name: &str) -> bool {
    let mut chars = name.chars();
    if chars.next() != Some('\u{FE}') {
        return false;
    }
    matches!(chars.next(), Some(second) if second != '@' && second != '#')
}

/// The high-bit tag the checker sets on synthesized (transient) symbol ids
/// (object-literal / union / mapped members), whose flags and name live in the
/// checker's transient arena rather than the bound program. A bound-program
/// `symbol(id)` lookup on such an id would index out of bounds, so the member
/// kind path treats a tagged id as a property without consulting the program.
///
/// Mirrors `internal/checker/core/mod.rs`'s private `SYNTHESIZED_SYMBOL_TAG`;
/// duplicated here because it is not part of the checker's public API.
// Go: internal/checker/checker.go (transient symbols live on the checker, not the program)
const SYNTHESIZED_SYMBOL_TAG: u32 = 1 << 31;

/// Reports whether `symbol` is a checker-synthesized (transient) symbol id.
///
/// Side effects: none (pure).
fn is_synthesized_symbol(symbol: SymbolId) -> bool {
    symbol.0 & SYNTHESIZED_SYMBOL_TAG != 0
}

/// Reports whether `node` is an `astnav`-synthesized token id (a token the AST
/// does not store, e.g. a `.`), which has no parent in the parsed arena.
///
/// Side effects: none (pure).
fn is_synthesized_token(node: NodeId) -> bool {
    node.0 & SYNTHESIZED_SYMBOL_TAG != 0
}

#[cfg(test)]
#[path = "completions_test.rs"]
mod tests;
