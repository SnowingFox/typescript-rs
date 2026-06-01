//! Port of Go `internal/ls/symbols.go`: the document-symbols / navigation-tree
//! feature (`textDocument/documentSymbol`).
//!
//! Go's `ProvideDocumentSymbols` walks the parsed source file and builds the
//! hierarchical navigation tree: a [`DocumentSymbol`] per top-level
//! function / class / interface / enum / namespace / type-alias / variable, with
//! class & namespace members nested as children. Each symbol carries a `name`, a
//! [`SymbolKind`], the full declaration `range`, and the `selectionRange`
//! covering just its name.
//!
//! # Reachable subset
//!
//! [`LanguageService::provide_document_symbols`] is purely syntactic (no
//! checker): it reads the program's already-parsed source file (its
//! [`NodeArena`](tsgo_ast::NodeArena) + root) and ports
//! `getDocumentSymbolsForChildren` / `newDocumentSymbol` /
//! `getSymbolKindFromNode` for the declaration tree — classes, interfaces,
//! enums (+ members), namespaces (+ members, with same-name namespace merging),
//! functions / methods / accessors / constructors (+ bodies), variable /
//! property / binding-element declarations (+ initializers), spread
//! assignments, and the interface/type-literal member group (method & property
//! signatures, call / construct / index signatures, enum members, shorthand
//! properties, type aliases).
//!
//! Because `tsgo_lsproto` does not yet carry a generated `DocumentSymbol`, this
//! module defines the LSP-shaped [`DocumentSymbol`] locally (mirroring
//! `lsproto.DocumentSymbol`), the same way [`crate::documenthighlights`] defines
//! its highlight types.
//!
//! DEFER(phase-7-ls): the flat `SymbolInformation` fallback
//! (`getDocumentSymbolInformations`, for clients without hierarchical support),
//! the JS expando merging and `module.exports` / `exports.x` assignment
//! declarations (`getAssignmentDeclarationKind` cases in `visit`), the JSDoc
//! `@typedef`/`@callback` symbols, the `import` clause / `import =` / export
//! specifier symbols, the `export =` / `export default` assignment symbols, the
//! call-expression callback labels in `getUnnamedNodeLabel`, and the `detail` /
//! `tags` / `deprecated` symbol fields.
//! blocked-by: the JS assignment-declaration machinery
//! (`getAssignmentDeclarationKind`), the JSDoc reparser, and the
//! `GetClientCapabilities` `HierarchicalDocumentSymbolSupport` surface (the flat
//! fallback). Also DEFER: the workspace-symbol / nav-to search
//! (`ProvideWorkspaceSymbols`), which needs a multi-file program view.

use tsgo_ast::{Kind, ModifierFlags, NodeArena, NodeData, NodeFlags, NodeId};
use tsgo_astnav::{get_start_of_node, NavSourceFile};
use tsgo_core::text::TextPos;
use tsgo_ls_lsconv::Converters;
use tsgo_lsproto::{Position, Range, SymbolKind};
use tsgo_scanner::skip_trivia;

use crate::languageservice::{DocumentScript, LanguageService};

/// A document symbol (LSP `DocumentSymbol`): a named node in the navigation
/// tree, with its declaration `range`, the `selection_range` covering its name,
/// and nested `children`.
///
/// Side effects: none (a plain data record).
// Go: internal/lsp/lsproto/lsp_generated.go:DocumentSymbol
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentSymbol {
    /// The symbol's display name.
    pub name: String,
    /// The symbol's [`SymbolKind`].
    pub kind: SymbolKind,
    /// The full range covering the symbol's declaration.
    pub range: Range,
    /// The range covering just the symbol's name (the editor selects this).
    pub selection_range: Range,
    /// The nested child symbols (members of a class / namespace, etc.).
    pub children: Vec<DocumentSymbol>,
}

impl LanguageService {
    /// Returns the hierarchical document symbols for `file_name`: the navigation
    /// tree of top-level declarations, with class & namespace members nested.
    ///
    /// Empty when there is no such file.
    ///
    /// Mirrors the reachable subset of Go's `ProvideDocumentSymbols` →
    /// `getDocumentSymbolsForChildren`: walk the AST, build a [`DocumentSymbol`]
    /// per declaration (name + [`SymbolKind`] + UTF-16 ranges via the project
    /// [`Converters`](tsgo_ls_lsconv::Converters)), and merge same-name
    /// namespaces.
    ///
    /// Side effects: none (reads the already-parsed file; no binding/checking).
    // Go: internal/ls/symbols.go:LanguageService.ProvideDocumentSymbols
    pub fn provide_document_symbols(&self, file_name: &str) -> Vec<DocumentSymbol> {
        let Some(script) = self.document_script(file_name) else {
            return Vec::new();
        };
        let Some(parsed) = self.program().get_source_file(file_name) else {
            return Vec::new();
        };
        let nav = NavSourceFile::from_borrowed_arena(
            parsed.arena(),
            parsed.node(),
            parsed.text().to_string(),
        );
        let ctx = SymbolCtx {
            nav: &nav,
            converters: self.converters(),
            script: &script,
        };
        get_document_symbols_for_children(&ctx, nav.root())
    }
}

/// The context for one document-symbol query: the navigation engine plus the
/// converters + document needed to map byte offsets to UTF-16 LSP positions.
struct SymbolCtx<'a, 'b> {
    nav: &'b NavSourceFile<'a>,
    converters: &'b Converters,
    script: &'b DocumentScript,
}

/// The maximum symbol-name length before truncation (Go's `maxLength`).
const MAX_LENGTH: usize = 150;

/// Builds the (merged) document-symbol list for `node`'s direct children, the
/// top-level entry point that applies same-name namespace merging.
///
/// Side effects: may synthesize tokens in `nav`'s side store.
// Go: internal/ls/symbols.go:getDocumentSymbolsForChildren
fn get_document_symbols_for_children(ctx: &SymbolCtx<'_, '_>, node: NodeId) -> Vec<DocumentSymbol> {
    let symbols = get_symbols_for_children(ctx, node);
    merge_expandos(symbols)
}

/// Collects the (raw, unmerged) symbols for `node`'s direct children by visiting
/// each child (Go's `getSymbolsForChildren`).
///
/// Side effects: may synthesize tokens in `nav`'s side store.
// Go: internal/ls/symbols.go:getSymbolsForChildren
fn get_symbols_for_children(ctx: &SymbolCtx<'_, '_>, node: NodeId) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    let mut children = Vec::new();
    ctx.nav.arena().for_each_child(node, &mut |child| {
        children.push(child);
        false
    });
    for child in children {
        visit(ctx, child, &mut out);
    }
    out
}

/// Visits one node, appending the symbol(s) it contributes to `out` (Go's
/// `visit` closure, reachable subset).
///
/// Side effects: may synthesize tokens in `nav`'s side store.
// Go: internal/ls/symbols.go:getDocumentSymbolsForChildren (visit)
fn visit(ctx: &SymbolCtx<'_, '_>, node: NodeId, out: &mut Vec<DocumentSymbol>) {
    let arena = ctx.nav.arena();
    match arena.kind(node) {
        Kind::ClassDeclaration
        | Kind::ClassExpression
        | Kind::InterfaceDeclaration
        | Kind::EnumDeclaration => {
            let children = get_symbols_for_children(ctx, node);
            add_symbol_for_node(ctx, node, None, children, out);
        }
        Kind::ModuleDeclaration => {
            let interior = get_interior_module(arena, node);
            let children = get_symbols_for_children(ctx, interior);
            add_symbol_for_node(ctx, node, None, children, out);
        }
        Kind::Constructor => {
            let children = match body_of(arena, node) {
                Some(body) => get_symbols_for_children(ctx, body),
                None => Vec::new(),
            };
            add_symbol_for_node(ctx, node, None, children, out);
            // Parameter properties (`constructor(public x)`) become members.
            for param in parameters_of(arena, node) {
                if is_parameter_property(arena, param) {
                    add_symbol_for_node(ctx, param, None, Vec::new(), out);
                }
            }
        }
        Kind::FunctionDeclaration
        | Kind::FunctionExpression
        | Kind::ArrowFunction
        | Kind::MethodDeclaration
        | Kind::GetAccessor
        | Kind::SetAccessor => {
            let children = match body_of(arena, node) {
                Some(body) => get_symbols_for_children(ctx, body),
                None => Vec::new(),
            };
            add_symbol_for_node(ctx, node, None, children, out);
        }
        Kind::VariableDeclaration
        | Kind::BindingElement
        | Kind::PropertyAssignment
        | Kind::PropertyDeclaration => {
            if let Some(name) = node_name(arena, node) {
                if is_binding_pattern(arena.kind(name)) {
                    visit(ctx, name, out);
                } else {
                    let children = match initializer_of(arena, node) {
                        Some(init) => get_symbols_for_children(ctx, init),
                        None => Vec::new(),
                    };
                    add_symbol_for_node(ctx, node, None, children, out);
                }
            }
        }
        Kind::SpreadAssignment => {
            let expr = match arena.data(node) {
                NodeData::SpreadAssignment(d) => Some(d.expression),
                _ => None,
            };
            add_symbol_for_node(ctx, node, expr, Vec::new(), out);
        }
        Kind::MethodSignature
        | Kind::PropertySignature
        | Kind::CallSignature
        | Kind::ConstructSignature
        | Kind::IndexSignature
        | Kind::EnumMember
        | Kind::ShorthandPropertyAssignment
        | Kind::TypeAliasDeclaration => {
            add_symbol_for_node(ctx, node, None, Vec::new(), out);
        }
        _ => {
            // The reachable subset recurses generically for everything else
            // (DEFER: JS expandos, import clauses, export assignments, JSDoc).
            let mut children = Vec::new();
            arena.for_each_child(node, &mut |child| {
                children.push(child);
                false
            });
            for child in children {
                visit(ctx, child, out);
            }
        }
    }
}

/// Builds and appends a [`DocumentSymbol`] for `node` (skipping reparsed nodes),
/// using the given `name` node and pre-collected `children`.
///
/// Side effects: may synthesize tokens in `nav`'s side store.
// Go: internal/ls/symbols.go:getDocumentSymbolsForChildren (addSymbolForNode)
fn add_symbol_for_node(
    ctx: &SymbolCtx<'_, '_>,
    node: NodeId,
    name: Option<NodeId>,
    children: Vec<DocumentSymbol>,
    out: &mut Vec<DocumentSymbol>,
) {
    if ctx.nav.arena().flags(node).contains(NodeFlags::REPARSED) {
        return;
    }
    if let Some(symbol) = new_document_symbol(ctx, node, name, children) {
        out.push(symbol);
    }
}

/// Builds a [`DocumentSymbol`] for `node` (Go's `newDocumentSymbol`), or `None`
/// when the symbol would have an empty name.
///
/// Side effects: may synthesize tokens in `nav`'s side store.
// Go: internal/ls/symbols.go:LanguageService.newDocumentSymbol
fn new_document_symbol(
    ctx: &SymbolCtx<'_, '_>,
    node: NodeId,
    name: Option<NodeId>,
    children: Vec<DocumentSymbol>,
) -> Option<DocumentSymbol> {
    let arena = ctx.nav.arena();
    let text = ctx.nav.text();
    let node_start_pos = skip_trivia(text, arena.loc(node).pos());
    let name = name.or_else(|| name_of_declaration(arena, node));

    let (mut label, name_start_pos, name_end_pos) =
        if arena.kind(node) == Kind::ModuleDeclaration && !is_ambient_module(arena, node) {
            let name = name?;
            let interior = get_interior_module(arena, node);
            let interior_name = module_name(arena, interior)?;
            (
                get_module_name(arena, node),
                skip_trivia(text, arena.loc(name).pos()),
                arena.loc(interior_name).end(),
            )
        } else if let Some(name) = name {
            (
                get_text_of_name(ctx, name),
                skip_trivia(text, arena.loc(name).pos()).max(node_start_pos),
                arena.loc(name).end().max(node_start_pos),
            )
        } else {
            (
                get_unnamed_node_label(arena, node),
                node_start_pos,
                node_start_pos,
            )
        };

    if label.is_empty() {
        return None;
    }
    let truncated = truncate_by_runes(&label, MAX_LENGTH);
    if truncated.len() < label.len() {
        label = format!("{truncated}...");
    }

    Some(DocumentSymbol {
        name: label,
        kind: get_symbol_kind_from_node(arena, node),
        range: Range {
            start: ctx.position(node_start_pos),
            end: ctx.position(arena.loc(node).end()),
        },
        selection_range: Range {
            start: ctx.position(name_start_pos),
            end: ctx.position(name_end_pos),
        },
        children,
    })
}

impl SymbolCtx<'_, '_> {
    /// Converts an internal byte offset to a UTF-16 LSP position.
    fn position(&self, pos: i32) -> Position {
        self.converters
            .position_to_line_and_character(self.script, TextPos(pos))
    }
}

/// Merges same-name namespaces into their first occurrence and recursively
/// merges every symbol's children (Go's `mergeExpandos`).
///
/// The JS expando-property merge (folding `A.b = ...` properties into a `A`
/// class / function / variable) is DEFERRED with the assignment-declaration
/// machinery, so the only merging the reachable subset performs is the
/// duplicate-namespace merge.
///
/// Side effects: none (pure list manipulation).
// Go: internal/ls/symbols.go:mergeExpandos
fn merge_expandos(symbols: Vec<DocumentSymbol>) -> Vec<DocumentSymbol> {
    let mut result: Vec<DocumentSymbol> = Vec::new();
    // name -> index in `result` of the first namespace with that name.
    let mut namespace_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for mut symbol in symbols {
        // Recurse into children first (Go recurses before merging).
        symbol.children = merge_expandos(std::mem::take(&mut symbol.children));

        if is_anonymous_name(&symbol.name) {
            result.push(symbol);
            continue;
        }
        if symbol.kind == SymbolKind(3) {
            if let Some(&target) = namespace_index.get(&symbol.name) {
                merge_children(&mut result[target], symbol);
                continue;
            }
            namespace_index.insert(symbol.name.clone(), result.len());
        }
        result.push(symbol);
    }
    result
}

/// Merges `source`'s children into `target` and re-merges + sorts them by range
/// (Go's `mergeChildren`; `newDocumentSymbol` always sets `Children`, so this
/// always takes the combine-and-sort branch).
// Go: internal/ls/symbols.go:mergeChildren
fn merge_children(target: &mut DocumentSymbol, source: DocumentSymbol) {
    let mut combined = std::mem::take(&mut target.children);
    combined.extend(source.children);
    let mut merged = merge_expandos(combined);
    merged.sort_by(|a, b| compare_ranges(&a.range, &b.range));
    target.children = merged;
}

/// Compares two ranges by `(start, end)` (Go's `lsproto.CompareRanges`).
// Go: internal/lsp/lsproto/util.go:CompareRanges
fn compare_ranges(a: &Range, b: &Range) -> std::cmp::Ordering {
    (a.start.line, a.start.character, a.end.line, a.end.character).cmp(&(
        b.start.line,
        b.start.character,
        b.end.line,
        b.end.character,
    ))
}

/// Reports whether `name` is an anonymous / synthetic label that never merges
/// (Go's `isAnonymousName`).
// Go: internal/ls/symbols.go:isAnonymousName
fn is_anonymous_name(name: &str) -> bool {
    matches!(
        name,
        "<function>" | "<class>" | "export=" | "default" | "constructor" | "()" | "new()" | "[]"
    ) || name.ends_with(") callback")
}

/// Reports whether `kind` is an object/array binding pattern.
// Go: internal/ast/utilities.go:IsBindingPattern
fn is_binding_pattern(kind: Kind) -> bool {
    matches!(kind, Kind::ObjectBindingPattern | Kind::ArrayBindingPattern)
}

/// Reports whether `param` is a constructor parameter-property (`public x`).
// Go: internal/ast/utilities.go:IsParameterPropertyDeclaration
fn is_parameter_property(arena: &NodeArena, param: NodeId) -> bool {
    match arena.data(param) {
        NodeData::ParameterDeclaration(d) => d.modifiers.as_ref().is_some_and(|m| {
            m.modifier_flags
                .intersects(ModifierFlags::PARAMETER_PROPERTY_MODIFIER)
        }),
        _ => false,
    }
}

/// Returns the name node of `node` for the declaration kinds the reachable
/// subset builds symbols for (Go's `ast.GetNameOfDeclaration`, reachable subset).
// Go: internal/ast/utilities.go:GetNameOfDeclaration
fn name_of_declaration(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.name,
        NodeData::ClassDeclaration(d)
        | NodeData::ClassExpression(d)
        | NodeData::InterfaceDeclaration(d) => d.name,
        NodeData::EnumDeclaration(d) => Some(d.name),
        NodeData::MethodDeclaration(d) => Some(d.name),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => Some(d.name),
        NodeData::PropertyDeclaration(d) | NodeData::PropertySignature(d) => Some(d.name),
        NodeData::PropertyAssignment(d) => Some(d.name),
        NodeData::MethodSignature(d) => Some(d.name),
        NodeData::ShorthandPropertyAssignment(d) => Some(d.name),
        NodeData::EnumMember(d) => Some(d.name),
        NodeData::VariableDeclaration(d) => Some(d.name),
        NodeData::TypeAliasDeclaration(d) => Some(d.name),
        NodeData::TypeParameterDeclaration(d) => Some(d.name),
        NodeData::ParameterDeclaration(d) => Some(d.name),
        NodeData::ModuleDeclaration(d) => Some(d.name),
        NodeData::BindingElement(d) => d.name,
        _ => None,
    }
}

/// Returns the `name` node of a variable / binding / property declaration.
// Go: internal/ast/utilities.go:Node.Name (reachable subset)
fn node_name(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::VariableDeclaration(d) => Some(d.name),
        NodeData::BindingElement(d) => d.name,
        NodeData::PropertyAssignment(d) => Some(d.name),
        NodeData::PropertyDeclaration(d) => Some(d.name),
        _ => None,
    }
}

/// Returns the `initializer` of a variable / binding / property declaration.
// Go: internal/ast/utilities.go:Node.Initializer (reachable subset)
fn initializer_of(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::VariableDeclaration(d) => d.initializer,
        NodeData::BindingElement(d) => d.initializer,
        NodeData::PropertyAssignment(d) => d.initializer,
        NodeData::PropertyDeclaration(d) => d.initializer,
        _ => None,
    }
}

/// Returns the body of a function-like node, if it has one.
// Go: internal/ast/utilities.go:Node.Body (reachable subset)
fn body_of(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.body,
        NodeData::MethodDeclaration(d) => d.body,
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => d.body,
        NodeData::ConstructorDeclaration(d) => d.body,
        NodeData::ArrowFunction(d) => Some(d.body),
        _ => None,
    }
}

/// Returns the parameter node ids of a function-like node (for parameter
/// properties).
// Go: internal/ast/utilities.go:Node.Parameters (reachable subset)
fn parameters_of(arena: &NodeArena, node: NodeId) -> Vec<NodeId> {
    match arena.data(node) {
        NodeData::ConstructorDeclaration(d) => d.parameters.nodes.clone(),
        _ => Vec::new(),
    }
}

/// Walks to the innermost module declaration of a (possibly dotted) namespace
/// (`namespace A.B.C` collapses to `C`).
// Go: internal/ls/symbols.go:getInteriorModule
fn get_interior_module(arena: &NodeArena, mut node: NodeId) -> NodeId {
    while let NodeData::ModuleDeclaration(d) = arena.data(node) {
        match d.body {
            Some(body) if arena.kind(body) == Kind::ModuleDeclaration => node = body,
            _ => break,
        }
    }
    node
}

/// Returns a module declaration's name node, if any.
fn module_name(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    match arena.data(node) {
        NodeData::ModuleDeclaration(d) => Some(d.name),
        _ => None,
    }
}

/// Reports whether a module declaration is an ambient module (`declare module
/// "x"`), i.e. its name is a string literal.
// Go: internal/ast/utilities.go:IsAmbientModule
fn is_ambient_module(arena: &NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::ModuleDeclaration(d) => arena.kind(d.name) == Kind::StringLiteral,
        _ => false,
    }
}

/// Builds the dotted display name of a (possibly nested) namespace declaration.
// Go: internal/ls/symbols.go:getModuleName
fn get_module_name(arena: &NodeArena, mut node: NodeId) -> String {
    let mut result = match module_name(arena, node) {
        Some(name) => arena.text(name).to_string(),
        None => String::new(),
    };
    while let NodeData::ModuleDeclaration(d) = arena.data(node) {
        match d.body {
            Some(body) if arena.kind(body) == Kind::ModuleDeclaration => {
                node = body;
                if let Some(name) = module_name(arena, node) {
                    result.push('.');
                    result.push_str(arena.text(name));
                }
            }
            _ => break,
        }
    }
    result
}

/// Returns the display text of a name node (Go's `getTextOfName`, reachable
/// subset): identifiers / numeric literals use their text, string literals are
/// quoted, and other names fall back to their source slice.
// Go: internal/ls/symbols.go:getTextOfName
fn get_text_of_name(ctx: &SymbolCtx<'_, '_>, node: NodeId) -> String {
    let arena = ctx.nav.arena();
    match arena.kind(node) {
        Kind::Identifier | Kind::PrivateIdentifier | Kind::NumericLiteral => {
            arena.text(node).to_string()
        }
        Kind::StringLiteral => format!("\"{}\"", arena.text(node)),
        Kind::NoSubstitutionTemplateLiteral => format!("`{}`", arena.text(node)),
        _ => {
            // Go's `scanner.GetTextOfNode`: the node's source text.
            let start = get_start_of_node(ctx.nav, node, false) as usize;
            let end = ctx.nav.end(node) as usize;
            ctx.nav.text()[start..end].to_string()
        }
    }
}

/// Returns the synthetic label for an unnamed declaration (Go's
/// `getUnnamedNodeLabel`, reachable subset; the call-expression callback labels
/// are deferred).
// Go: internal/ls/symbols.go:getUnnamedNodeLabel
fn get_unnamed_node_label(arena: &NodeArena, node: NodeId) -> String {
    let label = match arena.kind(node) {
        Kind::FunctionDeclaration | Kind::FunctionExpression | Kind::ArrowFunction => {
            if has_default_modifier(arena, node) {
                "default"
            } else {
                "<function>"
            }
        }
        Kind::ClassDeclaration | Kind::ClassExpression => {
            if has_default_modifier(arena, node) {
                "default"
            } else {
                "<class>"
            }
        }
        Kind::Constructor => "constructor",
        Kind::CallSignature => "()",
        Kind::ConstructSignature => "new()",
        Kind::IndexSignature => "[]",
        _ => "",
    };
    label.to_string()
}

/// Reports whether a declaration carries the `default` modifier.
fn has_default_modifier(arena: &NodeArena, node: NodeId) -> bool {
    let modifiers = match arena.data(node) {
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.modifiers.as_ref(),
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => d.modifiers.as_ref(),
        NodeData::ArrowFunction(d) => d.modifiers.as_ref(),
        _ => None,
    };
    modifiers.is_some_and(|m| m.modifier_flags.contains(ModifierFlags::DEFAULT))
}

/// Truncates `s` to at most `max` Unicode scalar values.
// Go: internal/stringutil/stringutil.go:TruncateByRunes
fn truncate_by_runes(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Maps an AST node to its LSP [`SymbolKind`] (Go's `getSymbolKindFromNode`,
/// reachable subset; the JS assignment-declaration cases fall through to
/// `Variable`).
// Go: internal/ls/symbols.go:getSymbolKindFromNode
fn get_symbol_kind_from_node(arena: &NodeArena, node: NodeId) -> SymbolKind {
    match arena.kind(node) {
        Kind::ModuleDeclaration => SymbolKind(3),
        Kind::ClassDeclaration | Kind::ClassExpression => SymbolKind(5),
        Kind::InterfaceDeclaration => SymbolKind(11),
        Kind::TypeAliasDeclaration => SymbolKind(5),
        Kind::EnumDeclaration => SymbolKind(10),
        Kind::VariableDeclaration => SymbolKind(13),
        Kind::ArrowFunction | Kind::FunctionDeclaration | Kind::FunctionExpression => {
            SymbolKind(12)
        }
        Kind::GetAccessor | Kind::SetAccessor => SymbolKind(7),
        Kind::MethodDeclaration | Kind::MethodSignature => SymbolKind(6),
        Kind::PropertyDeclaration
        | Kind::PropertySignature
        | Kind::PropertyAssignment
        | Kind::ShorthandPropertyAssignment
        | Kind::SpreadAssignment
        | Kind::IndexSignature => SymbolKind(7),
        Kind::CallSignature => SymbolKind(6),
        Kind::ConstructSignature => SymbolKind(9),
        Kind::Constructor | Kind::ClassStaticBlockDeclaration => SymbolKind(9),
        Kind::TypeParameter => SymbolKind(26),
        Kind::EnumMember => SymbolKind(22),
        Kind::Parameter => {
            if is_parameter_property(arena, node) {
                SymbolKind(7)
            } else {
                SymbolKind(13)
            }
        }
        Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral | Kind::NumericLiteral => {
            SymbolKind(7)
        }
        _ => SymbolKind(13),
    }
}

#[cfg(test)]
#[path = "symbols_test.rs"]
mod tests;
