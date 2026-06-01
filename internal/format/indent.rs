//! The `SmartIndenter` (reachable subset).
//!
//! 1:1 port of the reachable parts of Go `internal/format/indent.go`. The
//! formatter's span worker needs three things from here:
//!
//! - [`should_indent_child_node`] / [`node_will_indent_child`] — the rules that
//!   decide whether a parent node indents a given child (the core of
//!   block/statement nesting).
//! - [`get_indentation_for_node`] — the initial indentation for the span's
//!   enclosing node (`GetIndentationForNode` → `getIndentationForNodeWorker`).
//! - the column/line helpers used by the worker
//!   ([`find_first_non_whitespace_column`], ...).
//!
//! All run over a [`tsgo_astnav::NavEngine`] shared-borrow context and a
//! precomputed `line_starts` slice (Go caches the line map on the file).
//!
//! # Deferred (blocked-by)
//!
//! - `GetIndentation` (the position-based standalone indenter used by codefixes /
//!   on-enter) and its comment-aware helpers (`getRangeOfEnclosingComment`,
//!   `getCommentIndent`, `getBlockIndent`, `getSmartIndent`,
//!   `nextTokenIsCurlyBraceOnSameLineAsCursor`). These are the continuation /
//!   bound-comment edge cases the round explicitly defers.
//! - "actual indentation from source" (`getActualIndentationForNode`,
//!   `getActualIndentationForListItem`, `deriveActualIndentationFromList`,
//!   `getListByRange`/`getList`/`getVisualListRange`): only reached when the
//!   worker is NOT given an ignore range. The reachable format-span path always
//!   passes the span as the ignore range, so `useActualIndentation` is `false`
//!   for every node walked, and these report the "no actual indentation" value
//!   (`-1` / `None`). Multi-line list continuation indent is deferred with them.

use crate::format_code_settings::FormatCodeSettings;
use crate::util::range_is_on_one_line;
use std::borrow::Borrow;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId};
use tsgo_astnav::{get_start_of_node, NavEngine};
use tsgo_core::text::TextPos;
use tsgo_scanner::compute_line_of_position;

/// Returns the 0-based line containing `pos`.
fn line_of<A: Borrow<NodeArena>>(_file: &NavEngine<A>, line_starts: &[TextPos], pos: i32) -> i32 {
    compute_line_of_position(line_starts, pos)
}

/// Returns the 0-based line and byte offset of `pos`.
fn line_and_byte_offset(line_starts: &[TextPos], pos: i32) -> (i32, i32) {
    let line = compute_line_of_position(line_starts, pos);
    (line, pos - line_starts[line as usize].0)
}

/// Returns the start line of `node` (token start).
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/indent.go:getStartLineForNode
pub fn get_start_line_for_node<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    n: NodeId,
) -> i32 {
    line_of(file, line_starts, get_start_of_node(file, n, false))
}

/// Returns the start line and byte offset of `node` (token start).
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/indent.go:getStartLineAndCharacterForNode
fn get_start_line_and_character_for_node<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    n: NodeId,
) -> (i32, i32) {
    line_and_byte_offset(line_starts, get_start_of_node(file, n, false))
}

/// Returns the column of the first non-whitespace character in `[start_pos, end_pos)`.
///
/// Side effects: none (pure).
// Go: internal/format/indent.go:FindFirstNonWhitespaceColumn
pub fn find_first_non_whitespace_column(
    start_pos: i32,
    end_pos: i32,
    text: &str,
    options: &FormatCodeSettings,
) -> i32 {
    find_first_non_whitespace_character_and_column(start_pos, end_pos, text, options).1
}

/// Returns the byte offset (character) and tab-expanded column of the first
/// non-whitespace character in `[start_pos, end_pos)`.
///
/// Side effects: none (pure).
// Go: internal/format/indent.go:findFirstNonWhitespaceCharacterAndColumn
pub fn find_first_non_whitespace_character_and_column(
    start_pos: i32,
    end_pos: i32,
    text: &str,
    options: &FormatCodeSettings,
) -> (i32, i32) {
    let mut column = 0i32;
    let bytes = text.as_bytes();
    let mut pos = start_pos;
    while pos < end_pos {
        let ch = text[pos as usize..].chars().next().unwrap();
        if !tsgo_stringutil::is_white_space_single_line(ch) {
            break;
        }
        if ch == '\t' {
            if options.editor.tab_size > 0 {
                // NOTE: mirrors Go's `findFirstNonWhitespaceCharacterAndColumn`
                // verbatim (it uses `+`, unlike `characterToColumn` which uses `-`).
                column += options.editor.tab_size + (column % options.editor.tab_size);
            }
        } else {
            column += 1;
        }
        pos += ch.len_utf8() as i32;
    }
    let _ = bytes;
    (pos - start_pos, column)
}

/// Returns the column of the first non-whitespace character on the line that
/// contains `(line, char)`.
///
/// Side effects: none (pure).
// Go: internal/format/indent.go:findColumnForFirstNonWhitespaceCharacterInLine
// DEFER(phase-7): only used by the deferred actual-indentation helpers.
#[allow(dead_code)]
fn find_column_for_first_non_whitespace_character_in_line(
    line: i32,
    char: i32,
    line_starts: &[TextPos],
    text: &str,
    options: &FormatCodeSettings,
) -> i32 {
    let line_start = line_starts[line as usize].0;
    find_first_non_whitespace_column(line_start, line_start + char, text, options)
}

/// Reports whether `kind` is a control-flow-ending statement (`return`,
/// `throw`, `continue`, `break`) outside a block.
///
/// Side effects: none (pure).
// Go: internal/format/indent.go:isControlFlowEndingStatement
fn is_control_flow_ending_statement(kind: Kind, parent_kind: Kind) -> bool {
    matches!(
        kind,
        Kind::ReturnStatement
            | Kind::ThrowStatement
            | Kind::ContinueStatement
            | Kind::BreakStatement
    ) && parent_kind != Kind::Block
}

/// Reports whether the parent node should indent the given child by an explicit
/// rule.
///
/// Mirrors Go's `ShouldIndentChildNode`. `with_source_file` corresponds to Go's
/// non-nil `sourceFile` argument (gating the line-dependent branches in
/// [`node_will_indent_child`]).
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/indent.go:ShouldIndentChildNode
pub fn should_indent_child_node<A: Borrow<NodeArena>>(
    settings: &FormatCodeSettings,
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    with_source_file: bool,
    parent: NodeId,
    child: Option<NodeId>,
    is_next_child: bool,
) -> bool {
    node_will_indent_child(
        settings,
        file,
        line_starts,
        with_source_file,
        parent,
        child,
        false,
    ) && !(is_next_child
        && child.is_some_and(|c| is_control_flow_ending_statement(file.kind(c), file.kind(parent))))
}

/// Reports whether `parent` will indent `child` (the big per-kind switch).
///
/// Mirrors Go's `NodeWillIndentChild`. `with_source_file` gates the
/// line-relationship branches (Go's `sourceFile != nil`).
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/indent.go:NodeWillIndentChild
pub fn node_will_indent_child<A: Borrow<NodeArena>>(
    settings: &FormatCodeSettings,
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    with_source_file: bool,
    parent: NodeId,
    child: Option<NodeId>,
    indent_by_default: bool,
) -> bool {
    let child_kind = child.map_or(Kind::Unknown, |c| file.kind(c));
    match file.kind(parent) {
        Kind::ExpressionStatement
        | Kind::ClassDeclaration
        | Kind::ClassExpression
        | Kind::InterfaceDeclaration
        | Kind::EnumDeclaration
        | Kind::TypeAliasDeclaration
        | Kind::ArrayLiteralExpression
        | Kind::Block
        | Kind::ModuleBlock
        | Kind::ObjectLiteralExpression
        | Kind::TypeLiteral
        | Kind::MappedType
        | Kind::TupleType
        | Kind::ParenthesizedExpression
        | Kind::PropertyAccessExpression
        | Kind::CallExpression
        | Kind::NewExpression
        | Kind::VariableStatement
        | Kind::ExportAssignment
        | Kind::ReturnStatement
        | Kind::ConditionalExpression
        | Kind::ArrayBindingPattern
        | Kind::ObjectBindingPattern
        | Kind::JsxOpeningElement
        | Kind::JsxOpeningFragment
        | Kind::JsxSelfClosingElement
        | Kind::JsxExpression
        | Kind::MethodSignature
        | Kind::CallSignature
        | Kind::ConstructSignature
        | Kind::Parameter
        | Kind::FunctionType
        | Kind::ConstructorType
        | Kind::ParenthesizedType
        | Kind::TaggedTemplateExpression
        | Kind::AwaitExpression
        | Kind::NamedExports
        | Kind::NamedImports
        | Kind::ExportSpecifier
        | Kind::ImportSpecifier
        | Kind::PropertyDeclaration
        | Kind::CaseClause
        | Kind::DefaultClause => true,
        Kind::CaseBlock => settings.indent_switch_case.is_true_or_unknown(),
        Kind::VariableDeclaration | Kind::PropertyAssignment | Kind::BinaryExpression => {
            if settings
                .indent_multi_line_object_literal_beginning_on_blank_line
                .is_false_or_unknown()
                && with_source_file
                && child_kind == Kind::ObjectLiteralExpression
            {
                return range_is_on_one_line(line_starts, file.arena().loc(child.unwrap()));
            }
            if file.kind(parent) == Kind::BinaryExpression
                && with_source_file
                && child_kind == Kind::JsxElement
            {
                let parent_start_line = line_of(
                    file,
                    line_starts,
                    tsgo_scanner::skip_trivia(file.text(), file.pos(parent)),
                );
                let child_start_line = line_of(
                    file,
                    line_starts,
                    tsgo_scanner::skip_trivia(file.text(), file.pos(child.unwrap())),
                );
                return parent_start_line != child_start_line;
            }
            if file.kind(parent) != Kind::BinaryExpression {
                return true;
            }
            indent_by_default
        }
        Kind::DoStatement
        | Kind::WhileStatement
        | Kind::ForInStatement
        | Kind::ForOfStatement
        | Kind::ForStatement
        | Kind::IfStatement
        | Kind::FunctionDeclaration
        | Kind::FunctionExpression
        | Kind::MethodDeclaration
        | Kind::Constructor
        | Kind::GetAccessor
        | Kind::SetAccessor => child_kind != Kind::Block,
        Kind::ArrowFunction => {
            if with_source_file && child_kind == Kind::ParenthesizedExpression {
                return range_is_on_one_line(line_starts, file.arena().loc(child.unwrap()));
            }
            child_kind != Kind::Block
        }
        Kind::ExportDeclaration => child_kind != Kind::NamedExports,
        Kind::ImportDeclaration => {
            child_kind != Kind::ImportClause
                || child.is_some_and(|c| match file.arena().data(c) {
                    NodeData::ImportClause(d) => d
                        .named_bindings
                        .is_some_and(|nb| file.kind(nb) != Kind::NamedImports),
                    _ => false,
                })
        }
        Kind::JsxElement => child_kind != Kind::JsxClosingElement,
        Kind::JsxFragment => child_kind != Kind::JsxClosingFragment,
        Kind::IntersectionType | Kind::UnionType | Kind::SatisfiesExpression => {
            if child_kind == Kind::TypeLiteral
                || child_kind == Kind::TupleType
                || child_kind == Kind::MappedType
            {
                return false;
            }
            indent_by_default
        }
        Kind::TryStatement => {
            if child_kind == Kind::Block {
                return false;
            }
            indent_by_default
        }
        _ => indent_by_default,
    }
}

/// Reports whether `child` is the `else` branch of an `if` and begins on the
/// same line as the `else` keyword.
///
/// Side effects: may synthesize tokens via the nav context.
// Go: internal/format/indent.go:childStartsOnTheSameLineWithElseInIfStatement
pub fn child_starts_on_the_same_line_with_else_in_if_statement<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    parent: NodeId,
    child: NodeId,
    child_start_line: i32,
) -> bool {
    if file.kind(parent) == Kind::IfStatement {
        if let NodeData::IfStatement(d) = file.arena().data(parent) {
            if d.else_statement == Some(child) {
                let else_keyword = file
                    .find_preceding_token(file.pos(child))
                    .expect("else keyword expected");
                let else_keyword_start_line =
                    get_start_line_for_node(file, line_starts, else_keyword);
                return else_keyword_start_line == child_start_line;
            }
        }
    }
    false
}

/// A multiline conditional's `whenTrue`/`whenFalse` branch that should not be
/// re-indented (its own multi-line layout already indents its contents).
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/indent.go:childIsUnindentedBranchOfConditionalExpression
pub fn child_is_unindented_branch_of_conditional_expression<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    parent: NodeId,
    child: NodeId,
    child_start_line: i32,
) -> bool {
    if file.kind(parent) == Kind::ConditionalExpression {
        if let NodeData::ConditionalExpression(d) = file.arena().data(parent) {
            let (condition, when_true, when_false) = (d.condition, d.when_true, d.when_false);
            if child == when_true || child == when_false {
                let condition_end_line = line_of(file, line_starts, file.end(condition));
                if child == when_true {
                    return child_start_line == condition_end_line;
                }
                let true_start_line = get_start_line_for_node(file, line_starts, when_true);
                let true_end_line = line_of(file, line_starts, file.end(when_true));
                return condition_end_line == true_start_line && true_end_line == child_start_line;
            }
        }
    }
    false
}

/// Reports whether `child` is a call/new argument starting on the same line as
/// the previous argument.
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/indent.go:argumentStartsOnSameLineAsPreviousArgument
pub fn argument_starts_on_same_line_as_previous_argument<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    parent: NodeId,
    child: NodeId,
    child_start_line: i32,
) -> bool {
    let arguments: Vec<NodeId> = match file.arena().data(parent) {
        NodeData::CallExpression(d) => d.arguments.nodes.clone(),
        NodeData::NewExpression(d) => d
            .arguments
            .as_ref()
            .map_or_else(Vec::new, |l| l.nodes.clone()),
        _ => return false,
    };
    if arguments.is_empty() {
        return false;
    }
    let current_index = arguments.iter().position(|&n| n == child);
    let current_index = match current_index {
        None => return false,
        Some(0) => return false,
        Some(i) => i,
    };
    let previous_node = arguments[current_index - 1];
    let line_of_previous_node = line_of(file, line_starts, file.end(previous_node));
    child_start_line == line_of_previous_node
}

/// Reports whether `child` is a call argument whose start line overlaps the
/// callee expression's end line.
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/indent.go:isArgumentAndStartLineOverlapsExpressionBeingCalled
fn is_argument_and_start_line_overlaps_expression_being_called<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    parent: NodeId,
    child: NodeId,
    child_start_line: i32,
) -> bool {
    let (expression, contains) = match file.arena().data(parent) {
        NodeData::CallExpression(d) => (d.expression, d.arguments.nodes.contains(&child)),
        _ => return false,
    };
    if !contains {
        return false;
    }
    let expression_end_line = line_of(file, line_starts, file.end(expression));
    expression_end_line == child_start_line
}

/// Returns the list that wraps `node`, or `None`.
///
/// Mirrors Go's `GetContainingList`. The list-detection itself
/// ([`get_list_by_range`]) is deferred (see module docs), so this reports `None`
/// for the reachable node kinds.
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/indent.go:GetContainingList
pub fn get_containing_list<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    node: NodeId,
) -> Option<NodeId> {
    file.arena().parent(node)?;
    // DEFER(phase-7): wrapping-list detection (getListByRange / getVisualListRange).
    // blocked-by: per-node list accessors (TypeArgumentList/ParameterList/...).
    None
}

/// Returns the start line/char of `child`'s containing list, or of `parent`'s
/// token start when there is none.
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/indent.go:getContainingListOrParentStart
fn get_containing_list_or_parent_start<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    parent: NodeId,
    child: NodeId,
) -> (i32, i32) {
    let start_pos = match get_containing_list(file, child) {
        Some(_list) => {
            // DEFER(phase-7): list start position; unreachable while get_containing_list is None.
            get_start_of_node(file, parent, false)
        }
        None => get_start_of_node(file, parent, false),
    };
    line_and_byte_offset(line_starts, start_pos)
}

/// Computes the initial indentation for `n`, ignoring the actual indentation of
/// positions inside `ignore_actual_indentation_range`.
///
/// Mirrors Go's `GetIndentationForNode`.
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/indent.go:GetIndentationForNode
pub fn get_indentation_for_node<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    n: NodeId,
    ignore_actual_indentation_range: Option<tsgo_core::text::TextRange>,
    options: &FormatCodeSettings,
) -> i32 {
    let (start_line, start_char) = get_start_line_and_character_for_node(file, line_starts, n);
    get_indentation_for_node_worker(
        file,
        line_starts,
        n,
        start_line,
        start_char,
        ignore_actual_indentation_range,
        0,
        false,
        options,
    )
}

/// The walk-up worker behind [`get_indentation_for_node`].
///
/// Side effects: none (pure; reads the nav context).
// Go: internal/format/indent.go:getIndentationForNodeWorker
#[allow(clippy::too_many_arguments)]
fn get_indentation_for_node_worker<A: Borrow<NodeArena>>(
    file: &NavEngine<A>,
    line_starts: &[TextPos],
    current_init: NodeId,
    current_start_line_init: i32,
    // Go tracks `currentStartCharacter` for the deferred actual-indentation block
    // (`getActualIndentationForNode`); it is otherwise unread, so it is dropped here.
    _current_start_character_init: i32,
    ignore_actual_indentation_range: Option<tsgo_core::text::TextRange>,
    indentation_delta_init: i32,
    is_next_child: bool,
    options: &FormatCodeSettings,
) -> i32 {
    let mut current = current_init;
    let mut current_start_line = current_start_line_init;
    let mut indentation_delta = indentation_delta_init;
    let mut parent = file.arena().parent(current);

    while let Some(parent_id) = parent {
        let mut use_actual_indentation = true;
        if let Some(ignore) = ignore_actual_indentation_range {
            let start = get_start_of_node(file, current, false);
            use_actual_indentation = start < ignore.pos() || start > ignore.end();
        }
        // DEFER(phase-7): when `use_actual_indentation` is true, Go reads the actual
        // indentation from source (list items + statement/declaration indentation).
        // Unreachable on the format-span path, which always supplies the span as the
        // ignore range (so this is false for every node walked).
        // blocked-by: getListByRange / IsDeclaration / IsStatementButNotDeclaration.
        let _ = use_actual_indentation;

        let (containing_list_or_parent_start_line, _containing_list_or_parent_start_character) =
            get_containing_list_or_parent_start(file, line_starts, parent_id, current);
        let parent_and_child_share_line = containing_list_or_parent_start_line
            == current_start_line
            || child_starts_on_the_same_line_with_else_in_if_statement(
                file,
                line_starts,
                parent_id,
                current,
                current_start_line,
            );

        if should_indent_child_node(
            options,
            file,
            line_starts,
            true,
            parent_id,
            Some(current),
            is_next_child,
        ) && !parent_and_child_share_line
        {
            indentation_delta += options.editor.indent_size;
        }

        let use_true_start = is_argument_and_start_line_overlaps_expression_being_called(
            file,
            line_starts,
            parent_id,
            current,
            current_start_line,
        );

        current = parent_id;
        parent = file.arena().parent(current);

        if use_true_start {
            current_start_line =
                line_of(file, line_starts, get_start_of_node(file, current, false));
        } else {
            current_start_line = containing_list_or_parent_start_line;
        }
    }

    indentation_delta + options.editor.base_indent_size
}

#[cfg(test)]
#[path = "indent_test.rs"]
mod tests;
