//! Port of Go `internal/ls/folding.go`: the folding-ranges / outlining-spans
//! feature (`textDocument/foldingRange`).
//!
//! Go's `ProvideFoldingRange` walks the parsed source file and emits one
//! `lsproto.FoldingRange` per foldable construct: the body of every block
//! (`{ }`), object / array literal, class / interface / enum / namespace body,
//! `switch` case block, and the leading-comment groups (a multi-line `/* */`
//! comment, or two-or-more consecutive `//` comments). The ranges are sorted by
//! `(startLine, startCharacter)`.
//!
//! # Reachable subset
//!
//! [`LanguageService::provide_folding_ranges`] is purely syntactic: it parses no
//! types and needs no checker, only the program's already-parsed source file
//! (its [`NodeArena`](tsgo_ast::NodeArena) + root). It ports
//! `addNodeOutliningSpans` / `visitNode` / `getOutliningSpanForNode` for the
//! brace/bracket-delimited constructs (blocks — function, attached, and
//! standalone —, module blocks, classes/interfaces/enums, case blocks, type
//! literals, object/array literals, binding patterns, and `case`/`default`
//! clause statement lists) plus the leading-comment folds.
//!
//! DEFER(phase-7-ls): the `//#region`/`//#endregion` named regions
//! (`addRegionOutliningSpans`), the import-group fold (the consecutive
//! `IsAnyImportSyntax` run in `addNodeOutliningSpans`), the
//! `lineFoldingOnly`/`collapsedText` client-capability adjustments
//! (`adjustFoldingEnd` / `supportsCollapsedText`), and the JSX / template-literal
//! / call-expression / parenthesized-expression / named-imports-or-exports
//! spans.
//! blocked-by: the `GetClientCapabilities` capability surface (region collapse
//! text + `lineFoldingOnly`), and the JSX / template span helpers.

use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId, NodeList};
use tsgo_astnav::{get_start_of_node, NavSourceFile};
use tsgo_core::text::{TextPos, TextRange};
use tsgo_lsproto::{FoldingRange, FoldingRangeKind};
use tsgo_scanner::{compute_line_of_position, get_leading_comment_ranges};

use crate::languageservice::LanguageService;

/// Go's `addNodeOutliningSpans` depth budget.
const DEPTH_REMAINING_START: i32 = 40;

impl LanguageService {
    /// Returns the folding ranges for `file_name`: one [`FoldingRange`] per
    /// foldable construct (block / object / array / class / interface / enum /
    /// namespace body, `switch` case block, and leading-comment group), sorted
    /// by `(start_line, start_character)`.
    ///
    /// Empty when there is no such file.
    ///
    /// Mirrors the reachable subset of Go's `ProvideFoldingRange` →
    /// `addNodeOutliningSpans`: walk the AST, collect each foldable node's span,
    /// and convert it to a UTF-16 [`FoldingRange`] via the project
    /// [`Converters`](tsgo_ls_lsconv::Converters).
    ///
    /// Side effects: none (reads the already-parsed file; no binding/checking).
    // Go: internal/ls/folding.go:LanguageService.ProvideFoldingRange
    pub fn provide_folding_ranges(&self, file_name: &str) -> Vec<FoldingRange> {
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
        let spans = collect_outlining_spans(&nav);

        let converters = self.converters();
        let mut result: Vec<FoldingRange> = spans
            .into_iter()
            .map(|span| {
                let range = converters.to_lsp_range(&script, span.range);
                FoldingRange {
                    start_line: range.start.line,
                    start_character: Some(range.start.character),
                    end_line: range.end.line,
                    end_character: Some(range.end.character),
                    kind: span.kind,
                    collapsed_text: None,
                }
            })
            .collect();
        // Go sorts by StartLine, then StartCharacter.
        result.sort_by(|a, b| {
            a.start_line
                .cmp(&b.start_line)
                .then(a.start_character.cmp(&b.start_character))
        });
        result
    }
}

/// A foldable span: an internal byte range plus an optional folding kind
/// (the reachable subset only sets a kind for comment folds).
///
/// Side effects: none (plain data).
struct OutliningSpan {
    range: TextRange,
    kind: Option<FoldingRangeKind>,
}

/// The traversal state for one folding query: the navigation context and the
/// file's ECMA line starts (for same-line checks), plus the accumulated spans.
///
/// Side effects: none (a borrow + a scratch buffer).
struct FoldingWalk<'a, 'b> {
    nav: &'b NavSourceFile<'a>,
    line_starts: Vec<TextPos>,
    out: Vec<OutliningSpan>,
}

/// Walks the source file's statements (and the EOF token for trailing comments)
/// and collects every foldable span (Go's `addNodeOutliningSpans`).
///
/// Side effects: may synthesize brace/bracket tokens in `nav`'s side store.
// Go: internal/ls/folding.go:LanguageService.addNodeOutliningSpans
fn collect_outlining_spans(nav: &NavSourceFile<'_>) -> Vec<OutliningSpan> {
    let line_starts = tsgo_core::compute_ecma_line_starts(nav.text());
    let mut walk = FoldingWalk {
        nav,
        line_starts,
        out: Vec::new(),
    };
    let root = nav.root();
    // The reachable subset walks every top-level statement (Go also groups
    // consecutive imports into one `imports` fold — DEFER, see module note).
    if let NodeData::SourceFile(data) = nav.arena().data(root) {
        let statements: Vec<NodeId> = data.statements.nodes.clone();
        for stmt in statements {
            walk.visit_node(stmt, DEPTH_REMAINING_START);
        }
        // Visit the EOF token so comments not attached to a statement are folded.
        let eof = data.end_of_file_token;
        walk.visit_node(eof, DEPTH_REMAINING_START);
    }
    walk.out
}

impl FoldingWalk<'_, '_> {
    /// Visits one node: collects its leading-comment folds (for the comment-owner
    /// kinds and for block/class member-list ends), its own outlining span, then
    /// recurses into its children.
    ///
    /// Side effects: may synthesize tokens in `nav`'s side store.
    // Go: internal/ls/folding.go:visitNode
    fn visit_node(&mut self, n: NodeId, depth_remaining: i32) {
        if depth_remaining == 0 {
            return;
        }
        let arena = self.nav.arena();
        if arena.flags(n).contains(NodeFlags::REPARSED) {
            return;
        }
        let kind = self.nav.kind(n);

        if is_comment_owner(kind) {
            self.add_leading_comments_for_node(n);
        }

        // Leading comments of a block / module-block / class-or-interface member
        // list's *end* (Go folds comments trailing the last statement/member).
        match arena.data(n) {
            NodeData::Block(d) => {
                let end = d.list.end();
                self.add_leading_comments_for_pos(end);
            }
            NodeData::ModuleBlock(d) => {
                let end = d.statements.end();
                self.add_leading_comments_for_pos(end);
            }
            NodeData::ClassDeclaration(d)
            | NodeData::ClassExpression(d)
            | NodeData::InterfaceDeclaration(d) => {
                let end = d.members.end();
                self.add_leading_comments_for_pos(end);
            }
            _ => {}
        }

        if let Some(span) = self.get_outlining_span_for_node(n, kind) {
            self.out.push(span);
        }

        let next_depth = depth_remaining - 1;
        let mut children: Vec<NodeId> = Vec::new();
        arena.for_each_child(n, &mut |child| {
            children.push(child);
            false
        });
        for child in children {
            self.visit_node(child, next_depth);
        }
    }

    /// The outlining span for `n`, if its kind is one the reachable subset folds.
    ///
    /// Side effects: may synthesize tokens in `nav`'s side store.
    // Go: internal/ls/folding.go:getOutliningSpanForNode
    fn get_outlining_span_for_node(&self, n: NodeId, kind: Kind) -> Option<OutliningSpan> {
        let arena = self.nav.arena();
        match kind {
            Kind::Block => {
                let parent = arena.parent(n);
                let parent_kind = parent.map(|p| arena.kind(p));
                if matches!(parent_kind, Some(pk) if is_function_like(pk)) {
                    return self.function_span(parent.unwrap(), n);
                }
                match parent_kind {
                    Some(
                        Kind::DoStatement
                        | Kind::ForInStatement
                        | Kind::ForOfStatement
                        | Kind::ForStatement
                        | Kind::IfStatement
                        | Kind::WhileStatement
                        | Kind::WithStatement
                        | Kind::CatchClause,
                    ) => self.span_for_node(n, Kind::OpenBraceToken, true),
                    Some(Kind::TryStatement) => {
                        // The try-block or finally-block: collapse the block.
                        if let Some(span) = self.span_for_node(n, Kind::OpenBraceToken, true) {
                            return Some(span);
                        }
                        Some(self.create_folding_range_from_node(n))
                    }
                    _ => Some(self.create_folding_range_from_node(n)),
                }
            }
            Kind::ModuleBlock => self.span_for_node(n, Kind::OpenBraceToken, true),
            Kind::ClassDeclaration
            | Kind::ClassExpression
            | Kind::InterfaceDeclaration
            | Kind::EnumDeclaration
            | Kind::CaseBlock
            | Kind::TypeLiteral
            | Kind::ObjectBindingPattern => self.span_for_node(n, Kind::OpenBraceToken, true),
            Kind::ObjectLiteralExpression => {
                let use_full_start = !self.parent_is_array_or_call(n);
                self.span_for_node(n, Kind::OpenBraceToken, use_full_start)
            }
            Kind::ArrayLiteralExpression => {
                let use_full_start = !self.parent_is_array_or_call(n);
                self.span_for_node(n, Kind::OpenBracketToken, use_full_start)
            }
            Kind::ArrayBindingPattern => {
                let parent = arena.parent(n);
                let use_full_start =
                    !matches!(parent.map(|p| arena.kind(p)), Some(Kind::BindingElement));
                self.span_for_node(n, Kind::OpenBracketToken, use_full_start)
            }
            Kind::CaseClause | Kind::DefaultClause => match arena.data(n) {
                NodeData::CaseOrDefaultClause(d) => self.span_for_node_array(&d.statements),
                _ => None,
            },
            _ => None,
        }
    }

    /// Reports whether `n`'s parent is an array literal or a call expression (the
    /// `useFullStart` toggle for object/array literal spans).
    fn parent_is_array_or_call(&self, n: NodeId) -> bool {
        let arena = self.nav.arena();
        matches!(
            arena.parent(n).map(|p| arena.kind(p)),
            Some(Kind::ArrayLiteralExpression | Kind::CallExpression)
        )
    }

    /// The fold for a function body block: from the open token (the open paren
    /// when parameters span multiple lines, else the body's open brace) to the
    /// body's close brace, using the open token's full start.
    ///
    /// Side effects: may synthesize tokens in `nav`'s side store.
    // Go: internal/ls/folding.go:functionSpan
    fn function_span(&self, node: NodeId, body: NodeId) -> Option<OutliningSpan> {
        let open_token = self.try_get_function_open_token(node, body);
        let close_token = self.nav.find_child_of_kind(body, Kind::CloseBraceToken);
        match (open_token, close_token) {
            (Some(open), Some(close)) => Some(self.range_between_tokens(open, close, true)),
            _ => None,
        }
    }

    /// The open token of a function fold (Go's `tryGetFunctionOpenToken`).
    ///
    /// Side effects: may synthesize tokens in `nav`'s side store.
    // Go: internal/ls/folding.go:tryGetFunctionOpenToken
    fn try_get_function_open_token(&self, node: NodeId, body: NodeId) -> Option<NodeId> {
        if let Some(params) = parameters_of(self.nav.arena(), node) {
            if self.is_node_array_multi_line(params) {
                if let Some(open_paren) = self.nav.find_child_of_kind(node, Kind::OpenParenToken) {
                    return Some(open_paren);
                }
            }
        }
        self.nav.find_child_of_kind(body, Kind::OpenBraceToken)
    }

    /// Reports whether a parameter list spans more than one source line.
    // Go: internal/ls/folding.go:isNodeArrayMultiLine
    fn is_node_array_multi_line(&self, list: &NodeList) -> bool {
        if list.nodes.is_empty() {
            return false;
        }
        let first = list.nodes[0];
        let last = list.nodes[list.nodes.len() - 1];
        !self.positions_are_on_same_line(self.nav.pos(first), self.nav.end(last))
    }

    /// The fold delimited by the `open`/`close` tokens found in `node`.
    ///
    /// Side effects: may synthesize tokens in `nav`'s side store.
    // Go: internal/ls/folding.go:spanForNode
    fn span_for_node(
        &self,
        node: NodeId,
        open: Kind,
        use_full_start: bool,
    ) -> Option<OutliningSpan> {
        let close = if open == Kind::OpenBraceToken {
            Kind::CloseBraceToken
        } else {
            Kind::CloseBracketToken
        };
        let open_token = self.nav.find_child_of_kind(node, open)?;
        let close_token = self.nav.find_child_of_kind(node, close)?;
        Some(self.range_between_tokens(open_token, close_token, use_full_start))
    }

    /// The fold spanning a (non-empty) statement list (Go's `spanForNodeArray`).
    fn span_for_node_array(&self, statements: &NodeList) -> Option<OutliningSpan> {
        if statements.nodes.is_empty() {
            return None;
        }
        Some(OutliningSpan {
            range: TextRange::new(statements.pos(), statements.end()),
            kind: None,
        })
    }

    /// The fold from `open` to `close`: from `open`'s full start (or its
    /// token-start when `use_full_start` is false) to `close`'s end.
    // Go: internal/ls/folding.go:rangeBetweenTokens
    fn range_between_tokens(
        &self,
        open: NodeId,
        close: NodeId,
        use_full_start: bool,
    ) -> OutliningSpan {
        let start = if use_full_start {
            self.nav.pos(open)
        } else {
            get_start_of_node(self.nav, open, false)
        };
        OutliningSpan {
            range: TextRange::new(start, self.nav.end(close)),
            kind: None,
        }
    }

    /// A fold over the whole node (Go's `createFoldingRange(createLspRangeFromNode)`).
    fn create_folding_range_from_node(&self, n: NodeId) -> OutliningSpan {
        OutliningSpan {
            range: TextRange::new(get_start_of_node(self.nav, n, false), self.nav.end(n)),
            kind: None,
        }
    }

    /// Reports whether `pos1`/`pos2` lie on the same source line.
    // Go: internal/printer/utilities.go:PositionsAreOnSameLine
    fn positions_are_on_same_line(&self, pos1: i32, pos2: i32) -> bool {
        compute_line_of_position(&self.line_starts, pos1)
            == compute_line_of_position(&self.line_starts, pos2)
    }

    /// Adds the leading-comment folds preceding node `n`.
    // Go: internal/ls/folding.go:addOutliningForLeadingCommentsForNode
    fn add_leading_comments_for_node(&mut self, n: NodeId) {
        if self.nav.kind(n) == Kind::JsxText {
            return;
        }
        let pos = self.nav.pos(n);
        self.add_leading_comments_for_pos(pos);
    }

    /// Adds the leading-comment folds at byte `pos`: one per multi-line `/* */`
    /// comment, and one combined fold per run of two-or-more consecutive `//`
    /// comments.
    // Go: internal/ls/folding.go:addOutliningForLeadingCommentsForPos
    fn add_leading_comments_for_pos(&mut self, pos: i32) {
        let text = self.nav.text().to_string();
        let mut first_single = -1;
        let mut last_single_end = -1;
        let mut single_count = 0;

        for comment in get_leading_comment_ranges(&text, pos) {
            let comment_pos = comment.loc.pos();
            let comment_end = comment.loc.end();
            match comment.kind {
                Kind::SingleLineCommentTrivia => {
                    let comment_text = &text[comment_pos as usize..comment_end as usize];
                    // Never fold region delimiters into single-line comment runs.
                    if is_region_delimiter(comment_text) {
                        if let Some(span) = combine_single_line_comments(
                            first_single,
                            last_single_end,
                            single_count,
                        ) {
                            self.out.push(span);
                        }
                        single_count = 0;
                        continue;
                    }
                    if single_count == 0 {
                        first_single = comment_pos;
                    }
                    last_single_end = comment_end;
                    single_count += 1;
                }
                Kind::MultiLineCommentTrivia => {
                    if let Some(span) =
                        combine_single_line_comments(first_single, last_single_end, single_count)
                    {
                        self.out.push(span);
                    }
                    self.out.push(OutliningSpan {
                        range: TextRange::new(comment_pos, comment_end),
                        kind: Some(FoldingRangeKind::comment()),
                    });
                    single_count = 0;
                }
                _ => {}
            }
        }
        if let Some(span) =
            combine_single_line_comments(first_single, last_single_end, single_count)
        {
            self.out.push(span);
        }
    }
}

/// Combines a run of consecutive single-line comments into one comment fold,
/// but only when there are two or more (Go's `combineAndAddMultipleSingleLineComments`).
fn combine_single_line_comments(first: i32, last_end: i32, count: i32) -> Option<OutliningSpan> {
    if count > 1 {
        Some(OutliningSpan {
            range: TextRange::new(first, last_end),
            kind: Some(FoldingRangeKind::comment()),
        })
    } else {
        None
    }
}

/// Reports whether a single-line comment is a `//#region` / `//#endregion`
/// delimiter (so it never folds into a normal comment run).
// Go: internal/ls/folding.go:parseRegionDelimiter
fn is_region_delimiter(comment_text: &str) -> bool {
    let line = comment_text.trim_start();
    let Some(rest) = line.strip_prefix("//") else {
        return false;
    };
    let rest = rest.trim().trim_end_matches('\r');
    let Some(rest) = rest.strip_prefix('#') else {
        return false;
    };
    let rest = rest.strip_prefix("end").unwrap_or(rest);
    rest.starts_with("region")
}

/// Reports whether `kind` owns its leading comments for folding (Go's
/// comment-owner predicate in `visitNode`): a non-binary declaration, a variable
/// statement, a `return` statement, a call/new expression, or the EOF token.
// Go: internal/ls/folding.go:visitNode (leading-comment guard)
fn is_comment_owner(kind: Kind) -> bool {
    (kind != Kind::BinaryExpression && is_declaration_kind(kind))
        || kind == Kind::VariableStatement
        || kind == Kind::ReturnStatement
        || kind == Kind::CallExpression
        || kind == Kind::NewExpression
        || kind == Kind::EndOfFile
}

/// Reports whether `kind` is function-like (Go's `ast.IsFunctionLike`, reachable
/// subset: the kinds that can be a block body's parent or carry parameters).
// Go: internal/ast/utilities.go:IsFunctionLike
fn is_function_like(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::FunctionDeclaration
            | Kind::FunctionExpression
            | Kind::ArrowFunction
            | Kind::MethodDeclaration
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::MethodSignature
            | Kind::CallSignature
            | Kind::ConstructSignature
            | Kind::IndexSignature
            | Kind::FunctionType
            | Kind::ConstructorType
    )
}

/// Reports whether `kind` is a declaration kind (reachable subset of Go's
/// `ast.IsDeclarationKind`), for the comment-owner guard.
// Go: internal/ast/utilities.go:IsDeclarationKind
fn is_declaration_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::FunctionDeclaration
            | Kind::FunctionExpression
            | Kind::ClassDeclaration
            | Kind::ClassExpression
            | Kind::InterfaceDeclaration
            | Kind::TypeAliasDeclaration
            | Kind::EnumDeclaration
            | Kind::EnumMember
            | Kind::ModuleDeclaration
            | Kind::MethodDeclaration
            | Kind::MethodSignature
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::PropertyDeclaration
            | Kind::PropertySignature
            | Kind::PropertyAssignment
            | Kind::ShorthandPropertyAssignment
            | Kind::VariableDeclaration
            | Kind::BindingElement
            | Kind::Parameter
            | Kind::TypeParameter
            | Kind::ImportEqualsDeclaration
            | Kind::ImportClause
            | Kind::NamespaceImport
            | Kind::ImportSpecifier
            | Kind::ExportSpecifier
            | Kind::NamespaceExportDeclaration
            | Kind::NamespaceExport
    )
}

/// The parameter list of a function-like node, if it has one.
// Go: internal/ast/utilities.go:Node.Parameters (reachable subset)
fn parameters_of(arena: &NodeArena, node: NodeId) -> Option<&NodeList> {
    match arena.data(node) {
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => Some(&d.parameters),
        NodeData::MethodDeclaration(d) => Some(&d.parameters),
        NodeData::ConstructorDeclaration(d) => Some(&d.parameters),
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => {
            Some(&d.parameters)
        }
        NodeData::ArrowFunction(d) => Some(&d.parameters),
        NodeData::MethodSignature(d) => Some(&d.parameters),
        _ => None,
    }
}

#[cfg(test)]
#[path = "folding_test.rs"]
mod tests;
