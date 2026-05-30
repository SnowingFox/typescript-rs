//! Unit tests for the reference-resolver shell.

use super::*;
use tsgo_ast::NodeId;

// Go: internal/binder/referenceresolver.go:NewReferenceResolver
#[test]
fn new_reference_resolver_has_no_hooks() {
    let r = new_reference_resolver(ReferenceResolverHooks::default());
    assert!(!r.hooks_installed());
}

// Go: internal/binder/referenceresolver.go:ReferenceResolver (deferred queries)
#[test]
fn deferred_queries_report_unresolved() {
    let r = new_reference_resolver(ReferenceResolverHooks::default());
    assert_eq!(r.get_referenced_import_declaration(NodeId(0)), None);
    assert_eq!(r.get_referenced_value_declaration(NodeId(0)), None);
    assert!(r.get_referenced_value_declarations(NodeId(0)).is_empty());
    assert_eq!(r.get_element_access_expression_name(NodeId(0)), None);
}
