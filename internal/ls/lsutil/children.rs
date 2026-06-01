//! Child / token navigation helpers (1:1 port of Go `children.go`).
//!
//! These functions answer "what is the last/first child or token of this node?"
//! — questions the parser does not pre-answer because punctuation tokens
//! (`;`, `(`, `)`, ...) are not kept as standalone AST nodes. Like Go (and like
//! `tsgo_astnav`), they re-scan the source between AST children to synthesize
//! those tokens on demand.
//!
//! # Navigation context
//!
//! Go threads a `*ast.SourceFile` (which carries the source text and the
//! synthesized-token cache). The Rust `tsgo_ast` `SourceFile` node carries
//! neither, so this module defines [`SourceFile`]: an owning context bundling
//! the [`NodeArena`], the root id, the text, the language variant, and the
//! token cache. This mirrors `tsgo_astnav::SourceFile` exactly.
//!
//! `tsgo_scanner` defers the `ast.SourceFile`-based `GetScannerForSourceFile`
//! and `tsgo_astnav` keeps `GetOrCreateToken` private, so — to stay within this
//! crate's edit boundary — the scanner factory and the token cache are
//! replicated here over the owned arena. The behavior is identical to Go's
//! `scanner.GetScannerForSourceFile` + `sourceFile.GetOrCreateToken`.

use std::collections::HashMap;

use tsgo_ast::utilities::{is_identifier, is_token_kind};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeFlags, NodeId};
use tsgo_core::languagevariant::LanguageVariant;
use tsgo_core::text::TextRange;
use tsgo_scanner::Scanner;

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
/// Owns the [`NodeArena`], the root `SourceFile` node id, the source text, the
/// language variant, and the synthesized-token cache. Stands in for Go's
/// `*ast.SourceFile` (which carried both the text and the cache). Held by
/// `&mut` so the navigation helpers can synthesize and memoize tokens, which is
/// what makes repeated queries at the same position return the same node id.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::SourceFile;
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// let text = "let a = 1;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// assert_eq!(sf.arena().kind(sf.root()), tsgo_ast::Kind::SourceFile);
/// ```
///
/// Side effects: methods taking `&mut self` may push synthesized token nodes and
/// populate the token cache.
pub struct SourceFile {
    arena: NodeArena,
    root: NodeId,
    text: String,
    language_variant: LanguageVariant,
    token_cache: HashMap<TokenCacheKey, NodeId>,
}

impl SourceFile {
    /// Builds a navigation context from a parse result's `arena`, `root`
    /// `SourceFile` id, and the original source `text`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ls_lsutil::SourceFile;
    /// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
    /// use tsgo_core::scriptkind::ScriptKind;
    /// let text = "a;";
    /// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
    /// let sf = SourceFile::new(r.arena, r.source_file, text.to_string());
    /// assert_eq!(sf.text(), "a;");
    /// ```
    ///
    /// # Panics
    /// Panics if `root` is not a `SourceFile` node.
    ///
    /// Side effects: none (takes ownership of the arena and text).
    pub fn new(arena: NodeArena, root: NodeId, text: String) -> SourceFile {
        let language_variant = match arena.data(root) {
            NodeData::SourceFile(d) => d.language_variant,
            _ => panic!("SourceFile::new expects a SourceFile node as root"),
        };
        SourceFile {
            arena,
            root,
            text,
            language_variant,
            token_cache: HashMap::new(),
        }
    }

    /// Returns the root `SourceFile` node id.
    ///
    /// # Examples
    /// ```
    /// # use tsgo_ls_lsutil::SourceFile;
    /// # use tsgo_parser::{parse_source_file, SourceFileParseOptions};
    /// # use tsgo_core::scriptkind::ScriptKind;
    /// let r = parse_source_file(SourceFileParseOptions::default(), "x;", ScriptKind::Ts);
    /// let sf = SourceFile::new(r.arena, r.source_file, "x;".to_string());
    /// assert_eq!(sf.arena().kind(sf.root()), tsgo_ast::Kind::SourceFile);
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
    /// # use tsgo_ls_lsutil::SourceFile;
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
    /// # use tsgo_ls_lsutil::SourceFile;
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

    /// Returns a previously synthesized token for `(kind, pos, end, parent)`, or
    /// creates, caches, and returns a fresh one.
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

    /// Creates a scanner positioned at `pos` over the source text, having
    /// scanned its first token (mirrors `scanner.GetScannerForSourceFile`).
    ///
    /// Side effects: allocates a scanner that owns a copy of the text.
    // Go: internal/scanner/scanner.go:GetScannerForSourceFile
    pub(crate) fn scanner_at(&self, pos: i32) -> Scanner {
        let mut s = Scanner::new();
        // PERF(port): Go shares `sourceFile.Text()` by reference; the scanner
        // here owns its text, so each query clones the source.
        s.set_text(self.text.clone());
        s.set_language_variant(self.language_variant);
        s.reset_pos(pos);
        s.scan();
        s
    }
}

/// Reports whether `pos` is a synthesized (negative) position.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:PositionIsSynthesized
fn position_is_synthesized(pos: i32) -> bool {
    pos < 0
}

/// Reports whether `node` is a JSDoc node with a single (string) comment.
///
/// Always `false`: the parser has not ported the JSDoc reparser, so no node
/// carries cached JSDoc comments. Kept so [`get_last_child`] mirrors Go's
/// structure.
///
/// Side effects: none (pure).
// DEFER(phase-3): return the real result once the parser reparses JSDoc.
// blocked-by: JSDoc reparser (tsgo_parser).
// Go: internal/ast/utilities.go:IsJSDocSingleCommentNode
fn is_jsdoc_single_comment_node(_arena: &NodeArena, _node: NodeId) -> bool {
    false
}

/// Panics unless `node` has a real (non-synthesized) position.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::assert_has_real_position;
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// let r = parse_source_file(SourceFileParseOptions::default(), "x;", ScriptKind::Ts);
/// assert_has_real_position(&r.arena, r.source_file); // does not panic
/// ```
///
/// # Panics
/// Panics if the node's start or end position is synthesized (negative).
///
/// Side effects: none (pure; may panic).
// Go: internal/ls/lsutil/children.go:AssertHasRealPosition
pub fn assert_has_real_position(arena: &NodeArena, node: NodeId) {
    let loc = arena.loc(node);
    if position_is_synthesized(loc.pos()) || position_is_synthesized(loc.end()) {
        panic!("Node must have a real position for this operation.");
    }
}

/// Returns the last visited (non-reparsed) child of `node`, or `None`.
///
/// NOTE: this does not include unvisited tokens; for those use
/// [`get_last_child`] or [`get_last_token`].
///
/// Go uses `astnav.VisitEachChildAndJSDoc` with separate single-node / node-list
/// visitor hooks (the list hook scans in reverse for the last non-reparsed
/// element). `tsgo_ast`'s `for_each_child` yields a flat, source-ordered child
/// stream, so keeping the last non-reparsed child while iterating produces the
/// same result. JSDoc is always empty here (the parser does not reparse it),
/// matching `tsgo_astnav::visit_each_child_and_jsdoc`.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::{get_last_visited_child, SourceFile};
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_ast::{Kind, NodeData};
/// let text = "let a = 1;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let stmt = match r.arena.data(r.source_file) {
///     NodeData::SourceFile(d) => d.statements.nodes[0],
///     _ => unreachable!(),
/// };
/// // The variable statement's last visited child is its declaration list.
/// let last = get_last_visited_child(&r.arena, stmt).unwrap();
/// assert_eq!(r.arena.kind(last), Kind::VariableDeclarationList);
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/children.go:GetLastVisitedChild
pub fn get_last_visited_child(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    let mut last_child: Option<NodeId> = None;
    arena.for_each_child(node, &mut |n| {
        if !arena.flags(n).contains(NodeFlags::REPARSED) {
            last_child = Some(n);
        }
        false
    });
    last_child
}

/// Returns the last child of `node`, including unvisited trailing tokens.
///
/// Replaces Go's `last(node.getChildren(sourceFile))`: it finds the last visited
/// child, then scans the tokens after it (up to the node's end) and returns the
/// last token if any, otherwise the last visited child.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::{get_last_child, SourceFile};
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_ast::{Kind, NodeData};
/// let text = "let a = 1;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let stmt = match r.arena.data(r.source_file) {
///     NodeData::SourceFile(d) => d.statements.nodes[0],
///     _ => unreachable!(),
/// };
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// // The trailing `;` is the statement's last child.
/// let last = get_last_child(&mut sf, stmt).unwrap();
/// assert_eq!(sf.arena().kind(last), Kind::SemicolonToken);
/// ```
///
/// Side effects: may synthesize and cache the trailing token nodes.
// Go: internal/ls/lsutil/children.go:GetLastChild
pub fn get_last_child(file: &mut SourceFile, node: NodeId) -> Option<NodeId> {
    let last_child_node = get_last_visited_child(&file.arena, node);
    if is_jsdoc_single_comment_node(&file.arena, node) && last_child_node.is_none() {
        return None;
    }
    let token_start_pos = match last_child_node {
        Some(c) => file.arena.loc(c).end(),
        None => file.arena.loc(node).pos(),
    };
    let node_end = file.arena.loc(node).end();
    let mut last_token: Option<NodeId> = None;
    let mut scanner = file.scanner_at(token_start_pos);
    let mut start_pos = token_start_pos;
    while start_pos < node_end {
        let token_kind = scanner.token();
        let token_full_start = scanner.token_full_start();
        let token_end = scanner.token_end();
        last_token = Some(file.get_or_create_token(token_kind, token_full_start, token_end, node));
        start_pos = token_end;
        scanner.scan();
    }
    // core.IfElse(lastToken != nil, lastToken, lastChildNode)
    last_token.or(last_child_node)
}

/// Returns the last token of `node` (descending into the last child), or `None`
/// for tokens/identifiers.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::{get_last_token, SourceFile};
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_ast::{Kind, NodeData};
/// let text = "let a = 1;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let stmt = match r.arena.data(r.source_file) {
///     NodeData::SourceFile(d) => d.statements.nodes[0],
///     _ => unreachable!(),
/// };
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// let last = get_last_token(&mut sf, stmt).unwrap();
/// assert_eq!(sf.arena().kind(last), Kind::SemicolonToken);
/// ```
///
/// Side effects: may synthesize and cache token nodes.
// Go: internal/ls/lsutil/children.go:GetLastToken
pub fn get_last_token(file: &mut SourceFile, node: NodeId) -> Option<NodeId> {
    let kind = file.arena.kind(node);
    if is_token_kind(kind) || is_identifier(&file.arena, node) {
        return None;
    }

    assert_has_real_position(&file.arena, node);

    let last_child = get_last_child(file, node)?;
    if file.arena.kind(last_child) < Kind::FIRST_NODE {
        Some(last_child)
    } else {
        get_last_token(file, last_child)
    }
}

/// Returns the first token of `node` (descending into the first child), or
/// `None` for tokens/identifiers.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::{get_first_token, SourceFile};
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_ast::{Kind, NodeData};
/// let text = "let a = 1;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let stmt = match r.arena.data(r.source_file) {
///     NodeData::SourceFile(d) => d.statements.nodes[0],
///     _ => unreachable!(),
/// };
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// // The first token is the `let` keyword.
/// let first = get_first_token(&mut sf, stmt).unwrap();
/// assert_eq!(sf.arena().kind(first), Kind::LetKeyword);
/// ```
///
/// Side effects: may synthesize and cache token nodes.
// Go: internal/ls/lsutil/children.go:GetFirstToken
pub fn get_first_token(file: &mut SourceFile, node: NodeId) -> Option<NodeId> {
    if is_identifier(&file.arena, node) || is_token_kind(file.arena.kind(node)) {
        return None;
    }
    assert_has_real_position(&file.arena, node);

    // Find the first non-reparsed child. Go checks `node.Flags` (the parent's
    // flags) inside the callback, so a reparsed parent yields no first child.
    let mut first_child: Option<NodeId> = None;
    let node_flags = file.arena.flags(node);
    file.arena.for_each_child(node, &mut |n| {
        if node_flags.contains(NodeFlags::REPARSED) {
            return false;
        }
        first_child = Some(n);
        true
    });

    let token_end_position = match first_child {
        Some(c) => file.arena.loc(c).pos(),
        None => file.arena.loc(node).end(),
    };
    let node_pos = file.arena.loc(node).pos();
    let mut first_token: Option<NodeId> = None;
    if node_pos < token_end_position {
        let scanner = file.scanner_at(node_pos);
        let token_kind = scanner.token();
        let token_full_start = scanner.token_full_start();
        let token_end = scanner.token_end();
        first_token = Some(file.get_or_create_token(token_kind, token_full_start, token_end, node));
    }

    if first_token.is_some() {
        return first_token;
    }
    let first_child = first_child?;
    if file.arena.kind(first_child) < Kind::FIRST_NODE {
        Some(first_child)
    } else {
        get_first_token(file, first_child)
    }
}

#[cfg(test)]
#[path = "children_test.rs"]
mod tests;
