//! Node query/utility helpers (`set_parent_in_children`, `is_*` predicates).
//!
//! Go's `utilities.go` is ~417 functions; this phase ports the parent-wiring
//! helper used by deep clone plus a representative spread of kind predicates.
//! More are pulled in by upstream phases as their callers land.

use crate::{Kind, NodeArena, NodeId};

impl NodeArena {
    /// Sets the `parent` of every node in the subtree rooted at `root` to its
    /// enclosing node, recursively. The root's own parent is left unchanged.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::NodeArena;
    /// let mut arena = NodeArena::new();
    /// let a = arena.new_identifier("a");
    /// let b = arena.new_identifier("b");
    /// let qn = arena.new_qualified_name(a, b);
    /// arena.set_parent_in_children(qn);
    /// assert_eq!(arena.parent(a), Some(qn));
    /// assert_eq!(arena.parent(qn), None);
    /// ```
    ///
    /// Side effects: mutates the `parent` of descendant nodes.
    // Go: internal/ast/utilities.go:SetParentInChildren
    pub fn set_parent_in_children(&mut self, root: NodeId) {
        let mut children = Vec::new();
        self.for_each_child(root, &mut |c| {
            children.push(c);
            false
        });
        for child in children {
            self.set_parent(child, Some(root));
            self.set_parent_in_children(child);
        }
    }
}

/// Reports whether node `id` is an identifier.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsIdentifier
pub fn is_identifier(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::Identifier
}

/// Reports whether node `id` is a call expression.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsCallExpression
pub fn is_call_expression(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::CallExpression
}

/// Reports whether node `id` is a property access expression.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsPropertyAccessExpression
pub fn is_property_access_expression(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::PropertyAccessExpression
}

/// Reports whether node `id` is a string literal.
///
/// Side effects: none (pure).
// Go: internal/ast/ast_generated.go:IsStringLiteral
pub fn is_string_literal(arena: &NodeArena, id: NodeId) -> bool {
    arena.kind(id) == Kind::StringLiteral
}

#[cfg(test)]
#[path = "utilities_test.rs"]
mod tests;
