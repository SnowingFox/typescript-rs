//! Port of Go `internal/ls/callhierarchy.go`: the call-hierarchy feature.
//!
//! Go's call-hierarchy feature provides three LSP operations:
//! - `textDocument/prepareCallHierarchy` resolves the call hierarchy item at a
//!   position ([`resolveCallHierarchyDeclaration`] → item).
//! - `callHierarchy/incomingCalls` finds callers of a declaration via the
//!   find-all-references machinery.
//! - `callHierarchy/outgoingCalls` walks a declaration's body to find callees
//!   ([`collectCallSites`]).
//!
//! # Reachable subset
//!
//! This round ports the declaration-validation helpers and the pure utility
//! functions:
//! - [`is_named_expression`] / [`is_assigned_expression`] /
//!   [`is_variable_like`]: predicate helpers for classifying call-hierarchy
//!   declarations.
//! - [`is_possible_call_hierarchy_declaration`] /
//!   [`is_valid_call_hierarchy_declaration`]: determines whether an AST node
//!   can or does serve as a call-hierarchy declaration.
//! - [`CallSite`]: the outgoing-call site record.
//! - [`move_range_past_modifiers`]: adjusts a range to skip past modifiers.
//!
//! DEFER(phase-7-ls): the full `resolveCallHierarchyDeclaration` (requires
//! `checker.GetSymbolAtLocation` + alias resolution), `createCallHierarchyItem`
//! (requires `getSymbolKindFromNode`, `Converters`, `lsproto::CallHierarchyItem`),
//! `getIncomingCalls` (requires `handleCrossProject` + full FAR), `getOutgoingCalls`
//! / `collectCallSites` (requires `checker` + the full `findImplementation`), and
//! `getCallHierarchyItemName` / `getCallHierarchyItemContainerName` (require
//! `printer::Write` and `checker::SymbolToString`).

use tsgo_core::text::TextRange;

/// The kind of node that qualifies as a call-hierarchy item, used for
/// classification.
// Go: determined by kind checks in callhierarchy.go
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallHierarchyItemKind {
    SourceFile,
    ModuleDeclaration,
    FunctionDeclaration,
    FunctionExpression,
    ClassDeclaration,
    ClassExpression,
    ClassStaticBlock,
    MethodDeclaration,
    MethodSignature,
    GetAccessor,
    SetAccessor,
    ArrowFunction,
}

/// A record of an outgoing call site found during body traversal.
///
/// Side effects: none (a plain data record).
// Go: internal/ls/callhierarchy.go:callSite
#[derive(Clone, Debug)]
pub struct CallSite {
    /// The file-level byte range of the call expression target.
    pub text_range: TextRange,
    /// A description of the call target for display.
    pub target_description: String,
}

/// Returns `true` for AST node kinds that are function or class expressions
/// with an identifier name.
///
/// Side effects: none (pure predicate).
// Go: internal/ls/callhierarchy.go:isNamedExpression
pub fn is_named_expression(is_func_or_class_expr: bool, has_name: bool) -> bool {
    is_func_or_class_expr && has_name
}

/// Returns `true` for AST node kinds that are property or variable
/// declarations.
///
/// Side effects: none (pure predicate).
// Go: internal/ls/callhierarchy.go:isVariableLike
pub fn is_variable_like(is_property_decl: bool, is_variable_decl: bool) -> bool {
    is_property_decl || is_variable_decl
}

/// Returns `true` for unnamed function/arrow/class expressions assigned to a
/// `const` variable or class property.
///
/// Side effects: none (pure predicate).
// Go: internal/ls/callhierarchy.go:isAssignedExpression
pub fn is_assigned_expression(
    is_func_arrow_or_class_expr: bool,
    has_name: bool,
    parent_is_variable_like: bool,
    parent_initializer_is_node: bool,
    parent_name_is_identifier: bool,
    parent_is_const_or_property: bool,
) -> bool {
    if !is_func_arrow_or_class_expr {
        return false;
    }
    if has_name {
        return false;
    }
    parent_is_variable_like
        && parent_initializer_is_node
        && parent_name_is_identifier
        && parent_is_const_or_property
}

/// Returns `true` if the node kind could possibly be a call-hierarchy
/// declaration.
///
/// Side effects: none.
// Go: internal/ls/callhierarchy.go:isPossibleCallHierarchyDeclaration
pub fn is_possible_call_hierarchy_declaration(kind: CallHierarchyItemKind) -> bool {
    matches!(
        kind,
        CallHierarchyItemKind::SourceFile
            | CallHierarchyItemKind::ModuleDeclaration
            | CallHierarchyItemKind::FunctionDeclaration
            | CallHierarchyItemKind::FunctionExpression
            | CallHierarchyItemKind::ClassDeclaration
            | CallHierarchyItemKind::ClassExpression
            | CallHierarchyItemKind::ClassStaticBlock
            | CallHierarchyItemKind::MethodDeclaration
            | CallHierarchyItemKind::MethodSignature
            | CallHierarchyItemKind::GetAccessor
            | CallHierarchyItemKind::SetAccessor
    )
}

/// Returns `true` if the node is a valid call-hierarchy declaration: a source
/// file, a module declaration with an identifier name, or any function-like /
/// class / accessor declaration (including named/assigned expressions).
///
/// Side effects: none (pure predicate).
// Go: internal/ls/callhierarchy.go:isValidCallHierarchyDeclaration
pub fn is_valid_call_hierarchy_declaration(
    kind: CallHierarchyItemKind,
    module_name_is_identifier: bool,
    is_named_expr: bool,
    is_assigned_expr: bool,
) -> bool {
    match kind {
        CallHierarchyItemKind::SourceFile => true,
        CallHierarchyItemKind::ModuleDeclaration => module_name_is_identifier,
        CallHierarchyItemKind::FunctionDeclaration
        | CallHierarchyItemKind::ClassDeclaration
        | CallHierarchyItemKind::ClassStaticBlock
        | CallHierarchyItemKind::MethodDeclaration
        | CallHierarchyItemKind::MethodSignature
        | CallHierarchyItemKind::GetAccessor
        | CallHierarchyItemKind::SetAccessor => true,
        CallHierarchyItemKind::FunctionExpression | CallHierarchyItemKind::ClassExpression => {
            is_named_expr || is_assigned_expr
        }
        CallHierarchyItemKind::ArrowFunction => is_assigned_expr,
    }
}

/// Returns a text range that skips past any modifiers on the node.
///
/// If the node has modifiers, returns a range starting just after the last
/// modifier and ending at the node's end. Otherwise returns the original
/// `(pos, end)`.
///
/// Side effects: none.
// Go: internal/ls/callhierarchy.go:moveRangePastModifiers
pub fn move_range_past_modifiers(
    node_pos: i32,
    node_end: i32,
    last_modifier_end: Option<i32>,
) -> TextRange {
    match last_modifier_end {
        Some(mod_end) => TextRange::new(mod_end, node_end),
        None => TextRange::new(node_pos, node_end),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_named_expression_true_when_both() {
        assert!(is_named_expression(true, true));
    }

    #[test]
    fn is_named_expression_false_when_no_name() {
        assert!(!is_named_expression(true, false));
    }

    #[test]
    fn is_named_expression_false_when_not_expr() {
        assert!(!is_named_expression(false, true));
    }

    #[test]
    fn is_variable_like_cases() {
        assert!(is_variable_like(true, false));
        assert!(is_variable_like(false, true));
        assert!(is_variable_like(true, true));
        assert!(!is_variable_like(false, false));
    }

    #[test]
    fn is_possible_call_hierarchy_declaration_source_file() {
        assert!(is_possible_call_hierarchy_declaration(
            CallHierarchyItemKind::SourceFile
        ));
    }

    #[test]
    fn is_possible_call_hierarchy_declaration_arrow_function_excluded() {
        assert!(!is_possible_call_hierarchy_declaration(
            CallHierarchyItemKind::ArrowFunction
        ));
    }

    #[test]
    fn is_valid_source_file() {
        assert!(is_valid_call_hierarchy_declaration(
            CallHierarchyItemKind::SourceFile,
            false,
            false,
            false,
        ));
    }

    #[test]
    fn is_valid_module_needs_identifier_name() {
        assert!(is_valid_call_hierarchy_declaration(
            CallHierarchyItemKind::ModuleDeclaration,
            true,
            false,
            false,
        ));
        assert!(!is_valid_call_hierarchy_declaration(
            CallHierarchyItemKind::ModuleDeclaration,
            false,
            false,
            false,
        ));
    }

    #[test]
    fn is_valid_function_expression_needs_name_or_assignment() {
        assert!(!is_valid_call_hierarchy_declaration(
            CallHierarchyItemKind::FunctionExpression,
            false,
            false,
            false,
        ));
        assert!(is_valid_call_hierarchy_declaration(
            CallHierarchyItemKind::FunctionExpression,
            false,
            true,
            false,
        ));
        assert!(is_valid_call_hierarchy_declaration(
            CallHierarchyItemKind::FunctionExpression,
            false,
            false,
            true,
        ));
    }

    #[test]
    fn is_valid_method_always_valid() {
        assert!(is_valid_call_hierarchy_declaration(
            CallHierarchyItemKind::MethodDeclaration,
            false,
            false,
            false,
        ));
    }

    #[test]
    fn is_valid_arrow_needs_assignment() {
        assert!(!is_valid_call_hierarchy_declaration(
            CallHierarchyItemKind::ArrowFunction,
            false,
            false,
            false,
        ));
        assert!(is_valid_call_hierarchy_declaration(
            CallHierarchyItemKind::ArrowFunction,
            false,
            false,
            true,
        ));
    }

    #[test]
    fn move_range_past_modifiers_no_modifiers() {
        let r = move_range_past_modifiers(10, 50, None);
        assert_eq!(r.pos(), 10);
        assert_eq!(r.end(), 50);
    }

    #[test]
    fn move_range_past_modifiers_with_modifiers() {
        let r = move_range_past_modifiers(10, 50, Some(25));
        assert_eq!(r.pos(), 25);
        assert_eq!(r.end(), 50);
    }

    #[test]
    fn call_site_construction() {
        let site = CallSite {
            text_range: TextRange::new(5, 15),
            target_description: "myFunction".to_string(),
        };
        assert_eq!(site.text_range.pos(), 5);
        assert_eq!(site.text_range.end(), 15);
        assert_eq!(site.target_description, "myFunction");
    }

    #[test]
    fn is_assigned_expression_all_conditions_met() {
        assert!(is_assigned_expression(
            true,  // is func/arrow/class expr
            false, // no name
            true,  // parent is variable-like
            true,  // parent initializer is this node
            true,  // parent name is identifier
            true,  // parent is const or property
        ));
    }

    #[test]
    fn is_assigned_expression_has_name_rejects() {
        assert!(!is_assigned_expression(true, true, true, true, true, true));
    }

    #[test]
    fn is_assigned_expression_not_func_rejects() {
        assert!(!is_assigned_expression(false, true, true, true, true, true));
    }
}
