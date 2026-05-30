//! Reference resolution for the emit/transform phases.
//!
//! Ports the shape of Go `internal/binder/referenceresolver.go`. The concrete
//! `GetReferenced*` queries run during emit and depend on checker hooks, so
//! their bodies are deferred to a later phase; this module provides the trait,
//! the hook bag, and a constructor returning a placeholder implementation.

use tsgo_ast::NodeId;

/// Resolves identifier references to their declarations/containers during emit.
///
/// Side effects: none yet (queries are deferred).
// Go: internal/binder/referenceresolver.go:ReferenceResolver
pub trait ReferenceResolver {
    /// Returns the export container a referenced identifier resolves to.
    // Go: referenceresolver.go:GetReferencedExportContainer
    fn get_referenced_export_container(&self, node: NodeId, prefix_locals: bool) -> Option<NodeId>;

    /// Returns the import declaration a referenced identifier resolves to.
    // Go: referenceresolver.go:GetReferencedImportDeclaration
    fn get_referenced_import_declaration(&self, node: NodeId) -> Option<NodeId>;

    /// Returns the value declaration a referenced identifier resolves to.
    // Go: referenceresolver.go:GetReferencedValueDeclaration
    fn get_referenced_value_declaration(&self, node: NodeId) -> Option<NodeId>;

    /// Returns all value declarations a referenced identifier resolves to.
    // Go: referenceresolver.go:GetReferencedValueDeclarations
    fn get_referenced_value_declarations(&self, node: NodeId) -> Vec<NodeId>;

    /// Returns the constant name of an element-access expression, if any.
    // Go: referenceresolver.go:GetElementAccessExpressionName
    fn get_element_access_expression_name(&self, expression: NodeId) -> Option<String>;

    /// Returns the member value declaration a node resolves to.
    // Go: referenceresolver.go:GetReferencedMemberValueDeclaration
    fn get_referenced_member_value_declaration(&self, node: NodeId) -> Option<NodeId>;
}

/// The checker-provided hooks a [`ReferenceResolver`] delegates to.
///
/// The concrete hook closures are wired up in the checker phase; this is the
/// structural placeholder.
///
/// Side effects: none (data holder).
// Go: internal/binder/referenceresolver.go:ReferenceResolverHooks
#[derive(Default)]
pub struct ReferenceResolverHooks {
    /// Whether checker hooks have been installed (placeholder marker).
    pub installed: bool,
}

/// The default [`ReferenceResolver`] implementation.
///
/// Side effects: none yet (queries are deferred).
// Go: internal/binder/referenceresolver.go:referenceResolver
pub struct ReferenceResolverImpl {
    hooks: ReferenceResolverHooks,
}

/// Creates a reference resolver from the given `hooks`.
///
/// # Examples
/// ```
/// use tsgo_binder::{new_reference_resolver, ReferenceResolver, ReferenceResolverHooks};
/// use tsgo_ast::NodeId;
/// let r = new_reference_resolver(ReferenceResolverHooks::default());
/// assert_eq!(r.get_referenced_import_declaration(NodeId(0)), None);
/// ```
///
/// Side effects: none.
// Go: internal/binder/referenceresolver.go:NewReferenceResolver
pub fn new_reference_resolver(hooks: ReferenceResolverHooks) -> ReferenceResolverImpl {
    ReferenceResolverImpl { hooks }
}

impl ReferenceResolverImpl {
    /// Reports whether checker hooks have been installed.
    ///
    /// Side effects: none (pure).
    pub fn hooks_installed(&self) -> bool {
        self.hooks.installed
    }
}

impl ReferenceResolver for ReferenceResolverImpl {
    // The bodies require checker hooks and are filled in during the emit phase;
    // until then every query reports "not resolved".
    fn get_referenced_export_container(
        &self,
        _node: NodeId,
        _prefix_locals: bool,
    ) -> Option<NodeId> {
        None
    }

    fn get_referenced_import_declaration(&self, _node: NodeId) -> Option<NodeId> {
        None
    }

    fn get_referenced_value_declaration(&self, _node: NodeId) -> Option<NodeId> {
        None
    }

    fn get_referenced_value_declarations(&self, _node: NodeId) -> Vec<NodeId> {
        Vec::new()
    }

    fn get_element_access_expression_name(&self, _expression: NodeId) -> Option<String> {
        None
    }

    fn get_referenced_member_value_declaration(&self, _node: NodeId) -> Option<NodeId> {
        None
    }
}

#[cfg(test)]
#[path = "referenceresolver_test.rs"]
mod tests;
