//! "Is this node syntactically completed?" helpers (1:1 port of Go
//! `internal/ls/lsutil/completednode.go`).
//!
//! These answer "does this node look finished?" — e.g. does a block end with
//! `}`, a call with `)`, a statement with `;`? The language service uses them to
//! decide whether a position belongs to a node ([`position_belongs_to_node`])
//! when offering completions / signature help.
//!
//! # Re-enabled over a shared arena
//!
//! Previously deferred: `hasChildOfKind` needs `astnav.FindChildOfKind`, which
//! used to require an arena-owning `tsgo_astnav::SourceFile` that could not
//! share this crate's arena. With `tsgo_astnav`'s shared-borrow surface, this
//! module builds a `tsgo_astnav::NavSourceFile` that *borrows* this crate's
//! arena (`&self`), so [`has_child_of_kind`] runs with shared access and the
//! whole module is `&self` (no `&mut`). [`node_ends_with`] only needs token
//! *kinds*, so it tracks them directly with a scanner instead of synthesizing
//! token nodes (a documented divergence from Go's `GetOrCreateToken`, which is
//! behaviorally identical here).

use crate::children::{get_last_visited_child, SourceFile};
use tsgo_ast::utilities::{node_is_missing, node_is_present};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId};

/// Reports whether `position` belongs to `candidate` — i.e. it is strictly
/// inside the node, or the node is not syntactically completed (so trailing
/// positions still belong to it). Assumes `candidate.pos() <= position`.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::{position_belongs_to_node, SourceFile};
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// let text = "function f() {}";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// // A position inside the source file belongs to the (incomplete-at-pos-0) root.
/// assert!(position_belongs_to_node(&sf, sf.root(), 0));
/// ```
///
/// # Panics
/// Panics if `candidate.pos() > position` (mirrors Go's assertion).
///
/// Side effects: none beyond a scanner pass (no arena mutation).
// Go: internal/ls/lsutil/completednode.go:PositionBelongsToNode
pub fn position_belongs_to_node(file: &SourceFile, candidate: NodeId, position: i32) -> bool {
    if file.arena().loc(candidate).pos() > position {
        panic!("Expected candidate.pos <= position");
    }
    position < file.arena().loc(candidate).end() || !is_completed_node(file, candidate)
}

/// Reports whether `n` is syntactically completed (closed brace/paren/bracket,
/// terminating semicolon, completed sub-node, ...), per TS's `isCompletedNode`.
///
/// # Examples
/// ```
/// use tsgo_ls_lsutil::{is_completed_node, SourceFile};
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_ast::{Kind, NodeData};
/// let text = "function f() {}";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let func = match r.arena.data(r.source_file) {
///     NodeData::SourceFile(d) => d.statements.nodes[0],
///     _ => unreachable!(),
/// };
/// let sf = SourceFile::new(r.arena, r.source_file, text.to_string());
/// assert!(is_completed_node(&sf, func));
/// ```
///
/// Side effects: none beyond scanner passes (no arena mutation).
// Go: internal/ls/lsutil/completednode.go:IsCompletedNode
pub fn is_completed_node(file: &SourceFile, n: NodeId) -> bool {
    is_completed_node_opt(file, Some(n))
}

/// `is_completed_node` over an optional node (Go's `IsCompletedNode(nil)` →
/// `false`), used by the recursive arms whose sub-nodes may be absent.
///
/// Side effects: none beyond scanner passes.
// Go: internal/ls/lsutil/completednode.go:IsCompletedNode
fn is_completed_node_opt(file: &SourceFile, n: Option<NodeId>) -> bool {
    let Some(n) = n else {
        return false;
    };
    let arena = file.arena();
    if node_is_missing(arena, n) {
        return false;
    }

    match arena.kind(n) {
        Kind::ClassDeclaration
        | Kind::InterfaceDeclaration
        | Kind::EnumDeclaration
        | Kind::ObjectLiteralExpression
        | Kind::ObjectBindingPattern
        | Kind::TypeLiteral
        | Kind::Block
        | Kind::ModuleBlock
        | Kind::CaseBlock
        | Kind::NamedImports
        | Kind::NamedExports => node_ends_with(file, n, Kind::CloseBraceToken),

        Kind::CatchClause => is_completed_node(file, catch_clause_block(arena, n)),

        Kind::NewExpression => {
            if !new_expression_has_arguments(arena, n) {
                return true;
            }
            node_ends_with(file, n, Kind::CloseParenToken)
        }

        Kind::CallExpression | Kind::ParenthesizedExpression | Kind::ParenthesizedType => {
            node_ends_with(file, n, Kind::CloseParenToken)
        }

        Kind::FunctionType | Kind::ConstructorType => {
            is_completed_node_opt(file, node_type(arena, n))
        }

        Kind::Constructor
        | Kind::GetAccessor
        | Kind::SetAccessor
        | Kind::FunctionDeclaration
        | Kind::FunctionExpression
        | Kind::MethodDeclaration
        | Kind::MethodSignature
        | Kind::ConstructSignature
        | Kind::CallSignature
        | Kind::ArrowFunction => {
            if let Some(body) = node_body(arena, n) {
                return is_completed_node(file, body);
            }
            if let Some(ty) = node_type(arena, n) {
                return is_completed_node(file, ty);
            }
            // Even though type parameters can be unclosed, we can get away with
            // having at least a closing paren.
            has_child_of_kind(file, n, Kind::CloseParenToken)
        }

        Kind::ModuleDeclaration => match node_body(arena, n) {
            Some(body) => is_completed_node(file, body),
            None => false,
        },

        Kind::IfStatement => {
            let (then_stmt, else_stmt) = if_statement_branches(arena, n);
            match else_stmt {
                Some(els) => is_completed_node(file, els),
                None => is_completed_node(file, then_stmt),
            }
        }

        Kind::ExpressionStatement => {
            is_completed_node_opt(file, node_expression(arena, n))
                || has_child_of_kind(file, n, Kind::SemicolonToken)
        }

        Kind::ArrayLiteralExpression
        | Kind::ArrayBindingPattern
        | Kind::ElementAccessExpression
        | Kind::ComputedPropertyName
        | Kind::TupleType => node_ends_with(file, n, Kind::CloseBracketToken),

        Kind::IndexSignature => match node_type(arena, n) {
            Some(ty) => is_completed_node(file, ty),
            None => has_child_of_kind(file, n, Kind::CloseBracketToken),
        },

        // There is no terminator token for CaseClause/DefaultClause, so for
        // simplicity always consider them non-completed.
        Kind::CaseClause | Kind::DefaultClause => false,

        Kind::ForStatement | Kind::ForInStatement | Kind::ForOfStatement | Kind::WhileStatement => {
            is_completed_node_opt(file, node_statement(arena, n))
        }

        Kind::DoStatement => {
            // Rough approximation: if a DoStatement has a `while` keyword, then
            // completeness is checked by the presence of `)`.
            if has_child_of_kind(file, n, Kind::WhileKeyword) {
                node_ends_with(file, n, Kind::CloseParenToken)
            } else {
                is_completed_node_opt(file, node_statement(arena, n))
            }
        }

        Kind::TypeQuery => is_completed_node(file, type_query_expr_name(arena, n)),

        Kind::TypeOfExpression
        | Kind::DeleteExpression
        | Kind::VoidExpression
        | Kind::YieldExpression
        | Kind::SpreadElement => is_completed_node_opt(file, node_expression(arena, n)),

        Kind::TaggedTemplateExpression => {
            is_completed_node(file, tagged_template_template(arena, n))
        }

        Kind::TemplateExpression => {
            is_completed_node_opt(file, template_expression_last_span(arena, n))
        }

        Kind::TemplateSpan => node_is_present(arena, template_span_literal(arena, n)),

        Kind::ExportDeclaration | Kind::ImportDeclaration => {
            node_is_present_opt(arena, node_module_specifier(arena, n))
        }

        Kind::PrefixUnaryExpression => is_completed_node(file, prefix_unary_operand(arena, n)),

        Kind::BinaryExpression => is_completed_node(file, binary_expression_right(arena, n)),

        Kind::ConditionalExpression => is_completed_node(file, conditional_when_false(arena, n)),

        _ => true,
    }
}

/// Reports whether `n` ends with `expected_last_token`.
///
/// If the last child is a `;`, it is skipped and `expected_last_token` is
/// compared with the child before it.
///
/// Divergence from Go: Go builds token nodes via `GetOrCreateToken` and reads
/// their `.Kind`; only the kinds matter here, so this tracks token kinds with a
/// scanner directly (no node synthesis), keeping the function `&self`.
///
/// Side effects: a scanner pass over the trailing tokens; no arena mutation.
// Go: internal/ls/lsutil/completednode.go:nodeEndsWith
fn node_ends_with(file: &SourceFile, n: NodeId, expected_last_token: Kind) -> bool {
    let arena = file.arena();
    let last_child_node = get_last_visited_child(arena, n);
    let mut last_kinds: Vec<Kind> = Vec::new();
    let token_start_pos = match last_child_node {
        Some(c) => {
            last_kinds.push(arena.kind(c));
            arena.loc(c).end()
        }
        None => arena.loc(n).pos(),
    };
    let node_end = arena.loc(n).end();
    let mut scanner = file.scanner_at(token_start_pos);
    let mut start_pos = token_start_pos;
    while start_pos < node_end {
        let token_kind = scanner.token();
        let token_end = scanner.token_end();
        last_kinds.push(token_kind);
        start_pos = token_end;
        scanner.scan();
    }
    if last_kinds.is_empty() {
        return false;
    }
    let last = last_kinds[last_kinds.len() - 1];
    if last == expected_last_token {
        return true;
    }
    if last == Kind::SemicolonToken && last_kinds.len() > 1 {
        return last_kinds[last_kinds.len() - 2] == expected_last_token;
    }
    false
}

/// Reports whether `containing_node` has a direct child node or token of `kind`.
///
/// Runs `astnav.FindChildOfKind` over a *borrowed* view of this crate's arena
/// (shared access); the synthesized token, if any, lives in the temporary nav
/// handle's side store, never in this crate's arena.
///
/// Side effects: a scanner pass; no arena mutation.
// Go: internal/ls/lsutil/completednode.go:hasChildOfKind
fn has_child_of_kind(file: &SourceFile, containing_node: NodeId, kind: Kind) -> bool {
    let nav = tsgo_astnav::NavSourceFile::from_borrowed_arena(
        file.arena(),
        file.root(),
        file.text().to_string(),
    );
    nav.find_child_of_kind(containing_node, kind).is_some()
}

/// `node_is_present` over an optional node (Go's `NodeIsPresent(nil)` →
/// `false`).
///
/// Side effects: none (pure).
fn node_is_present_opt(arena: &NodeArena, n: Option<NodeId>) -> bool {
    n.is_some_and(|id| node_is_present(arena, id))
}

/// Returns the body of a function-/module-like node, mirroring Go's
/// `node.Body()` for the kinds [`is_completed_node`] queries (`None` for the
/// signature kinds that have no body).
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.Body
fn node_body(arena: &NodeArena, n: NodeId) -> Option<NodeId> {
    match arena.data(n) {
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.body,
        NodeData::MethodDeclaration(d) => d.body,
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => d.body,
        NodeData::ConstructorDeclaration(d) => d.body,
        NodeData::ArrowFunction(d) => Some(d.body),
        NodeData::ModuleDeclaration(d) => d.body,
        _ => None,
    }
}

/// Returns the return/value type of a signature-like node, mirroring Go's
/// `node.Type()` for the kinds [`is_completed_node`] queries.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.Type
fn node_type(arena: &NodeArena, n: NodeId) -> Option<NodeId> {
    match arena.data(n) {
        NodeData::FunctionDeclaration(d) | NodeData::FunctionExpression(d) => d.type_node,
        NodeData::MethodDeclaration(d) => d.type_node,
        NodeData::GetAccessorDeclaration(d) | NodeData::SetAccessorDeclaration(d) => d.type_node,
        NodeData::ConstructorDeclaration(d) => d.type_node,
        NodeData::ArrowFunction(d) => d.type_node,
        NodeData::MethodSignature(d) => d.type_node,
        NodeData::CallSignature(d) | NodeData::ConstructSignature(d) => d.type_node,
        NodeData::FunctionType(d) | NodeData::ConstructorType(d) => d.type_node,
        NodeData::IndexSignatureDeclaration(d) => d.type_node,
        _ => None,
    }
}

/// Returns the operand/argument expression of a unary-child / yield node,
/// mirroring Go's `node.Expression()` for the kinds [`is_completed_node`]
/// queries (`None` for a bare `yield`).
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.Expression
fn node_expression(arena: &NodeArena, n: NodeId) -> Option<NodeId> {
    match arena.data(n) {
        NodeData::ExpressionStatement(d)
        | NodeData::TypeOfExpression(d)
        | NodeData::DeleteExpression(d)
        | NodeData::VoidExpression(d)
        | NodeData::SpreadElement(d) => Some(d.expression),
        NodeData::YieldExpression(d) => d.expression,
        _ => None,
    }
}

/// Returns the body statement of an iteration statement, mirroring Go's
/// `node.Statement()` for the kinds [`is_completed_node`] queries.
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.Statement
fn node_statement(arena: &NodeArena, n: NodeId) -> Option<NodeId> {
    match arena.data(n) {
        NodeData::ForStatement(d) => Some(d.statement),
        NodeData::ForInOrOfStatement(d) => Some(d.statement),
        NodeData::WhileStatement(d) => Some(d.statement),
        NodeData::DoStatement(d) => Some(d.statement),
        _ => None,
    }
}

/// Returns the module specifier of an import/export declaration, mirroring Go's
/// `node.ModuleSpecifier()` (an export without `from` has none).
///
/// Side effects: none (pure).
// Go: internal/ast/ast.go:Node.ModuleSpecifier
fn node_module_specifier(arena: &NodeArena, n: NodeId) -> Option<NodeId> {
    match arena.data(n) {
        NodeData::ImportDeclaration(d) => Some(d.module_specifier),
        NodeData::ExportDeclaration(d) => d.module_specifier,
        _ => None,
    }
}

/// Reports whether a `new` expression has an argument list (`new C` vs `new C()`).
///
/// Side effects: none (pure).
fn new_expression_has_arguments(arena: &NodeArena, n: NodeId) -> bool {
    match arena.data(n) {
        NodeData::NewExpression(d) => d.arguments.is_some(),
        _ => unreachable!("new_expression_has_arguments on a non-NewExpression node"),
    }
}

/// Returns the `catch` clause's body block.
fn catch_clause_block(arena: &NodeArena, n: NodeId) -> NodeId {
    match arena.data(n) {
        NodeData::CatchClause(d) => d.block,
        _ => unreachable!("catch_clause_block on a non-CatchClause node"),
    }
}

/// Returns `(then_statement, else_statement)` of an `if` statement.
fn if_statement_branches(arena: &NodeArena, n: NodeId) -> (NodeId, Option<NodeId>) {
    match arena.data(n) {
        NodeData::IfStatement(d) => (d.then_statement, d.else_statement),
        _ => unreachable!("if_statement_branches on a non-IfStatement node"),
    }
}

/// Returns the queried entity name of a `typeof` type query.
fn type_query_expr_name(arena: &NodeArena, n: NodeId) -> NodeId {
    match arena.data(n) {
        NodeData::TypeQuery(d) => d.expr_name,
        _ => unreachable!("type_query_expr_name on a non-TypeQuery node"),
    }
}

/// Returns the template literal of a tagged-template expression.
fn tagged_template_template(arena: &NodeArena, n: NodeId) -> NodeId {
    match arena.data(n) {
        NodeData::TaggedTemplateExpression(d) => d.template,
        _ => unreachable!("tagged_template_template on a non-TaggedTemplateExpression node"),
    }
}

/// Returns the last template span of a template expression, or `None` when it
/// has no spans (Go's `core.LastOrNil` of a nil/empty list).
fn template_expression_last_span(arena: &NodeArena, n: NodeId) -> Option<NodeId> {
    match arena.data(n) {
        NodeData::TemplateExpression(d) => tsgo_core::last_or_nil(&d.template_spans.nodes),
        _ => unreachable!("template_expression_last_span on a non-TemplateExpression node"),
    }
}

/// Returns the trailing literal of a template span.
fn template_span_literal(arena: &NodeArena, n: NodeId) -> NodeId {
    match arena.data(n) {
        NodeData::TemplateSpan(d) => d.literal,
        _ => unreachable!("template_span_literal on a non-TemplateSpan node"),
    }
}

/// Returns the operand of a prefix unary expression.
fn prefix_unary_operand(arena: &NodeArena, n: NodeId) -> NodeId {
    match arena.data(n) {
        NodeData::PrefixUnaryExpression(d) => d.operand,
        _ => unreachable!("prefix_unary_operand on a non-PrefixUnaryExpression node"),
    }
}

/// Returns the right operand of a binary expression.
fn binary_expression_right(arena: &NodeArena, n: NodeId) -> NodeId {
    match arena.data(n) {
        NodeData::BinaryExpression(d) => d.right,
        _ => unreachable!("binary_expression_right on a non-BinaryExpression node"),
    }
}

/// Returns the false branch of a conditional (ternary) expression.
fn conditional_when_false(arena: &NodeArena, n: NodeId) -> NodeId {
    match arena.data(n) {
        NodeData::ConditionalExpression(d) => d.when_false,
        _ => unreachable!("conditional_when_false on a non-ConditionalExpression node"),
    }
}

#[cfg(test)]
#[path = "completednode_test.rs"]
mod tests;
