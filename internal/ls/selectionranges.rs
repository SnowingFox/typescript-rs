//! Port of Go `internal/ls/selectionranges.go`: the smart-selection-ranges
//! feature (`textDocument/selectionRange`).
//!
//! Go's `ProvideSelectionRanges` converts each requested LSP position to a byte
//! offset and calls `getSmartSelectionRange`, which walks the AST from the
//! source file down to the deepest node containing the position, pushing a
//! [`SelectionRange`] for each node (and a few synthesized stops) so the client
//! can expand/shrink the selection outward through the ancestor chain.
//!
//! This is a purely syntactic feature: it needs no checker, only the program's
//! already-parsed source file (its [`NodeArena`](tsgo_ast::NodeArena) + root)
//! driven through [`astnav`](tsgo_astnav)'s shared-borrow
//! [`NavSourceFile`](tsgo_astnav::NavSourceFile).
//!
//! # Divergence from Go
//!
//! Go drives `current.VisitEachChild` with an `ast.NodeVisitor` carrying two
//! hooks: a per-node `Visit` and a per-list `VisitNodes`. `tsgo_ast`'s
//! shared-borrow surface exposes [`for_each_child`](tsgo_ast::NodeArena::for_each_child)
//! (a flat, source-ordered child stream) but no list-aware visitor that runs
//! with only a shared borrow. So this port splits the one pass into two over the
//! same children: [`Walk::push_child_list_spans`] reproduces the `VisitNodes`
//! hook (the list-span stops), then a
//! [`for_each_child`](tsgo_ast::NodeArena::for_each_child) pass reproduces the
//! per-node `Visit`. Sibling nodes and list spans are disjoint, and a list span
//! is always pushed before the element it contains, so the resulting parent
//! chain is identical to Go's interleaved traversal.
//!
//! # DEFER
//!
//! DEFER(phase-3): the JSDoc-specific stops (`current.JSDoc(...)` visiting and
//! the `JSDocTypeExpression`/`JSDocSignature`/`JSDocTypeLiteral` skips) are
//! ported structurally but inert — the parser has not reparsed JSDoc, so no node
//! carries cached JSDoc and the tree contains no JSDoc-kind nodes.
//! blocked-by: JSDoc reparser (tsgo_parser).

use tsgo_ast::{Kind, NodeData, NodeId, NodeList};
use tsgo_astnav::{get_start_of_node, NavSourceFile};
use tsgo_core::text::{TextPos, TextRange};
use tsgo_ls_lsconv::{Converters, Script};
use tsgo_lsproto::{Position, SelectionRange};
use tsgo_scanner::get_trailing_comment_ranges;

use crate::languageservice::LanguageService;

impl LanguageService {
    /// Returns one [`SelectionRange`] chain per requested position: the deepest
    /// AST node containing the position, with `parent` pointers walking outward
    /// to the whole source file (so the editor can expand/shrink the selection).
    ///
    /// Empty when there is no such file. A position that lands on no inner node
    /// still yields the full-file range (Go always returns a range), so this
    /// never panics on an empty file or a position at the file boundary.
    ///
    /// Mirrors Go's `ProvideSelectionRanges`: convert each LSP position to a byte
    /// offset via the project [`Converters`](tsgo_ls_lsconv::Converters), then
    /// run `getSmartSelectionRange`.
    ///
    /// Side effects: none (reads the already-parsed file; no binding/checking).
    // Go: internal/ls/selectionranges.go:LanguageService.ProvideSelectionRanges
    pub fn provide_selection_ranges(
        &self,
        file_name: &str,
        positions: &[Position],
    ) -> Vec<SelectionRange> {
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
        let converters = self.converters();

        positions
            .iter()
            .map(|position| {
                let pos = converters
                    .line_and_character_to_position(&script, position.clone())
                    .0;
                let walk = Walk {
                    nav: &nav,
                    converters,
                    script: &script,
                    pos,
                };
                walk.run()
            })
            .collect()
    }
}

/// The traversal state for one selection-range query: the navigation context,
/// the position converters, the document, and the target byte offset.
///
/// Side effects: none on its own (its methods may synthesize tokens in `nav`).
struct Walk<'a, 'b> {
    nav: &'a NavSourceFile<'b>,
    converters: &'a Converters,
    script: &'a dyn Script,
    pos: i32,
}

impl Walk<'_, '_> {
    /// Walks from the source file down to the deepest node containing `pos`,
    /// returning the [`SelectionRange`] parent chain (innermost first).
    ///
    /// Side effects: may synthesize tokens in `nav`'s side store.
    // Go: internal/ls/selectionranges.go:getSmartSelectionRange
    fn run(&self) -> SelectionRange {
        let root = self.nav.root();
        let full_range = self.converters.to_lsp_range(
            self.script,
            TextRange::new(self.nav.pos(root), self.nav.end(root)),
        );
        let mut result = SelectionRange {
            range: full_range,
            parent: None,
        };

        let mut current = Some(root);
        while let Some(cur) = current {
            // Go: `parent := current` — the node whose children are visited.
            let parent = cur;
            let mut next: Option<NodeId> = None;

            // Go visits `current.JSDoc(sourceFile)` first. DEFER(phase-3): always
            // empty (no reparsed JSDoc). blocked-by: JSDoc reparser (tsgo_parser).

            // Pass A — the `VisitNodes` hook: a stop per child node list span.
            self.push_child_list_spans(cur, &mut result);

            // Pass B — the `Visit` hook: the child node containing `pos`.
            let mut children = Vec::new();
            self.nav.arena().for_each_child(cur, &mut |c| {
                children.push(c);
                false
            });
            for node in children {
                if next.is_some() {
                    break;
                }
                self.visit(parent, node, &mut result, &mut next);
            }

            current = next;
        }
        result
    }

    /// The per-node `Visit` hook: when `node` contains `pos`, push the stops it
    /// contributes (a multi-line function body, the node's own range) and record
    /// it as the node to descend into next.
    ///
    /// Side effects: may synthesize tokens in `nav`'s side store.
    // Go: internal/ls/selectionranges.go:getSmartSelectionRange (visit closure)
    fn visit(
        &self,
        parent: NodeId,
        node: NodeId,
        result: &mut SelectionRange,
        next: &mut Option<NodeId>,
    ) {
        // A single-line comment trailing `node` gets its own stop (the whole
        // comment, then its content after the `//`).
        if let Some(comment) = get_trailing_comment_ranges(self.nav.text(), self.nav.end(node))
            .into_iter()
            .next()
        {
            if comment.kind == Kind::SingleLineCommentTrivia {
                self.push_comment(result, comment.loc.pos(), comment.loc.end());
            }
        }

        if !self.node_contains_position(node) {
            return;
        }

        // Add a stop for a multi-line function body before the block is skipped.
        if self.nav.kind(node) == Kind::Block && is_function_like_declaration(self.nav.kind(parent))
        {
            let start = get_start_of_node(self.nav, node, false);
            let end = self.nav.end(node);
            if !self.positions_are_on_same_line(start, end) {
                self.push(result, start, end);
            }
        }

        // Synthesize a stop for `${ ... }`: the `${` and `}` actually belong to
        // sibling template literals, so the span itself has no node.
        if self.nav.kind(parent) == Kind::TemplateSpan {
            let literal = match self.nav.arena().data(parent) {
                NodeData::TemplateSpan(d) => Some(d.literal),
                _ => None,
            };
            if let Some(literal) = literal {
                // The `${` is two characters before the expression start; the `}`
                // is the first character of the span's middle/tail literal.
                let span_start = self.nav.pos(node) - 2;
                let span_end = get_start_of_node(self.nav, literal, false) + 1;
                let text_len = self.nav.text().len() as i32;
                if span_start >= 0 && span_end <= text_len && span_start < span_end {
                    self.push(result, span_start, span_end);
                }
            }
        }

        if !self.should_skip_node(node, parent) {
            let start = get_start_of_node(self.nav, node, false);
            let end = self.nav.end(node);
            self.push(result, start, end);

            // String literals should have a stop both inside and outside their
            // quotes. (Templates likewise get an inner-content stop.)
            if matches!(
                self.nav.kind(node),
                Kind::StringLiteral
                    | Kind::TemplateExpression
                    | Kind::NoSubstitutionTemplateLiteral
            ) {
                // Only add the inner-content stop when there is content (guards
                // unterminated literals).
                if start + 1 < end - 1 {
                    self.push(result, start + 1, end - 1);
                }
            }
        }

        *next = Some(node);
    }

    /// The `VisitNodes` hook: for each child node list of `node`, push a stop for
    /// the span from the first element's start to the last element's end (Go
    /// skips this when `node` is a `VariableDeclarationList` or a
    /// `TemplateExpression`).
    ///
    /// Side effects: may synthesize tokens in `nav`'s side store.
    // Go: internal/ls/selectionranges.go:getSmartSelectionRange (visitNodes closure)
    fn push_child_list_spans(&self, node: NodeId, result: &mut SelectionRange) {
        if matches!(
            self.nav.kind(node),
            Kind::VariableDeclarationList | Kind::TemplateExpression
        ) {
            return;
        }
        let mut spans = Vec::new();
        for_each_child_list(self.nav, node, &mut |first, last| spans.push((first, last)));
        for (first, last) in spans {
            let start = get_start_of_node(self.nav, first, false);
            let end = self.nav.end(last);
            if start <= self.pos && self.pos < end {
                self.push(result, start, end);
            }
        }
    }

    /// Pushes the stop `[start, end)` onto `result` as a new innermost selection
    /// range (whose parent is the previous `result`), unless the span is empty,
    /// does not contain `pos` (inclusive of both ends), or equals the current
    /// innermost range (Go's dedup of an equal-range parent).
    ///
    /// Side effects: none beyond mutating `result`.
    // Go: internal/ls/selectionranges.go:getSmartSelectionRange (pushSelectionRange closure)
    fn push(&self, result: &mut SelectionRange, start: i32, end: i32) {
        if start == end {
            return;
        }
        if !(start <= self.pos && self.pos <= end) {
            return;
        }
        let lsp_range = self
            .converters
            .to_lsp_range(self.script, TextRange::new(start, end));
        if result.range == lsp_range {
            return;
        }
        let parent = std::mem::replace(
            result,
            SelectionRange {
                range: lsp_range,
                parent: None,
            },
        );
        result.parent = Some(Box::new(parent));
    }

    /// Pushes a single-line comment's two stops onto `result`: the whole comment
    /// `[start, end)`, then its content after the leading `//`.
    ///
    /// Side effects: none beyond mutating `result`.
    // Go: internal/ls/selectionranges.go:getSmartSelectionRange (pushSelectionCommentRange closure)
    fn push_comment(&self, result: &mut SelectionRange, start: i32, end: i32) {
        self.push(result, start, end);
        let text = self.nav.text().as_bytes();
        let mut comment_pos = start;
        while comment_pos < end
            && (comment_pos as usize) < text.len()
            && text[comment_pos as usize] == b'/'
        {
            comment_pos += 1;
        }
        self.push(result, comment_pos, end);
    }

    /// Reports whether `node`'s `[token-start, end)` span contains `pos` (token
    /// start counts leading JSDoc; the end is exclusive).
    ///
    /// Side effects: none (pure read).
    // Go: internal/ls/selectionranges.go:getSmartSelectionRange (nodeContainsPosition closure)
    fn node_contains_position(&self, node: NodeId) -> bool {
        let start = get_start_of_node(self.nav, node, true);
        let end = self.nav.end(node);
        start <= self.pos && self.pos < end
    }

    /// Reports whether the two byte offsets lie on the same source line.
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/selectionranges.go:getSmartSelectionRange (positionsAreOnSameLine closure)
    fn positions_are_on_same_line(&self, pos1: i32, pos2: i32) -> bool {
        if pos1 == pos2 {
            return true;
        }
        let line1 = self
            .converters
            .position_to_line_and_character(self.script, TextPos(pos1))
            .line;
        let line2 = self
            .converters
            .position_to_line_and_character(self.script, TextPos(pos2))
            .line;
        line1 == line2
    }

    /// Reports whether `node` should not contribute its own range stop: blocks
    /// (a function body adds its own multi-line stop earlier), template
    /// head/middle/tail and spans, a `VariableDeclarationList` under a
    /// `VariableStatement`, a lone `VariableDeclaration` under a single-
    /// declaration list, and the inert JSDoc type containers.
    ///
    /// Side effects: none (pure read).
    // Go: internal/ls/selectionranges.go:getSmartSelectionRange (shouldSkipNode closure)
    fn should_skip_node(&self, node: NodeId, parent: NodeId) -> bool {
        let kind = self.nav.kind(node);
        if kind == Kind::Block {
            return true;
        }
        if matches!(
            kind,
            Kind::TemplateSpan | Kind::TemplateHead | Kind::TemplateTail
        ) {
            return true;
        }
        if kind == Kind::VariableDeclarationList && self.nav.kind(parent) == Kind::VariableStatement
        {
            return true;
        }
        // Skip lone variable declarations (a single-declaration list).
        if kind == Kind::VariableDeclaration
            && self.nav.kind(parent) == Kind::VariableDeclarationList
        {
            if let NodeData::VariableDeclarationList(d) = self.nav.arena().data(parent) {
                if d.declarations.nodes.len() == 1 {
                    return true;
                }
            }
        }
        // DEFER(phase-3): JSDoc type containers (inert — no reparsed JSDoc).
        // blocked-by: JSDoc reparser (tsgo_parser).
        matches!(
            kind,
            Kind::JSDocTypeExpression | Kind::JSDocSignature | Kind::JSDocTypeLiteral
        )
    }
}

/// Reports whether `kind` is a function-like *declaration* (one that can carry a
/// block body): the parent kinds for which a multi-line body block gets its own
/// selection-range stop.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsFunctionLikeDeclaration
fn is_function_like_declaration(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::FunctionDeclaration
            | Kind::MethodDeclaration
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::Constructor
            | Kind::FunctionExpression
            | Kind::ArrowFunction
    )
}

/// Emits `(first, last)` for a node list if it is non-empty.
fn emit_list(list: &NodeList, f: &mut dyn FnMut(NodeId, NodeId)) {
    if let (Some(&first), Some(&last)) = (list.nodes.first(), list.nodes.last()) {
        f(first, last);
    }
}

/// Emits `(first, last)` for an optional node list if present and non-empty.
fn emit_opt_list(list: &Option<NodeList>, f: &mut dyn FnMut(NodeId, NodeId)) {
    if let Some(l) = list {
        emit_list(l, f);
    }
}

/// Invokes `f(first, last)` with the first and last element of each non-empty
/// child node list of `node`, in source (field) order.
///
/// This is the shared-borrow, list-aware counterpart of
/// [`for_each_child`](tsgo_ast::NodeArena::for_each_child): it mirrors that
/// method's structure but emits only the node-list children (the ones Go's
/// `VisitEachChild` routes through `v.visitNodes` / `v.visitParameters` /
/// `v.visitTopLevelStatements`, all of which fall through to the `VisitNodes`
/// hook). Modifier lists are intentionally omitted: Go routes them through
/// `v.visitModifiers`, which does not call the `VisitNodes` hook.
///
/// Side effects: none beyond invoking `f`.
// Go: internal/ast/ast.go:Node.ForEachChild (the list/opt_list children only)
fn for_each_child_list(nav: &NavSourceFile<'_>, node: NodeId, f: &mut dyn FnMut(NodeId, NodeId)) {
    match nav.arena().data(node) {
        NodeData::CallExpression(d) => {
            emit_opt_list(&d.type_arguments, f);
            emit_list(&d.arguments, f);
        }
        NodeData::NewExpression(d) => {
            emit_opt_list(&d.type_arguments, f);
            emit_opt_list(&d.arguments, f);
        }
        NodeData::TaggedTemplateExpression(d) => emit_opt_list(&d.type_arguments, f),
        NodeData::TemplateExpression(d) => emit_list(&d.template_spans, f),
        NodeData::TemplateLiteralType(d) => emit_list(&d.template_spans, f),
        NodeData::ImportAttributes(d) => emit_list(&d.attributes, f),
        NodeData::JsxElement(d) | NodeData::JsxFragment(d) => emit_list(&d.children, f),
        NodeData::JsxOpeningElement(d) | NodeData::JsxSelfClosingElement(d) => {
            emit_opt_list(&d.type_arguments, f)
        }
        NodeData::JsxAttributes(d) => emit_list(&d.list, f),
        NodeData::ArrayLiteralExpression(d)
        | NodeData::Block(d)
        | NodeData::ObjectLiteralExpression(d) => emit_list(&d.list, f),
        NodeData::SyntaxList(d) => emit_list(&d.list, f),
        NodeData::VariableDeclarationList(d) => emit_list(&d.declarations, f),
        NodeData::ObjectBindingPattern(d) | NodeData::ArrayBindingPattern(d) => {
            emit_list(&d.elements, f)
        }
        NodeData::ArrowFunction(d) => {
            emit_opt_list(&d.type_parameters, f);
            emit_list(&d.parameters, f);
        }
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => {
            emit_opt_list(&d.type_parameters, f);
            emit_list(&d.parameters, f);
        }
        NodeData::MethodDeclaration(d) => {
            emit_opt_list(&d.type_parameters, f);
            emit_list(&d.parameters, f);
        }
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            emit_opt_list(&d.type_parameters, f);
            emit_list(&d.parameters, f);
        }
        NodeData::ConstructorDeclaration(d) => {
            emit_opt_list(&d.type_parameters, f);
            emit_list(&d.parameters, f);
        }
        NodeData::IndexSignatureDeclaration(d) => emit_list(&d.parameters, f),
        NodeData::MethodSignature(d) => {
            emit_opt_list(&d.type_parameters, f);
            emit_list(&d.parameters, f);
        }
        NodeData::CallSignature(d) | NodeData::ConstructSignature(d) => {
            emit_opt_list(&d.type_parameters, f);
            emit_list(&d.parameters, f);
        }
        NodeData::FunctionType(d) | NodeData::ConstructorType(d) => {
            emit_opt_list(&d.type_parameters, f);
            emit_list(&d.parameters, f);
        }
        NodeData::ClassDeclaration(d) | NodeData::ClassExpression(d) => {
            emit_opt_list(&d.type_parameters, f);
            emit_opt_list(&d.heritage_clauses, f);
            emit_list(&d.members, f);
        }
        NodeData::InterfaceDeclaration(d) => {
            emit_opt_list(&d.type_parameters, f);
            emit_opt_list(&d.heritage_clauses, f);
            emit_list(&d.members, f);
        }
        NodeData::TypeAliasDeclaration(d) => emit_opt_list(&d.type_parameters, f),
        NodeData::HeritageClause(d) => emit_list(&d.types, f),
        NodeData::ExpressionWithTypeArguments(d) => emit_opt_list(&d.type_arguments, f),
        NodeData::EnumDeclaration(d) => emit_list(&d.members, f),
        NodeData::TypeLiteral(d) => emit_list(&d.members, f),
        NodeData::MappedType(d) => emit_list(&d.members, f),
        NodeData::ModuleBlock(d) => emit_list(&d.statements, f),
        NodeData::NamedImports(d) | NodeData::NamedExports(d) => emit_list(&d.elements, f),
        NodeData::CaseBlock(d) => emit_list(&d.clauses, f),
        NodeData::CaseOrDefaultClause(d) => emit_list(&d.statements, f),
        NodeData::TypeReference(d) => emit_opt_list(&d.type_arguments, f),
        NodeData::UnionType(d) | NodeData::IntersectionType(d) => emit_list(&d.types, f),
        NodeData::TupleType(d) => emit_list(&d.types, f),
        NodeData::TypeQuery(d) => emit_opt_list(&d.type_arguments, f),
        NodeData::ImportType(d) => emit_opt_list(&d.type_arguments, f),
        NodeData::SourceFile(d) => emit_list(&d.statements, f),
        _ => {}
    }
}

#[cfg(test)]
#[path = "selectionranges_test.rs"]
mod tests;
