//! Port of the reachable subset of Go `internal/ls/change/delete.go`.
//!
//! The trivia-aware node-deletion dispatcher (`delete_declaration`), the
//! variable-declaration specialization (`delete_variable_declaration`), the core
//! `delete_node` trivia helper, and `start_position_to_delete_node_in_list`.
//!
//! # Deferred (blocked-by)
//!
//! - `deleteNodeInList` and `endPositionToDeleteNodeInList`: gated on
//!   `format::indent::get_containing_list`, which the `tsgo_format` port stubs
//!   to `None`. So comma-separated list deletion (parameters, type parameters,
//!   import specifiers, binding elements, multi-declaration variable lists, call
//!   arguments) collapses into the generic whole-node deletion below.
//! - `deleteImportBinding` / `deleteDefaultImport` and `sourceFile.Imports()`:
//!   need `ast` import-clause accessors not yet ported.
//! - The `ForOf`/`ForIn` variable case replaces the binding with `{}` via the
//!   deferred node-insertion path.

use std::rc::Rc;

use tsgo_ast::{Kind, NodeData, NodeId};
use tsgo_astnav::RcSourceFile;
use tsgo_scanner::{skip_trivia_ex, SkipTriviaOptions};

use crate::tracker::{file_name, ChangeTracker, LeadingTriviaOption, TrailingTriviaOption};

/// Deletes `node` with smart, kind-specific trivia handling.
///
/// Mirrors Go's `deleteDeclaration`. The reachable arms are
/// variable/class/function declarations, the `function`/`type` keywords, and the
/// trailing semicolon; the comma-list and import arms are deferred (see module
/// docs) and fall through to the generic whole-node deletion.
///
/// Side effects: appends an edit to `t`.
// Go: internal/ls/change/delete.go:deleteDeclaration
pub(crate) fn delete_declaration(
    t: &mut ChangeTracker,
    source_file: &Rc<RcSourceFile>,
    node: NodeId,
) {
    match source_file.kind(node) {
        Kind::VariableDeclaration => {
            delete_variable_declaration(t, source_file, node);
        }

        Kind::SemicolonToken => {
            delete_node(
                t,
                source_file,
                node,
                LeadingTriviaOption::IncludeAll,
                TrailingTriviaOption::Exclude,
            );
        }

        // For the `type`/`function` keyword, delete the keyword and the trailing
        // space (which is part of the next token's leading trivia).
        Kind::TypeKeyword | Kind::FunctionKeyword => {
            delete_node(
                t,
                source_file,
                node,
                LeadingTriviaOption::Exclude,
                TrailingTriviaOption::Include,
            );
        }

        Kind::ClassDeclaration | Kind::FunctionDeclaration => {
            // Go selects JSDoc-leading trivia when the node has JSDoc; in this
            // port `has_jsdoc_nodes` is always false (see crate root).
            let leading_trivia = if has_jsdoc_nodes() {
                LeadingTriviaOption::JSDoc
            } else {
                LeadingTriviaOption::StartLine
            };
            delete_node(
                t,
                source_file,
                node,
                leading_trivia,
                TrailingTriviaOption::Include,
            );
        }

        // DEFER(phase-7): the comma-list arms (Parameter, TypeParameter,
        // BindingElement, ImportSpecifier) and the import arms (ImportDeclaration,
        // ImportEqualsDeclaration, NamespaceImport) need `deleteNodeInList` /
        // import accessors; they collapse into the generic deletion below.
        // blocked-by: format::indent::get_containing_list, ast import accessors.
        _ => {
            // The Go default also specializes default-import (`ImportClause`)
            // and call-argument (`CallExpression`) parents via the deferred
            // paths above; here every remaining node is deleted whole.
            delete_node(
                t,
                source_file,
                node,
                LeadingTriviaOption::IncludeAll,
                TrailingTriviaOption::Include,
            );
        }
    }
}

/// Deletes a `VariableDeclaration`, widening to the enclosing statement when it
/// is the sole declaration.
///
/// Mirrors Go's `deleteVariableDeclaration`. The multi-declaration list case and
/// the `for-of`/`for-in` rebinding case are deferred (see module docs); the
/// `VariableStatement`, `ForStatement`, and `CatchClause` cases are reachable.
///
/// Side effects: appends an edit to `t`.
// Go: internal/ls/change/delete.go:deleteVariableDeclaration
fn delete_variable_declaration(
    t: &mut ChangeTracker,
    source_file: &Rc<RcSourceFile>,
    node: NodeId,
) {
    let Some(parent) = source_file.arena().parent(node) else {
        delete_node(
            t,
            source_file,
            node,
            LeadingTriviaOption::IncludeAll,
            TrailingTriviaOption::Include,
        );
        return;
    };

    if source_file.kind(parent) == Kind::CatchClause {
        let open_paren = source_file.find_child_of_kind(parent, Kind::OpenParenToken);
        let close_paren = source_file.find_child_of_kind(parent, Kind::CloseParenToken);
        if let (Some(open_paren), Some(close_paren)) = (open_paren, close_paren) {
            t.delete_node_range(
                source_file,
                open_paren,
                close_paren,
                LeadingTriviaOption::IncludeAll,
                TrailingTriviaOption::Include,
            );
        }
        return;
    }

    let declaration_count = match source_file.arena().data(parent) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes.len(),
        _ => 0,
    };
    if declaration_count != 1 {
        // DEFER(phase-7): multi-declaration list deletion (`deleteNodeInList`).
        // blocked-by: format::indent::get_containing_list.
        delete_node(
            t,
            source_file,
            node,
            LeadingTriviaOption::IncludeAll,
            TrailingTriviaOption::Include,
        );
        return;
    }

    let Some(gp) = source_file.arena().parent(parent) else {
        delete_node(
            t,
            source_file,
            node,
            LeadingTriviaOption::IncludeAll,
            TrailingTriviaOption::Include,
        );
        return;
    };

    match source_file.kind(gp) {
        Kind::ForOfStatement | Kind::ForInStatement => {
            // DEFER(phase-7): rebind to `{}` via the node-insertion path.
            // blocked-by: printer.ChangeTrackerWriter / format_node_given_indentation.
            delete_node(
                t,
                source_file,
                parent,
                LeadingTriviaOption::IncludeAll,
                TrailingTriviaOption::Include,
            );
        }
        Kind::ForStatement => {
            delete_node(
                t,
                source_file,
                parent,
                LeadingTriviaOption::IncludeAll,
                TrailingTriviaOption::Include,
            );
        }
        Kind::VariableStatement => {
            let leading_trivia = if has_jsdoc_nodes() {
                LeadingTriviaOption::JSDoc
            } else {
                LeadingTriviaOption::StartLine
            };
            delete_node(
                t,
                source_file,
                gp,
                leading_trivia,
                TrailingTriviaOption::Include,
            );
        }
        _ => {
            delete_node(
                t,
                source_file,
                gp,
                LeadingTriviaOption::IncludeAll,
                TrailingTriviaOption::Include,
            );
        }
    }
}

/// Deletes `node` with the given trivia options (the core trivia-aware
/// deletion).
///
/// Mirrors Go's free `deleteNode` (note: this also deletes comments). Equivalent
/// to [`ChangeTracker::delete_node`]; kept as a module-local helper so the
/// dispatch above reads 1:1 against `delete.go`.
///
/// Side effects: appends an empty-text edit to `t`.
// Go: internal/ls/change/delete.go:deleteNode
fn delete_node(
    t: &mut ChangeTracker,
    source_file: &RcSourceFile,
    node: NodeId,
    leading_trivia: LeadingTriviaOption,
    trailing_trivia: TrailingTriviaOption,
) {
    let start = t.get_adjusted_start_position(source_file, node, leading_trivia, false);
    let end = t.get_adjusted_end_position(source_file, node, trailing_trivia);
    let key = file_name(source_file).to_string();
    t.add_edit(&key, tsgo_core::text::TextRange::new(start, end), "");
}

impl ChangeTracker {
    /// Finds the first non-whitespace byte offset in `node`'s leading trivia.
    ///
    /// Mirrors Go's `startPositionToDeleteNodeInList`. Used by the deferred
    /// list-deletion / trailing-comma paths (see module docs), so it is not yet
    /// reached in production.
    ///
    /// Side effects: none (pure; reads the nav context).
    // Go: internal/ls/change/delete.go:startPositionToDeleteNodeInList
    #[allow(dead_code)]
    pub(crate) fn start_position_to_delete_node_in_list(
        &self,
        source_file: &RcSourceFile,
        node: NodeId,
    ) -> i32 {
        let start = self.get_adjusted_start_position(
            source_file,
            node,
            LeadingTriviaOption::IncludeAll,
            false,
        );
        skip_trivia_ex(
            source_file.text(),
            start,
            Some(&SkipTriviaOptions {
                stop_after_line_break: false,
                stop_at_comments: true,
                ..Default::default()
            }),
        )
    }
}

/// Reports whether `node` carries JSDoc comments.
///
/// Always `false` in this port: the parser has not reparsed JSDoc, so no node
/// has cached JSDoc. Kept so the dispatch mirrors Go's structure.
///
/// Side effects: none (pure).
// Go: internal/ls/change/delete.go:hasJSDocNodes
// DEFER(phase-3): real JSDoc detection. blocked-by: JSDoc reparser (tsgo_parser).
fn has_jsdoc_nodes() -> bool {
    false
}

#[cfg(test)]
#[path = "delete_test.rs"]
mod tests;
