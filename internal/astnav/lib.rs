//! `tsgo_astnav` — 1:1 Rust port of Go `internal/astnav` (position-based AST
//! navigation for the language service).
//!
//! Go upstream: `internal/astnav/tokens.go`. The package answers "given a byte
//! position, which token/node is there?" via [`get_token_at_position`],
//! [`get_touching_token`], [`get_touching_property_name`],
//! [`find_preceding_token`]/[`find_preceding_token_ex`], [`find_next_token`],
//! and [`find_child_of_kind`]. It walks the AST top-down and, when the position
//! falls on punctuation/trivia that the parser does not keep as a standalone
//! node, re-scans from a known boundary to synthesize the token and caches it on
//! the [`SourceFile`] (so repeated queries return the same node id).
//!
//! # Resolving the prior `BLOCKED` state without editing scanner/ast
//!
//! A previous wave reported this crate `BLOCKED` because `tsgo_scanner`
//! explicitly defers `GetScannerForSourceFile`/`GetTokenPosOfNode` and the
//! `tsgo_ast` `SourceFile` runtime has no token cache. Both are resolved here
//! *locally*:
//!
//! - [`SourceFile`] is this crate's navigation context: it owns the parsed
//!   [`NodeArena`], the root id, the source text, and the synthesized-token
//!   cache that backs the pointer-equality guarantee. It replaces Go's
//!   `*ast.SourceFile` (which carried both the text and the cache).
//! - A scanner is instantiated and driven over the source text directly (see
//!   `get_scanner_for_source_file`), exactly as Go's `getTokenPosOfNode` drives
//!   one internally, instead of depending on a scanner helper.
//! - Child/JSDoc walks use `tsgo_ast`'s `NodeArena::for_each_child`; the Go
//!   `VisitEachChild` hook split between single nodes and node lists is
//!   flattened into a single ordered child stream (see the divergence notes on
//!   [`visit_each_child_and_jsdoc`]).
//!
//! # JSDoc
//!
//! The parser has not ported the JSDoc reparser, so no node carries cached
//! JSDoc and the tree contains no JSDoc-kind nodes. The JSDoc-dependent branches
//! are ported structurally (so the shape matches Go) but are inert in practice;
//! they are flagged with `// DEFER(phase-3)` / `// blocked-by: JSDoc reparser`.

use tsgo_ast::utilities::{is_keyword_kind, is_token_kind, node_is_missing};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId};
use tsgo_core::languagevariant::LanguageVariant;
use tsgo_core::text::TextRange;
use tsgo_scanner::{skip_trivia_ex, Scanner, SkipTriviaOptions};

use std::collections::HashMap;

// Note: Go's `comparisonLessThan`/`EqualTo`/`GreaterThan` constants drove the
// per-node-list binary searches. This port flattens lists into a single ordered
// child stream and scans them linearly (see `visit_each_child_and_jsdoc`), so the
// three-way comparator constants are not needed.

/// Cache key for synthesized tokens, mirroring Go's `TokenCacheKey{parent, loc}`.
///
/// Side effects: none (pure value type).
// Go: internal/ast/ast.go:TokenCacheKey
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TokenCacheKey {
    parent: NodeId,
    pos: i32,
    end: i32,
}

/// A navigation context over a parsed source file.
///
/// Bundles the owning [`NodeArena`], the root `SourceFile` node id, the source
/// text, and the synthesized-token cache. This stands in for Go's
/// `*ast.SourceFile`: the Rust `tsgo_ast` `SourceFile` node stores neither the
/// text nor a token cache, so this crate supplies both. Holding it by `&mut`
/// lets the navigation functions synthesize and memoize tokens, which is what
/// makes repeated queries at the same position return the same node id.
///
/// # Examples
/// ```
/// use tsgo_astnav::SourceFile;
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// let text = "const x = 1;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// assert_eq!(sf.kind(sf.root()), tsgo_ast::Kind::SourceFile);
/// ```
///
/// Side effects: methods taking `&mut self` may push synthesized token nodes and
/// populate the token cache.
pub struct SourceFile {
    arena: NodeArena,
    root: NodeId,
    text: String,
    language_variant: LanguageVariant,
    eof_token: NodeId,
    token_cache: HashMap<TokenCacheKey, NodeId>,
}

impl SourceFile {
    /// Builds a navigation context from a parse result's `arena`, `root` source
    /// file id, and the original source `text`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_astnav::SourceFile;
    /// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
    /// use tsgo_core::scriptkind::ScriptKind;
    /// let text = "a;";
    /// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    /// let sf = SourceFile::new(r.arena, r.source_file, text.to_string());
    /// assert_eq!(sf.text(), "a;");
    /// ```
    ///
    /// Side effects: none (takes ownership of the arena and text).
    pub fn new(arena: NodeArena, root: NodeId, text: String) -> SourceFile {
        let (language_variant, eof_token) = match arena.data(root) {
            NodeData::SourceFile(d) => (d.language_variant, d.end_of_file_token),
            _ => panic!("SourceFile::new expects a SourceFile node as root"),
        };
        SourceFile {
            arena,
            root,
            text,
            language_variant,
            eof_token,
            token_cache: HashMap::new(),
        }
    }

    /// Returns the root `SourceFile` node id (mirrors Go's `file.AsNode()`).
    ///
    /// # Examples
    /// ```
    /// # use tsgo_astnav::SourceFile;
    /// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
    /// # use tsgo_core::scriptkind::ScriptKind;
    /// let r = parse_source_file(SourceFileParseOptions::default(), "x;", ScriptKind::Ts);
    /// let sf = SourceFile::new(r.arena, r.source_file, "x;".to_string());
    /// assert_eq!(sf.kind(sf.root()), tsgo_ast::Kind::SourceFile);
    /// ```
    ///
    /// Side effects: none (pure).
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Returns the source text of the file.
    ///
    /// # Examples
    /// ```
    /// # use tsgo_astnav::SourceFile;
    /// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
    /// # use tsgo_core::scriptkind::ScriptKind;
    /// let r = parse_source_file(SourceFileParseOptions::default(), "x;", ScriptKind::Ts);
    /// let sf = SourceFile::new(r.arena, r.source_file, "x;".to_string());
    /// assert_eq!(sf.text(), "x;");
    /// ```
    ///
    /// Side effects: none (pure).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the owning arena (for callers that need to inspect node payloads).
    ///
    /// # Examples
    /// ```
    /// # use tsgo_astnav::SourceFile;
    /// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
    /// # use tsgo_core::scriptkind::ScriptKind;
    /// let r = parse_source_file(SourceFileParseOptions::default(), "x;", ScriptKind::Ts);
    /// let sf = SourceFile::new(r.arena, r.source_file, "x;".to_string());
    /// assert_eq!(sf.arena().kind(sf.root()), tsgo_ast::Kind::SourceFile);
    /// ```
    ///
    /// Side effects: none (pure).
    pub fn arena(&self) -> &NodeArena {
        &self.arena
    }

    /// Returns the syntax kind of node `id`.
    ///
    /// # Examples
    /// ```
    /// # use tsgo_astnav::SourceFile;
    /// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
    /// # use tsgo_core::scriptkind::ScriptKind;
    /// let r = parse_source_file(SourceFileParseOptions::default(), "x;", ScriptKind::Ts);
    /// let sf = SourceFile::new(r.arena, r.source_file, "x;".to_string());
    /// assert_eq!(sf.kind(sf.root()), tsgo_ast::Kind::SourceFile);
    /// ```
    ///
    /// Side effects: none (pure).
    pub fn kind(&self, id: NodeId) -> Kind {
        self.arena.kind(id)
    }

    /// Returns the start offset (full start, including leading trivia) of `id`.
    ///
    /// # Examples
    /// ```
    /// # use tsgo_astnav::SourceFile;
    /// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
    /// # use tsgo_core::scriptkind::ScriptKind;
    /// let r = parse_source_file(SourceFileParseOptions::default(), "x;", ScriptKind::Ts);
    /// let sf = SourceFile::new(r.arena, r.source_file, "x;".to_string());
    /// assert_eq!(sf.pos(sf.root()), 0);
    /// ```
    ///
    /// Side effects: none (pure).
    pub fn pos(&self, id: NodeId) -> i32 {
        self.arena.loc(id).pos()
    }

    /// Returns the end offset (exclusive) of node `id`.
    ///
    /// # Examples
    /// ```
    /// # use tsgo_astnav::SourceFile;
    /// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
    /// # use tsgo_core::scriptkind::ScriptKind;
    /// let r = parse_source_file(SourceFileParseOptions::default(), "x;", ScriptKind::Ts);
    /// let sf = SourceFile::new(r.arena, r.source_file, "x;".to_string());
    /// assert_eq!(sf.end(sf.root()), 2);
    /// ```
    ///
    /// Side effects: none (pure).
    pub fn end(&self, id: NodeId) -> i32 {
        self.arena.loc(id).end()
    }

    /// Returns the per-node flags of `id`.
    ///
    /// Side effects: none (pure).
    fn flags(&self, id: NodeId) -> NodeFlags {
        self.arena.flags(id)
    }

    /// Returns the cached JSDoc nodes of `id`.
    ///
    /// Always empty: the parser has not ported the JSDoc reparser, so no node
    /// carries cached JSDoc. Kept so the navigation code mirrors Go's structure.
    ///
    /// Side effects: none (pure).
    // DEFER(phase-3): return real JSDoc once the parser reparses it.
    // blocked-by: JSDoc reparser (tsgo_parser).
    fn node_jsdoc(&self, _id: NodeId) -> &[NodeId] {
        &[]
    }

    /// Returns a previously synthesized token for `(kind, pos, end, parent)`, or
    /// creates, caches, and returns a fresh one.
    ///
    /// The cache key is `(parent, pos, end)` (matching Go's `TokenCacheKey`), so
    /// two queries that synthesize the same token return the same node id. This
    /// is the pointer-equality guarantee that Go gets from the source file's
    /// token cache.
    ///
    /// Side effects: on a miss, pushes a new token node and records it.
    // Go: internal/ast/ast.go:SourceFile.GetOrCreateToken
    fn get_or_create_token(&mut self, kind: Kind, pos: i32, end: i32, parent: NodeId) -> NodeId {
        let key = TokenCacheKey { parent, pos, end };
        if let Some(&id) = self.token_cache.get(&key) {
            return id;
        }
        let id = self.arena.new_token(kind);
        self.arena.set_loc(id, TextRange::new(pos, end));
        self.arena.set_parent(id, Some(parent));
        self.token_cache.insert(key, id);
        id
    }
}

/// A filter applied to a candidate "preceding token at the end position".
///
/// Go passes a `func(*ast.Node) bool` callback to `getTokenAtPosition`; only
/// `GetTouchingPropertyName` supplies a non-nil one. Modeled as a small enum to
/// avoid a closure borrowing the `SourceFile` that is otherwise held by `&mut`.
#[derive(Clone, Copy)]
enum PrecedingTokenFilter {
    /// No filter (Go `nil`): used by `GetTokenAtPosition`/`GetTouchingToken`.
    None,
    /// Accept property-name literals, keywords, and private identifiers.
    PropertyName,
}

impl PrecedingTokenFilter {
    /// Reports whether this is the no-op filter (Go's `nil` callback).
    fn is_none(self) -> bool {
        matches!(self, PrecedingTokenFilter::None)
    }

    /// Applies the filter to node `id` (only meaningful for `PropertyName`).
    // Go: internal/astnav/tokens.go:GetTouchingPropertyName (inline callback)
    fn applies(self, file: &SourceFile, id: NodeId) -> bool {
        match self {
            PrecedingTokenFilter::None => false,
            PrecedingTokenFilter::PropertyName => {
                is_property_name_literal(file, id)
                    || is_keyword_kind(file.kind(id))
                    || is_private_identifier(file, id)
            }
        }
    }
}

/// Reports whether `kind` is a JSDoc node kind.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsJSDocKind
fn is_jsdoc_kind(kind: Kind) -> bool {
    Kind::FIRST_J_S_DOC_NODE <= kind && kind <= Kind::LAST_J_S_DOC_NODE
}

/// Reports whether node `id` is a JSDoc node.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsJSDocNode
fn is_jsdoc_node(file: &SourceFile, id: NodeId) -> bool {
    is_jsdoc_kind(file.kind(id))
}

/// Reports whether node `id` is a `JsxText` consisting only of whitespace.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsWhitespaceOnlyJsxText
fn is_whitespace_only_jsx_text(file: &SourceFile, id: NodeId) -> bool {
    matches!(
        file.arena.data(id),
        NodeData::JsxText(d) if d.contains_only_trivia_white_spaces
    )
}

/// Reports whether node `id` is a token that is not whitespace-only JSX text.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsNonWhitespaceToken
fn is_non_whitespace_token(file: &SourceFile, id: NodeId) -> bool {
    is_token_kind(file.kind(id)) && !is_whitespace_only_jsx_text(file, id)
}

/// Reports whether node `id` is a JSX child node kind.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsJsxChild
fn is_jsx_child(file: &SourceFile, id: NodeId) -> bool {
    matches!(
        file.kind(id),
        Kind::JsxElement
            | Kind::JsxExpression
            | Kind::JsxSelfClosingElement
            | Kind::JsxText
            | Kind::JsxFragment
    )
}

/// Reports whether node `id` is an identifier/string/number/no-substitution
/// template literal usable as a property name.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsPropertyNameLiteral
fn is_property_name_literal(file: &SourceFile, id: NodeId) -> bool {
    matches!(
        file.kind(id),
        Kind::Identifier
            | Kind::StringLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::NumericLiteral
    )
}

/// Reports whether node `id` is a private identifier (`#foo`).
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsPrivateIdentifier
fn is_private_identifier(file: &SourceFile, id: NodeId) -> bool {
    file.kind(id) == Kind::PrivateIdentifier
}

/// Returns the last non-reparsed node of `nodes`, or `None`.
///
/// Only reached via [`find_rightmost_node`], which (like its Go counterpart) is
/// itself unused by the navigation entry points.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:FindLastVisibleNode
#[allow(dead_code)]
fn find_last_visible_node(file: &SourceFile, nodes: &[NodeId]) -> Option<NodeId> {
    let mut from_end = 1usize;
    while from_end <= nodes.len()
        && file
            .flags(nodes[nodes.len() - from_end])
            .contains(NodeFlags::REPARSED)
    {
        from_end += 1;
    }
    if from_end <= nodes.len() {
        Some(nodes[nodes.len() - from_end])
    } else {
        None
    }
}

/// Reports whether the algorithm should treat `node` as having no navigable
/// tokens of its own (JSDoc containers, tags, and links).
///
/// Side effects: none (pure).
// Go: internal/astnav/tokens.go:shouldSkipChild
fn should_skip_child(file: &SourceFile, id: NodeId) -> bool {
    let kind = file.kind(id);
    kind == Kind::JSDoc
        || kind == Kind::JSDocText
        || kind == Kind::JSDocTypeLiteral
        || kind == Kind::JSDocSignature
        || matches!(
            kind,
            Kind::JSDocLink | Kind::JSDocLinkCode | Kind::JSDocLinkPlain
        )
        || (Kind::FIRST_J_S_DOC_TAG_NODE <= kind && kind <= Kind::LAST_J_S_DOC_TAG_NODE)
}

/// Creates a scanner positioned at `pos` over `text`, having scanned its first
/// token (mirrors Go's `scanner.GetScannerForSourceFile`).
///
/// Side effects: allocates a scanner that owns a copy of `text`.
// Go: internal/scanner/scanner.go:GetScannerForSourceFile
fn get_scanner_for_source_file(text: &str, language_variant: LanguageVariant, pos: i32) -> Scanner {
    let mut s = Scanner::new();
    // PERF(port): Go shares `sourceFile.Text()` by reference; the scanner here
    // owns its text, so each query clones the source. Acceptable for the port.
    s.set_text(text.to_string());
    s.set_language_variant(language_variant);
    s.reset_pos(pos);
    s.scan();
    s
}

/// Reports whether the current `<<` token should be rescanned as JSX text
/// because it sits inside a JSX child.
///
/// Side effects: none (pure).
// Go: internal/astnav/tokens.go:shouldRescanLessThanLessThanToken
fn should_rescan_less_than_less_than_token(
    file: &SourceFile,
    containing_node: NodeId,
    token: Kind,
) -> bool {
    token == Kind::LessThanLessThanToken && is_jsx_child(file, containing_node)
}

/// Returns the current token, rescanning `<<` as JSX text when inside a JSX
/// child.
///
/// Side effects: may advance the scanner via a JSX rescan.
// Go: internal/astnav/tokens.go:scanNavigationToken
fn scan_navigation_token(
    scanner: &mut Scanner,
    file: &SourceFile,
    containing_node: NodeId,
) -> Kind {
    let token = scanner.token();
    if should_rescan_less_than_less_than_token(file, containing_node, token) {
        return scanner.re_scan_jsx_token(true);
    }
    token
}

/// Computes the token start of `id`, skipping leading trivia and honoring JSDoc
/// and `JsxText` quirks (mirrors `scanner.GetTokenPosOfNode`).
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:GetTokenPosOfNode
fn get_token_pos_of_node(file: &SourceFile, id: NodeId, include_jsdoc: bool) -> i32 {
    // Missing (zero-width) nodes keep their position: skipping trivia would jump
    // forward to the next token.
    if node_is_missing(&file.arena, id) {
        return file.pos(id);
    }
    if is_jsdoc_node(file, id) || file.kind(id) == Kind::JsxText {
        // JsxText cannot actually contain comments, even though the scanner
        // would treat `//`/`/*` as comments.
        return skip_trivia_ex(
            &file.text,
            file.pos(id),
            Some(&SkipTriviaOptions {
                stop_at_comments: true,
                ..Default::default()
            }),
        );
    }
    let jsdoc = file.node_jsdoc(id);
    if include_jsdoc && !jsdoc.is_empty() {
        // DEFER(phase-3): start-of-node would jump to the first JSDoc comment.
        // blocked-by: JSDoc reparser (tsgo_parser).
        return get_token_pos_of_node(file, jsdoc[0], false);
    }
    skip_trivia_ex(
        &file.text,
        file.pos(id),
        Some(&SkipTriviaOptions {
            in_jsdoc: file.flags(id).contains(NodeFlags::JSDOC),
            ..Default::default()
        }),
    )
}

/// Returns the position used to compare `node` against the target: the full
/// start when leading trivia is allowed, otherwise the token start.
///
/// Side effects: none (pure).
// Go: internal/astnav/tokens.go:getPosition
fn get_position(file: &SourceFile, id: NodeId, allow_in_leading_trivia: bool) -> i32 {
    if allow_in_leading_trivia {
        file.pos(id)
    } else {
        get_token_pos_of_node(file, id, true)
    }
}

/// Returns the start offset of `node`'s first token (optionally counting leading
/// JSDoc).
///
/// # Examples
/// ```
/// # use tsgo_astnav::{SourceFile, get_start_of_node};
/// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// # use tsgo_core::scriptkind::ScriptKind;
/// let text = "  const x = 1;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// // The statement's leading whitespace is skipped: it starts at `const`.
/// assert_eq!(get_start_of_node(&sf, sf.root(), true), 2);
/// ```
///
/// Side effects: none (pure).
// Go: internal/astnav/tokens.go:GetStartOfNode
pub fn get_start_of_node(file: &SourceFile, node: NodeId, include_jsdoc: bool) -> i32 {
    get_token_pos_of_node(file, node, include_jsdoc)
}

/// Reports whether `node` is a valid "preceding token" container: non-empty
/// width and not whitespace-only JSX text (EOF only when it has JSDoc).
///
/// Side effects: none (pure).
// Go: internal/astnav/tokens.go:isValidPrecedingNode
fn is_valid_preceding_node(file: &SourceFile, id: NodeId) -> bool {
    if file.kind(id) == Kind::EndOfFile {
        return !file.node_jsdoc(id).is_empty();
    }
    let start = get_start_of_node(file, id, false);
    let width = file.end(id) - start;
    !(is_whitespace_only_jsx_text(file, id) || width == 0)
}

/// Visits each child of `node` in source order, JSDoc first.
///
/// This is the port of Go's `VisitEachChildAndJSDoc`. Go drives an
/// `ast.NodeVisitor` whose hooks distinguish single children from node lists (so
/// list children can be binary searched). `tsgo_ast` exposes only
/// `NodeArena::for_each_child`, which yields a flat, in-source-order child
/// stream, so this port collapses the two hooks into one callback and the
/// callers replace the per-list binary search with a linear scan over the same
/// (sorted) children — same result, without the list/single distinction. JSDoc
/// is visited first to match Go, but is always empty here (see crate docs).
///
/// # Examples
/// ```
/// # use tsgo_astnav::{SourceFile, visit_each_child_and_jsdoc};
/// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// # use tsgo_core::scriptkind::ScriptKind;
/// let text = "a + b";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// let mut n = 0;
/// // The source file has one statement plus the end-of-file token.
/// visit_each_child_and_jsdoc(&sf, sf.root(), &mut |_| n += 1);
/// assert_eq!(n, 2);
/// ```
///
/// Side effects: invokes `visit` for each child; no mutation.
// Go: internal/astnav/tokens.go:VisitEachChildAndJSDoc
pub fn visit_each_child_and_jsdoc(file: &SourceFile, node: NodeId, visit: &mut dyn FnMut(NodeId)) {
    for &jsdoc in file.node_jsdoc(node) {
        visit(jsdoc);
    }
    file.arena.for_each_child(node, &mut |c| {
        visit(c);
        false
    });
}

/// Collects the children of `node` (JSDoc first) into a vector.
///
/// Side effects: none beyond allocating the result.
fn collect_children(file: &SourceFile, node: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    visit_each_child_and_jsdoc(file, node, &mut |c| out.push(c));
    out
}

/// Descends to the rightmost visible (non-reparsed) descendant of `node`.
///
/// Unused by the navigation entry points (it is dead code in the Go source as
/// well), but ported for completeness.
///
/// Side effects: none (pure).
// Go: internal/astnav/tokens.go:findRightmostNode
#[allow(dead_code)]
fn find_rightmost_node(file: &SourceFile, node: NodeId) -> NodeId {
    let mut current = node;
    loop {
        let children = collect_children(file, current);
        let next = find_last_visible_node(file, &children);
        match next {
            None => return current,
            Some(n) => current = n,
        }
    }
}

/// Tests `node` against the target `position`, returning `-1`/`0`/`1` for
/// before/contains/after, and recording `node` as `prev_subtree` when its end
/// is exactly at `position` and a preceding-token filter is in effect.
///
/// Side effects: may set `*prev_subtree`.
// Go: internal/astnav/tokens.go:getTokenAtPosition (testNode closure)
fn test_node(
    file: &SourceFile,
    node: NodeId,
    position: i32,
    allow_in_leading_trivia: bool,
    include_preceding: PrecedingTokenFilter,
    prev_subtree: &mut Option<NodeId>,
) -> i32 {
    let kind = file.kind(node);
    let end = file.end(node);
    if kind != Kind::EndOfFile
        && end == position
        && !include_preceding.is_none()
        && !file.flags(node).contains(NodeFlags::REPARSED)
    {
        *prev_subtree = Some(node);
    }

    // A node "contains" the position if position < end, except nodes at the file
    // end treat end as inclusive (there is nowhere else to look). This applies to
    // the EOF token and to JSDoc nodes reaching EOF.
    if end < position
        || (end == position
            && kind != Kind::EndOfFile
            && (!is_jsdoc_kind(kind) || end != file.end(file.eof_token)))
    {
        return -1;
    }
    let node_pos = get_position(file, node, allow_in_leading_trivia);
    if node_pos > position {
        return 1;
    }
    0
}

/// Core of the three public position-to-token entry points.
///
/// Side effects: may synthesize and cache tokens.
// Go: internal/astnav/tokens.go:getTokenAtPosition
fn get_token_at_position_core(
    file: &mut SourceFile,
    position: i32,
    allow_in_leading_trivia: bool,
    include_preceding: PrecedingTokenFilter,
) -> NodeId {
    let mut next: Option<NodeId> = None;
    let mut prev_subtree: Option<NodeId> = None;
    let mut current = file.root;
    // Lower boundary of the node/token that may be returned; eventually the
    // scanner's start position when the scanner is used.
    let mut left = 0i32;
    // First node visited after the one that advanced `left`; bounds the scanner.
    let mut node_after_left: Option<NodeId> = None;

    loop {
        let children = collect_children(file, current);
        for node in children {
            if file.flags(node).contains(NodeFlags::REPARSED) {
                continue;
            }
            if node_after_left.is_none() {
                node_after_left = Some(node);
            }
            if next.is_none() {
                let result = test_node(
                    file,
                    node,
                    position,
                    allow_in_leading_trivia,
                    include_preceding,
                    &mut prev_subtree,
                );
                match result {
                    -1 => {
                        // Do not move `left` into or past JSDoc: a token after the
                        // JSDoc would need to include all its leading trivia.
                        if !is_jsdoc_kind(file.kind(node)) {
                            left = file.end(node);
                        }
                        node_after_left = None;
                    }
                    0 => next = Some(node),
                    _ => {}
                }
            }
        }

        // If `prev_subtree` was set, it ends exactly at the target position. Check
        // whether its rightmost token should be returned per the filter.
        if let Some(ps) = prev_subtree {
            let child = find_preceding_token_ex(file, position, Some(ps), false);
            if let Some(child) = child {
                if file.end(child) == position && include_preceding.applies(file, child) {
                    // Optimization: the filter only ever accepts real AST nodes, so
                    // the scanner is not needed here.
                    return child;
                }
            }
            prev_subtree = None;
        }

        // No child contains the target position: we are as deep as the AST goes.
        // Either we found a token, or we must run the scanner to construct one.
        if next.is_none() {
            let current_kind = file.kind(current);
            if is_token_kind(current_kind) || should_skip_child(file, current) {
                return current;
            }
            let mut scanner = get_scanner_for_source_file(&file.text, file.language_variant, left);
            // Only scan up to the start of the next AST node after the node ending
            // at `left`; otherwise a position between two tokens could scan past
            // the next node before finding a token.
            let mut end = file.end(current);
            if let Some(nal) = node_after_left {
                end = file.pos(nal);
            }
            while left < end {
                let token = scan_navigation_token(&mut scanner, file, current);
                let token_full_start = scanner.token_full_start();
                let token_start = if allow_in_leading_trivia {
                    token_full_start
                } else {
                    scanner.token_start()
                };
                let token_end = scanner.token_end();
                if token_end > end {
                    break;
                }
                if token_start <= position && position < token_end {
                    if token == Kind::Identifier || !is_token_kind(token) {
                        if is_jsdoc_kind(current_kind) {
                            return current;
                        }
                        panic!(
                            "did not expect {} to have {} in its trivia",
                            current_kind, token
                        );
                    }
                    return file.get_or_create_token(token, token_full_start, token_end, current);
                }
                if !include_preceding.is_none() && token_end == position {
                    let prev_token =
                        file.get_or_create_token(token, token_full_start, token_end, current);
                    if include_preceding.applies(file, prev_token) {
                        return prev_token;
                    }
                }
                left = token_end;
                scanner.scan();
            }
            return current;
        }

        current = next.unwrap();
        left = file.pos(current);
        node_after_left = None;
        next = None;
    }
}

/// Returns the token at `position`, counting positions in leading trivia.
///
/// Mirrors TS `getTokenAtPosition`: the result may be a real AST node or a token
/// synthesized from the scanner (and memoized for pointer equality).
///
/// # Examples
/// ```
/// # use tsgo_astnav::{SourceFile, get_token_at_position};
/// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// # use tsgo_core::scriptkind::ScriptKind;
/// let text = "const x = 1;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// let t = get_token_at_position(&mut sf, 6);
/// assert_eq!(sf.kind(t), tsgo_ast::Kind::Identifier);
/// ```
///
/// Side effects: may synthesize and cache a token.
// Go: internal/astnav/tokens.go:GetTokenAtPosition
pub fn get_token_at_position(file: &mut SourceFile, position: i32) -> NodeId {
    get_token_at_position_core(file, position, true, PrecedingTokenFilter::None)
}

/// Returns the token "touching" `position` (positions in leading trivia are not
/// accepted), with no property-name filter.
///
/// # Examples
/// ```
/// # use tsgo_astnav::{SourceFile, get_touching_token};
/// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// # use tsgo_core::scriptkind::ScriptKind;
/// let text = "const x = 1;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// let t = get_touching_token(&mut sf, 6);
/// assert_eq!(sf.kind(t), tsgo_ast::Kind::Identifier);
/// ```
///
/// Side effects: may synthesize and cache a token.
// Go: internal/astnav/tokens.go:GetTouchingToken
pub fn get_touching_token(file: &mut SourceFile, position: i32) -> NodeId {
    get_token_at_position_core(file, position, false, PrecedingTokenFilter::None)
}

/// Returns the token touching `position`, preferring a preceding
/// property-name/keyword/private-identifier token when `position` is at a node's
/// end.
///
/// # Examples
/// ```
/// # use tsgo_astnav::{SourceFile, get_touching_property_name};
/// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// # use tsgo_core::scriptkind::ScriptKind;
/// let text = "const x = 1;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// let t = get_touching_property_name(&mut sf, 6);
/// assert_eq!(sf.kind(t), tsgo_ast::Kind::Identifier);
/// ```
///
/// Side effects: may synthesize and cache a token.
// Go: internal/astnav/tokens.go:GetTouchingPropertyName
pub fn get_touching_property_name(file: &mut SourceFile, position: i32) -> NodeId {
    get_token_at_position_core(file, position, false, PrecedingTokenFilter::PropertyName)
}

/// Finds the leftmost token satisfying `position < token.end()`; if that token
/// is invalid or `position` sits in its trivia, returns the rightmost valid
/// token with `token.end() <= position`.
///
/// # Examples
/// ```
/// # use tsgo_astnav::{SourceFile, find_preceding_token};
/// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// # use tsgo_core::scriptkind::ScriptKind;
/// let text = "a.b";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// // Position 2 is the `b`; the preceding token is the dot.
/// let t = find_preceding_token(&mut sf, 2).unwrap();
/// assert_eq!(sf.kind(t), tsgo_ast::Kind::DotToken);
/// ```
///
/// Side effects: may synthesize and cache tokens.
// Go: internal/astnav/tokens.go:FindPrecedingToken
pub fn find_preceding_token(file: &mut SourceFile, position: i32) -> Option<NodeId> {
    find_preceding_token_ex(file, position, None, false)
}

/// Like [`find_preceding_token`] but starting from `start_node` (defaulting to
/// the file root) and optionally excluding JSDoc.
///
/// # Examples
/// ```
/// # use tsgo_astnav::{SourceFile, find_preceding_token_ex};
/// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// # use tsgo_core::scriptkind::ScriptKind;
/// let text = "a.b";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// let t = find_preceding_token_ex(&mut sf, 2, None, false).unwrap();
/// assert_eq!(sf.kind(t), tsgo_ast::Kind::DotToken);
/// ```
///
/// # Panics
/// Panics if the result is whitespace-only JSX text (mirrors Go's assertion).
///
/// Side effects: may synthesize and cache tokens.
// Go: internal/astnav/tokens.go:FindPrecedingTokenEx
pub fn find_preceding_token_ex(
    file: &mut SourceFile,
    position: i32,
    start_node: Option<NodeId>,
    exclude_jsdoc: bool,
) -> Option<NodeId> {
    let node = start_node.unwrap_or(file.root);
    let result = fpt_find(file, node, position, exclude_jsdoc);
    if let Some(r) = result {
        if is_whitespace_only_jsx_text(file, r) {
            panic!("Expected result to be a non-whitespace token.");
        }
    }
    result
}

/// Recursive worker of [`find_preceding_token_ex`].
///
/// Side effects: may synthesize and cache tokens.
// Go: internal/astnav/tokens.go:FindPrecedingTokenEx (find closure)
fn fpt_find(
    file: &mut SourceFile,
    n: NodeId,
    position: i32,
    exclude_jsdoc: bool,
) -> Option<NodeId> {
    if is_non_whitespace_token(file, n) && file.kind(n) != Kind::EndOfFile {
        return Some(n);
    }

    // `found_child` is the leftmost child containing the target position;
    // `prev_child` is the last visited child before it.
    let mut found_child: Option<NodeId> = None;
    let mut prev_child: Option<NodeId> = None;
    let children = collect_children(file, n);
    for node in children {
        if file.flags(node).contains(NodeFlags::REPARSED) {
            continue;
        }
        if found_child.is_some() {
            continue;
        }
        if position < file.end(node) && prev_child.is_none_or(|pc| file.end(pc) <= position) {
            found_child = Some(node);
        } else {
            prev_child = Some(node);
        }
    }

    if let Some(found_child) = found_child {
        // The span of a node's tokens is [get_start_of_node(node), node.end()).
        // Given `position < found_child.end()`:
        // 1) `position` precedes the child's tokens (or the child has none): look
        //    for the last token in a previous child / preceding child tokens.
        // 2) `position` is within the same span: recurse on the child.
        let start = get_start_of_node(file, found_child, !exclude_jsdoc);
        let look_in_previous_child =
            start >= position || !is_valid_preceding_node(file, found_child);
        if look_in_previous_child {
            if position >= file.pos(found_child) {
                // Find JSDoc preceding `found_child` (always empty here).
                // DEFER(phase-3): JSDoc-preceding branch.
                // blocked-by: JSDoc reparser (tsgo_parser).
                let node_jsdoc = file.node_jsdoc(n).to_vec();
                let mut js_doc: Option<NodeId> = None;
                for i in (0..node_jsdoc.len()).rev() {
                    if file.pos(node_jsdoc[i]) >= file.pos(found_child) {
                        js_doc = Some(node_jsdoc[i]);
                        break;
                    }
                }
                if let Some(js_doc) = js_doc {
                    if !exclude_jsdoc && position < file.end(js_doc) {
                        return fpt_find(file, js_doc, position, exclude_jsdoc);
                    }
                    return find_rightmost_valid_token(
                        file,
                        file.end(js_doc),
                        n,
                        position,
                        exclude_jsdoc,
                    );
                }
                return find_rightmost_valid_token(
                    file,
                    file.pos(found_child),
                    n,
                    -1,
                    exclude_jsdoc,
                );
            }
            // Answer is in tokens between two visited children.
            return find_rightmost_valid_token(
                file,
                file.pos(found_child),
                n,
                position,
                exclude_jsdoc,
            );
        }
        // position is in [found_child.start(), found_child.end()): recurse.
        return fpt_find(file, found_child, position, exclude_jsdoc);
    }

    // Either the position is at the end of the file, or the desired token is in
    // the unvisited trailing tokens of the current node.
    if position >= file.end(n) {
        find_rightmost_valid_token(file, file.end(n), n, -1, exclude_jsdoc)
    } else {
        find_rightmost_valid_token(file, file.end(n), n, position, exclude_jsdoc)
    }
}

/// Reports whether the rightmost-valid-token search should visit `node`.
///
/// Side effects: none (pure).
// Go: internal/astnav/tokens.go:findRightmostValidToken (shouldVisitNode closure)
fn should_visit_node(
    file: &SourceFile,
    node: NodeId,
    end_pos: i32,
    position: i32,
    exclude_jsdoc: bool,
) -> bool {
    !(file.flags(node).contains(NodeFlags::REPARSED)
        || file.end(node) > end_pos
        || get_start_of_node(file, node, !exclude_jsdoc) >= position)
}

/// Finds the rightmost valid token in `[start, end_pos)` that precedes or
/// touches `position` (`position == -1` means "use the containing node's end").
///
/// Side effects: may synthesize and cache tokens.
// Go: internal/astnav/tokens.go:findRightmostValidToken
fn find_rightmost_valid_token(
    file: &mut SourceFile,
    end_pos: i32,
    containing_node: NodeId,
    position: i32,
    exclude_jsdoc: bool,
) -> Option<NodeId> {
    let position = if position == -1 {
        file.end(containing_node)
    } else {
        position
    };
    frvt_find(
        file,
        Some(containing_node),
        end_pos,
        containing_node,
        position,
        exclude_jsdoc,
    )
}

/// Recursive worker of [`find_rightmost_valid_token`].
///
/// Side effects: may synthesize and cache tokens.
// Go: internal/astnav/tokens.go:findRightmostValidToken (find closure)
fn frvt_find(
    file: &mut SourceFile,
    n: Option<NodeId>,
    end_pos: i32,
    containing_node: NodeId,
    position: i32,
    exclude_jsdoc: bool,
) -> Option<NodeId> {
    let n = n?;
    if is_non_whitespace_token(file, n) {
        return Some(n);
    }

    let mut rightmost_valid_node: Option<NodeId> = None;
    // Nodes after the last valid node.
    let mut rightmost_visited_nodes: Vec<NodeId> = Vec::new();
    let mut has_children = false;
    let children = collect_children(file, n);
    for node in children {
        if file.flags(node).contains(NodeFlags::REPARSED) {
            continue;
        }
        has_children = true;
        if !should_visit_node(file, node, end_pos, position, exclude_jsdoc) {
            continue;
        }
        rightmost_visited_nodes.push(node);
        if is_valid_preceding_node(file, node) {
            rightmost_valid_node = Some(node);
            rightmost_visited_nodes.clear();
        }
    }

    // Three cases:
    // 1. The answer is a token of `rightmost_valid_node`.
    // 2. The answer is one of the unvisited tokens after the rightmost valid node.
    // 3. The current node is a childless, token-less node (the answer is itself).

    // Case 2: scan unvisited trailing tokens between the rightmost visited nodes.
    if !should_skip_child(file, n) {
        let mut start_pos = rightmost_valid_node.map_or_else(|| file.pos(n), |r| file.end(r));
        let mut scanner = get_scanner_for_source_file(&file.text, file.language_variant, start_pos);
        let mut tokens: Vec<NodeId> = Vec::new();
        for visited in rightmost_visited_nodes.clone() {
            // Trailing tokens that occur before this node.
            while start_pos < file.pos(visited).min(position) {
                let token = scan_navigation_token(&mut scanner, file, n);
                let token_start = scanner.token_start();
                if token_start >= position {
                    break;
                }
                let token_full_start = scanner.token_full_start();
                let token_end = scanner.token_end();
                start_pos = token_end;
                tokens.push(file.get_or_create_token(token, token_full_start, token_end, n));
                scanner.scan();
            }
            start_pos = file.end(visited);
            scanner.reset_pos(start_pos);
            scanner.scan();
        }
        // Trailing tokens after the last visited node.
        while start_pos < end_pos.min(position) {
            let token = scan_navigation_token(&mut scanner, file, n);
            let token_start = scanner.token_start();
            if token_start >= position {
                break;
            }
            let token_full_start = scanner.token_full_start();
            let token_end = scanner.token_end();
            start_pos = token_end;
            tokens.push(file.get_or_create_token(token, token_full_start, token_end, n));
            scanner.scan();
        }

        // Find the preceding valid token.
        for i in (0..tokens.len()).rev() {
            if !is_whitespace_only_jsx_text(file, tokens[i]) {
                return Some(tokens[i]);
            }
        }
    }

    // Case 3: childless node.
    if !has_children {
        if n != containing_node {
            return Some(n);
        }
        return None;
    }
    // Case 1: recurse on the rightmost valid node.
    let new_end = rightmost_valid_node.map_or(end_pos, |r| file.end(r));
    frvt_find(
        file,
        rightmost_valid_node,
        new_end,
        containing_node,
        position,
        exclude_jsdoc,
    )
}

/// Finds the token immediately following `previous_token` within `parent`.
///
/// # Examples
/// ```
/// # use tsgo_astnav::{SourceFile, get_token_at_position, find_next_token};
/// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// # use tsgo_core::scriptkind::ScriptKind;
/// let text = "a.b";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// let a = get_token_at_position(&mut sf, 0);
/// let root = sf.root();
/// let next = find_next_token(&mut sf, a, root).unwrap();
/// assert_eq!(sf.kind(next), tsgo_ast::Kind::DotToken);
/// ```
///
/// # Panics
/// Panics when the scanner finds trivia (rather than a token) immediately after
/// `previous_token` (mirrors Go's assertion).
///
/// Side effects: may synthesize and cache a token.
// Go: internal/astnav/tokens.go:FindNextToken
pub fn find_next_token(
    file: &mut SourceFile,
    previous_token: NodeId,
    parent: NodeId,
) -> Option<NodeId> {
    fnt_find(file, parent, previous_token)
}

/// Recursive worker of [`find_next_token`].
///
/// Side effects: may synthesize and cache a token.
// Go: internal/astnav/tokens.go:FindNextToken (find closure)
fn fnt_find(file: &mut SourceFile, n: NodeId, previous_token: NodeId) -> Option<NodeId> {
    let prev_end = file.end(previous_token);
    if is_token_kind(file.kind(n)) && file.pos(n) == prev_end {
        // A token starting at the end of the previous token: return it.
        return Some(n);
    }
    // The child that contains `previous_token` or occurs immediately after it.
    let mut found_node: Option<NodeId> = None;
    let children = collect_children(file, n);
    for node in children {
        if found_node.is_some() {
            break;
        }
        if !file.flags(node).contains(NodeFlags::REPARSED)
            && file.pos(node) <= prev_end
            && file.end(node) > prev_end
        {
            found_node = Some(node);
        }
    }
    // Case 3: look for the next token inside the found node.
    if let Some(found_node) = found_node {
        return fnt_find(file, found_node, previous_token);
    }
    let start_pos = prev_end;
    // Case 2: look for the next token directly.
    if start_pos >= file.pos(n) && start_pos < file.end(n) {
        let scanner = get_scanner_for_source_file(&file.text, file.language_variant, start_pos);
        let token = scanner.token();
        let token_full_start = scanner.token_full_start();
        let token_end = scanner.token_end();
        // Use the full start (which includes leading trivia) to match TS's
        // `findNextToken`, where `n.pos === previousToken.end` is checked.
        if token_full_start == prev_end {
            return Some(file.get_or_create_token(token, token_full_start, token_end, n));
        }
        panic!(
            "Expected to find next token at {}, got token {} at {}",
            prev_end, token, token_full_start
        );
    }
    // Case 1: no answer.
    None
}

/// Searches `containing_node` for the first child node or token of `kind`,
/// scanning the tokens that occur between AST children as needed.
///
/// # Examples
/// ```
/// # use tsgo_astnav::{SourceFile, find_child_of_kind};
/// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// # use tsgo_core::scriptkind::ScriptKind;
/// let text = "function f(){}";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// let func = match sf.arena().data(sf.root()) {
///     tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
///     _ => unreachable!(),
/// };
/// // The `function` keyword precedes the function's first child (its name).
/// let kw = find_child_of_kind(&mut sf, func, tsgo_ast::Kind::FunctionKeyword).unwrap();
/// assert_eq!(sf.kind(kw), tsgo_ast::Kind::FunctionKeyword);
/// ```
///
/// Side effects: may synthesize and cache tokens.
// Go: internal/astnav/tokens.go:FindChildOfKind
pub fn find_child_of_kind(
    file: &mut SourceFile,
    containing_node: NodeId,
    kind: Kind,
) -> Option<NodeId> {
    let mut last_node_pos = file.pos(containing_node);
    let mut scan = get_scanner_for_source_file(&file.text, file.language_variant, last_node_pos);

    let children = collect_children(file, containing_node);
    let mut found: Option<NodeId> = None;
    for node in children {
        if file.flags(node).contains(NodeFlags::REPARSED) {
            continue;
        }
        // Look for the child in preceding tokens.
        let mut start_pos = last_node_pos;
        let mut matched = false;
        while start_pos < file.pos(node) {
            let token_kind = scan.token();
            let token_end = scan.token_end();
            if token_kind == kind {
                let token_full_start = scan.token_full_start();
                found = Some(file.get_or_create_token(
                    token_kind,
                    token_full_start,
                    token_end,
                    containing_node,
                ));
                matched = true;
                break;
            }
            start_pos = token_end;
            scan.scan();
        }
        if matched {
            break;
        }
        if file.kind(node) == kind {
            found = Some(node);
            break;
        }
        last_node_pos = file.end(node);
        scan.reset_pos(last_node_pos);
    }

    if found.is_some() {
        return found;
    }

    // Look for the child in trailing tokens.
    let mut start_pos = last_node_pos;
    while start_pos < file.end(containing_node) {
        let token_kind = scan.token();
        let token_end = scan.token_end();
        if token_kind == kind {
            let token_full_start = scan.token_full_start();
            return Some(file.get_or_create_token(
                token_kind,
                token_full_start,
                token_end,
                containing_node,
            ));
        }
        start_pos = token_end;
        scan.scan();
    }
    None
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
