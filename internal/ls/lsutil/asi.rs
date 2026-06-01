//! Automatic-semicolon-insertion (ASI) syntax classification.
//!
//! 1:1 port of Go `internal/ls/lsutil/asi.go`. These predicates classify a
//! syntax [`Kind`] by what terminator the grammar expects after it (semicolon,
//! comma-or-semicolon, function/module block, ...), which drives ASI-related
//! decisions in the formatter and code-fix layers.
//!
//! The node-level entry points [`node_is_asi_candidate`] /
//! [`position_is_asi_candidate`] navigate tokens via `tsgo_astnav`'s shared
//! surface (`FindNextToken` / `GetStartOfNode` over a borrowed arena) and the
//! additive `scanner::get_ecma_line_of_position`.

use crate::children::{get_last_child, get_last_token, SourceFile};
use tsgo_ast::{Kind, NodeArena, NodeId};

/// Reports whether `kind` may be an ASI candidate, i.e. whether any of the more
/// specific terminator classifications applies.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::syntax_may_be_asi_candidate;
/// use tsgo_ast::Kind;
/// assert!(syntax_may_be_asi_candidate(Kind::VariableStatement));
/// assert!(syntax_may_be_asi_candidate(Kind::FunctionDeclaration));
/// assert!(!syntax_may_be_asi_candidate(Kind::SourceFile));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/asi.go:SyntaxMayBeASICandidate
pub fn syntax_may_be_asi_candidate(kind: Kind) -> bool {
    syntax_requires_trailing_comma_or_semicolon_or_asi(kind)
        || syntax_requires_trailing_function_block_or_semicolon_or_asi(kind)
        || syntax_requires_trailing_module_block_or_semicolon_or_asi(kind)
        || syntax_requires_trailing_semicolon_or_asi(kind)
}

/// Reports whether the grammar allows a trailing comma, semicolon, or ASI after
/// a node of this kind (the type-member signatures).
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::syntax_requires_trailing_comma_or_semicolon_or_asi;
/// use tsgo_ast::Kind;
/// assert!(syntax_requires_trailing_comma_or_semicolon_or_asi(Kind::PropertySignature));
/// assert!(!syntax_requires_trailing_comma_or_semicolon_or_asi(Kind::VariableStatement));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingCommaOrSemicolonOrASI
pub fn syntax_requires_trailing_comma_or_semicolon_or_asi(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::CallSignature
            | Kind::ConstructSignature
            | Kind::IndexSignature
            | Kind::PropertySignature
            | Kind::MethodSignature
    )
}

/// Reports whether the grammar allows a trailing function block, semicolon, or
/// ASI after a node of this kind (function-like declarations).
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::syntax_requires_trailing_function_block_or_semicolon_or_asi;
/// use tsgo_ast::Kind;
/// assert!(syntax_requires_trailing_function_block_or_semicolon_or_asi(Kind::FunctionDeclaration));
/// assert!(!syntax_requires_trailing_function_block_or_semicolon_or_asi(Kind::ModuleDeclaration));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingFunctionBlockOrSemicolonOrASI
pub fn syntax_requires_trailing_function_block_or_semicolon_or_asi(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::FunctionDeclaration
            | Kind::Constructor
            | Kind::MethodDeclaration
            | Kind::GetAccessor
            | Kind::SetAccessor
    )
}

/// Reports whether the grammar allows a trailing module block, semicolon, or
/// ASI after a node of this kind (module declarations).
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::syntax_requires_trailing_module_block_or_semicolon_or_asi;
/// use tsgo_ast::Kind;
/// assert!(syntax_requires_trailing_module_block_or_semicolon_or_asi(Kind::ModuleDeclaration));
/// assert!(!syntax_requires_trailing_module_block_or_semicolon_or_asi(Kind::FunctionDeclaration));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingModuleBlockOrSemicolonOrASI
pub fn syntax_requires_trailing_module_block_or_semicolon_or_asi(kind: Kind) -> bool {
    kind == Kind::ModuleDeclaration
}

/// Reports whether the grammar requires a trailing semicolon (or ASI) after a
/// node of this kind (statements and declarations terminated by `;`).
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::syntax_requires_trailing_semicolon_or_asi;
/// use tsgo_ast::Kind;
/// assert!(syntax_requires_trailing_semicolon_or_asi(Kind::ReturnStatement));
/// assert!(!syntax_requires_trailing_semicolon_or_asi(Kind::FunctionDeclaration));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsutil/asi.go:SyntaxRequiresTrailingSemicolonOrASI
pub fn syntax_requires_trailing_semicolon_or_asi(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::VariableStatement
            | Kind::ExpressionStatement
            | Kind::DoStatement
            | Kind::ContinueStatement
            | Kind::BreakStatement
            | Kind::ReturnStatement
            | Kind::ThrowStatement
            | Kind::DebuggerStatement
            | Kind::PropertyDeclaration
            | Kind::TypeAliasDeclaration
            | Kind::ImportDeclaration
            | Kind::ImportEqualsDeclaration
            | Kind::ExportDeclaration
            | Kind::NamespaceExportDeclaration
            | Kind::ExportAssignment
    )
}

/// Reports whether `node` is a place where automatic semicolon insertion could
/// apply: it has no trailing `;` (nor block/comma terminator for the relevant
/// kinds) and the next token starts on a later line, is a `}`, or is the end of
/// file.
///
/// Navigates tokens via `tsgo_astnav`'s shared-borrow surface over this crate's
/// arena, so the parsed arena is never mutated for navigation.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::{node_is_asi_candidate, SourceFile};
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_ast::NodeData;
/// let text = "let a = 1\nlet b = 2;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let stmt0 = match r.arena.data(r.source_file) {
///     NodeData::SourceFile(d) => d.statements.nodes[0],
///     _ => unreachable!(),
/// };
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// assert!(node_is_asi_candidate(&mut sf, stmt0));
/// ```
///
/// Side effects: may synthesize trailing token nodes into the arena (via
/// `get_last_token`/`get_last_child`); navigation uses a borrowed view.
// Go: internal/ls/lsutil/asi.go:NodeIsASICandidate
pub fn node_is_asi_candidate(file: &mut SourceFile, node: NodeId) -> bool {
    let node_kind = file.arena().kind(node);

    let last_token = get_last_token(file, node);
    let last_token_kind = last_token.map(|t| file.arena().kind(t));
    if last_token_kind == Some(Kind::SemicolonToken) {
        return false;
    }

    if syntax_requires_trailing_comma_or_semicolon_or_asi(node_kind) {
        if last_token_kind == Some(Kind::CommaToken) {
            return false;
        }
    } else if syntax_requires_trailing_module_block_or_semicolon_or_asi(node_kind) {
        if let Some(last_child) = get_last_child(file, node) {
            if is_module_block(file.arena(), last_child) {
                return false;
            }
        }
    } else if syntax_requires_trailing_function_block_or_semicolon_or_asi(node_kind) {
        if let Some(last_child) = get_last_child(file, node) {
            if is_function_block(file.arena(), last_child) {
                return false;
            }
        }
    } else if !syntax_requires_trailing_semicolon_or_asi(node_kind) {
        return false;
    }

    // See comment in parser's `parseDoStatement`.
    if node_kind == Kind::DoStatement {
        return true;
    }

    // All `&mut`-borrowing work (token synthesis) is done; build the shared,
    // borrowed navigation view to find the next token without mutating the arena.
    let top_node = match find_ancestor(file.arena(), node, |arena, a| arena.parent(a).is_none()) {
        Some(t) => t,
        None => return true,
    };
    let nav = tsgo_astnav::NavSourceFile::from_borrowed_arena(
        file.arena(),
        file.root(),
        file.text().to_string(),
    );
    let next_token = match nav.find_next_token(node, top_node) {
        None => return true,
        Some(t) => t,
    };
    if nav.kind(next_token) == Kind::CloseBraceToken {
        return true;
    }

    let start_line =
        tsgo_scanner::get_ecma_line_of_position(file.text(), file.arena().loc(node).end());
    let end_line = tsgo_scanner::get_ecma_line_of_position(
        file.text(),
        tsgo_astnav::get_start_of_node(&nav, next_token, false),
    );
    start_line != end_line
}

/// Reports whether `pos` (with `context` the node containing/preceding it) is an
/// ASI candidate position: some ancestor of `context` ends exactly at `pos`, is
/// a may-be-ASI kind, and [`node_is_asi_candidate`].
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::{position_is_asi_candidate, SourceFile};
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_ast::NodeData;
/// let text = "let a = 1\nlet b = 2;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let stmt0 = match r.arena.data(r.source_file) {
///     NodeData::SourceFile(d) => d.statements.nodes[0],
///     _ => unreachable!(),
/// };
/// let end = r.arena.loc(stmt0).end();
/// let mut sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// assert!(position_is_asi_candidate(&mut sf, end, stmt0));
/// ```
///
/// Side effects: same as [`node_is_asi_candidate`].
// Go: internal/ls/lsutil/asi.go:PositionIsASICandidate
pub fn position_is_asi_candidate(file: &mut SourceFile, pos: i32, context: NodeId) -> bool {
    let context_ancestor = find_ancestor_or_quit(file.arena(), context, |arena, ancestor| {
        if arena.loc(ancestor).end() != pos {
            return FindAncestorResult::Quit;
        }
        to_find_ancestor_result(syntax_may_be_asi_candidate(arena.kind(ancestor)))
    });
    match context_ancestor {
        Some(ancestor) => node_is_asi_candidate(file, ancestor),
        None => false,
    }
}

/// Walks up `node`'s parents, returning the first ancestor for which `callback`
/// is `true`, or `None`.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:FindAncestor
fn find_ancestor(
    arena: &NodeArena,
    node: NodeId,
    callback: impl Fn(&NodeArena, NodeId) -> bool,
) -> Option<NodeId> {
    let mut current = Some(node);
    while let Some(n) = current {
        if callback(arena, n) {
            return Some(n);
        }
        current = arena.parent(n);
    }
    None
}

/// The three outcomes of a [`find_ancestor_or_quit`] callback.
// Go: internal/ast/utilities.go:FindAncestorResult
#[derive(Clone, Copy, PartialEq, Eq)]
enum FindAncestorResult {
    False,
    True,
    Quit,
}

/// Maps a boolean to `True`/`False` (never `Quit`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:ToFindAncestorResult
fn to_find_ancestor_result(b: bool) -> FindAncestorResult {
    if b {
        FindAncestorResult::True
    } else {
        FindAncestorResult::False
    }
}

/// Walks up `node`'s parents until `callback` returns `True` (returns that
/// ancestor) or `Quit` (returns `None`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:FindAncestorOrQuit
fn find_ancestor_or_quit(
    arena: &NodeArena,
    node: NodeId,
    callback: impl Fn(&NodeArena, NodeId) -> FindAncestorResult,
) -> Option<NodeId> {
    let mut current = Some(node);
    while let Some(n) = current {
        match callback(arena, n) {
            FindAncestorResult::Quit => return None,
            FindAncestorResult::True => return Some(n),
            FindAncestorResult::False => {}
        }
        current = arena.parent(n);
    }
    None
}

/// Reports whether `n` is a block that is the body of a function-like node.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsFunctionBlock
fn is_function_block(arena: &NodeArena, n: NodeId) -> bool {
    arena.kind(n) == Kind::Block
        && arena
            .parent(n)
            .is_some_and(|p| is_function_like_kind(arena.kind(p)))
}

/// Reports whether `n` is a module/namespace body block.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsModuleBlock
fn is_module_block(arena: &NodeArena, n: NodeId) -> bool {
    arena.kind(n) == Kind::ModuleBlock
}

/// Reports whether `kind` is a function- or signature-like declaration kind.
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsFunctionLikeKind
fn is_function_like_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::MethodSignature
            | Kind::CallSignature
            | Kind::JSDocSignature
            | Kind::ConstructSignature
            | Kind::IndexSignature
            | Kind::FunctionType
            | Kind::ConstructorType
    ) || is_function_like_declaration_kind(kind)
}

/// Reports whether `kind` is a function-like declaration (not a signature).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:isFunctionLikeDeclarationKind
fn is_function_like_declaration_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::FunctionDeclaration
            | Kind::MethodDeclaration
            | Kind::Constructor
            | Kind::GetAccessor
            | Kind::SetAccessor
            | Kind::FunctionExpression
            | Kind::ArrowFunction
    )
}

#[cfg(test)]
#[path = "asi_test.rs"]
mod tests;
