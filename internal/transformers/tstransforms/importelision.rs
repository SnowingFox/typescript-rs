//! Port of Go `internal/transformers/tstransforms/importelision.go`: the
//! `ImportElisionTransformer`, which drops imports/exports whose binding is not
//! referenced as a value.
//!
//! # Scope (round 6af)
//!
//! Wires the checker's scope-correct [`EmitResolver::is_referenced`] (via
//! [`EmitReferenceResolver`]) into the transform pipeline to elide unused
//! *value* imports, matching Go's elision shape:
//! * an [`ImportSpecifier`](tsgo_ast::Kind::ImportSpecifier) / namespace import
//!   whose binding is unreferenced is dropped;
//! * a [`NamedImports`](tsgo_ast::Kind::NamedImports) with no surviving
//!   specifiers is dropped;
//! * an [`ImportClause`](tsgo_ast::Kind::ImportClause) with neither a surviving
//!   default name nor named bindings is dropped, which elides the whole
//!   [`ImportDeclaration`](tsgo_ast::Kind::ImportDeclaration);
//! * a side-effect-only import (`import "m";`, no clause) is never elided.
//!
//! The elision is scope-correct, not a textual name match: a use shadowed by an
//! inner binding of the same name does not keep an outer import alive (Go's
//! `isReferencedAliasDeclaration` over the resolved symbol).
//!
//! Round 6ag added the export-specifier side (`ExportDeclaration` /
//! `NamedExports` / `ExportSpecifier`, via `is_value_alias_declaration`). Round
//! 6ah, unblocked by checker 4ap, adds the `ImportEqualsDeclaration` and
//! `ExportAssignment` arms:
//! * an external-module `import x = require("m")` is dropped unless its alias is
//!   referenced as a value (`is_referenced_alias_declaration`);
//! * an `export = <value>` is kept while a type-only `export = I` is dropped
//!   (`is_value_alias_declaration`).
//!
//! # Deferred (DEFER(P5))
//!
//! The entity-name `import x = a.b` form needs the resolver's
//! `IsTopLevelValueImportEqualsWithEntityName` query (checker 4ap DEFER'd it);
//! cross-module re-exports (`import { x } from "m"; export = x` /
//! `export { x }`) need `resolveExternalModuleSymbol`. The type-only-position
//! split (a *type*-only use not keeping a value import alive), `import "m";`
//! attribute visiting, and the `verbatimModuleSyntax` / `isolatedModules` /
//! const-enum policy variants are likewise deferred.
//! blocked-by: checker `IsTopLevelValueImportEqualsWithEntityName` +
//! `resolveExternalModuleSymbol` + `markLinkedReferences`.

use crate::{new_transformer, EmitReferenceResolver, TransformOptions, Transformer};
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, NodeList};
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] that elides unreferenced value imports from a source
/// file, consulting `resolver` (the checker's scope-correct reference query).
///
/// The resolver is an *additive* parameter rather than a [`TransformOptions`]
/// field; see [`EmitReferenceResolver`] for why.
///
/// # Examples
/// ```
/// use tsgo_transformers::{
///     tstransforms::importelision::new_import_elision_transformer, EmitReferenceResolver,
///     TransformOptions,
/// };
/// # fn demo(resolver: EmitReferenceResolver) {
/// let _tx = new_import_elision_transformer(&TransformOptions::default(), resolver);
/// # }
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/tstransforms/importelision.go:NewImportElisionTransformer
pub fn new_import_elision_transformer(
    opt: &TransformOptions,
    resolver: EmitReferenceResolver,
) -> Transformer {
    new_transformer(
        // The root SourceFile is never elided, so `None` cannot occur at the top.
        Box::new(move |ec: &mut EmitContext, node: NodeId| {
            import_elision_visit(ec.arena_mut(), &resolver, node).unwrap_or(node)
        }),
        opt.context.clone(),
    )
}

/// Elides unreferenced value imports from the subtree rooted at `node`. Returns
/// `Some(rebuilt)` for kept/rewritten nodes and `None` to elide the node from
/// its containing list (Go's `visit` returning `nil`).
///
/// Side effects: may push rebuilt nodes onto the arena.
// Go: internal/transformers/tstransforms/importelision.go:ImportElisionTransformer.visit
fn import_elision_visit(
    arena: &mut NodeArena,
    resolver: &EmitReferenceResolver,
    node: NodeId,
) -> Option<NodeId> {
    match arena.kind(node) {
        Kind::SourceFile => Some(visit_source_file(arena, resolver, node)),
        Kind::ImportEqualsDeclaration => visit_import_equals_declaration(arena, resolver, node),
        Kind::ImportDeclaration => visit_import_declaration(arena, resolver, node),
        Kind::ImportClause => visit_import_clause(arena, resolver, node),
        Kind::NamespaceImport => {
            // Elide an unused namespace import (`import * as ns from "m"`).
            should_emit_alias_declaration(resolver, node).then_some(node)
        }
        Kind::NamedImports => visit_named_imports(arena, resolver, node),
        Kind::ImportSpecifier => {
            // Elide a type-only or unused named import specifier.
            should_emit_alias_declaration(resolver, node).then_some(node)
        }
        Kind::ExportAssignment => {
            // Go: KindExportAssignment. This transformer never runs under
            // `verbatimModuleSyntax` (Go panics if it is set), so an `export =`
            // is elided unless it aliases a value: `function f() {}; export = f`
            // is kept (`is_value_alias_declaration` true), while a type-only
            // `interface I {}; export = I` is dropped (false).
            resolver.is_value_alias_declaration(node).then_some(node)
        }
        Kind::ExportDeclaration => visit_export_declaration(arena, resolver, node),
        Kind::NamedExports => visit_named_exports(arena, resolver, node),
        Kind::ExportSpecifier => {
            // Elide an export specifier that does not alias a value.
            resolver.is_value_alias_declaration(node).then_some(node)
        }
        // Other nodes are returned unchanged (Go's `default: return node`): the
        // transform only recurses into the source file and import/export
        // structure.
        _ => Some(node),
    }
}

/// Rebuilds the source file, dropping any top-level import declarations whose
/// bindings were all elided.
///
/// Side effects: pushes the rebuilt source-file node.
// Go: internal/transformers/tstransforms/importelision.go (KindSourceFile arm)
fn visit_source_file(
    arena: &mut NodeArena,
    resolver: &EmitReferenceResolver,
    node: NodeId,
) -> NodeId {
    let (file_name, script_kind, language_variant, statements, end_of_file_token) =
        match arena.data(node) {
            NodeData::SourceFile(d) => (
                d.file_name.clone(),
                d.script_kind,
                d.language_variant,
                d.statements.clone(),
                d.end_of_file_token,
            ),
            _ => unreachable!("kind checked by caller"),
        };
    let statements = arena.visit_nodes_removable(&statements, &mut |a, c| {
        import_elision_visit(a, resolver, c)
    });
    arena.new_source_file(
        &file_name,
        script_kind,
        language_variant,
        statements,
        end_of_file_token,
    )
}

/// Visits an `import x = ...` declaration. For the external-module form
/// (`import x = require("m")`), the declaration is dropped unless its alias is
/// referenced as a value (Go's `shouldEmitAliasDeclaration`, consuming the
/// checker's `is_referenced_alias_declaration`). The entity-name form
/// (`import x = a.b`) is kept unchanged (DEFER, see below).
///
/// Side effects: none (returns the original node id when kept).
// Go: internal/transformers/tstransforms/importelision.go (KindImportEqualsDeclaration arm)
fn visit_import_equals_declaration(
    arena: &mut NodeArena,
    resolver: &EmitReferenceResolver,
    node: NodeId,
) -> Option<NodeId> {
    if is_external_module_import_equals_declaration(arena, node) {
        // Go: `IsExternalModuleImportEqualsDeclaration` -> keep iff
        // `shouldEmitAliasDeclaration` (which reduces to
        // `isReferencedAliasDeclaration` outside JS files). 4ap made an unused
        // `import x = require("m")` report `is_referenced_alias_declaration` =
        // false (the binding's own name `x` is excluded from the reference
        // scan), so an unused external import-equals is now elided.
        if !resolver.is_referenced_alias_declaration(node) {
            return None;
        }
        // Kept: Go's `VisitEachChild` would rebuild identical children (the name
        // and `require(...)` reference carry no elidable import structure), so
        // the original node is returned unchanged, as the namespace-import arm
        // does.
        Some(node)
    } else {
        // DEFER(P5): the entity-name form (`import x = a.b`) needs Go's
        // `shouldEmitImportEqualsDeclaration` ->
        // `isTopLevelValueImportEqualsWithEntityName`, which checker 4ap left
        // DEFER'd; keep it unchanged for now.
        // blocked-by: checker `IsTopLevelValueImportEqualsWithEntityName`.
        Some(node)
    }
}

/// Reports whether `node` is an `import x = require("m")` (external-module form),
/// i.e. its module reference is an `ExternalModuleReference` rather than an
/// entity name.
///
/// Side effects: none (pure read over the arena).
// Go: internal/ast/utilities.go:IsExternalModuleImportEqualsDeclaration
fn is_external_module_import_equals_declaration(arena: &NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::ImportEqualsDeclaration(d) => {
            arena.kind(d.module_reference) == Kind::ExternalModuleReference
        }
        _ => false,
    }
}

/// Visits an import declaration, eliding it wholesale when its import clause's
/// bindings were all elided. A side-effect-only import (no clause) is kept.
///
/// Side effects: may push the rebuilt import declaration / clause nodes.
// Go: internal/transformers/tstransforms/importelision.go (KindImportDeclaration arm)
fn visit_import_declaration(
    arena: &mut NodeArena,
    resolver: &EmitReferenceResolver,
    node: NodeId,
) -> Option<NodeId> {
    let (modifiers, import_clause, module_specifier, attributes) = match arena.data(node) {
        NodeData::ImportDeclaration(d) => (
            d.modifiers.clone(),
            d.import_clause,
            d.module_specifier,
            d.attributes,
        ),
        _ => unreachable!("kind/data mismatch"),
    };
    // Do not elide a side-effect-only import declaration (`import "m";`).
    let Some(import_clause) = import_clause else {
        return Some(node);
    };
    let import_clause = import_elision_visit(arena, resolver, import_clause)?;
    Some(arena.new_import_declaration(modifiers, Some(import_clause), module_specifier, attributes))
}

/// Visits an import clause, keeping the default name only when referenced and
/// visiting the named bindings; returns `None` when nothing survives.
///
/// Side effects: may push the rebuilt import clause node.
// Go: internal/transformers/tstransforms/importelision.go (KindImportClause arm)
fn visit_import_clause(
    arena: &mut NodeArena,
    resolver: &EmitReferenceResolver,
    node: NodeId,
) -> Option<NodeId> {
    let (phase_modifier, name, named_bindings) = match arena.data(node) {
        NodeData::ImportClause(d) => (d.phase_modifier, d.name, d.named_bindings),
        _ => unreachable!("kind/data mismatch"),
    };
    let name = name.filter(|_| should_emit_alias_declaration(resolver, node));
    let named_bindings = named_bindings.and_then(|b| import_elision_visit(arena, resolver, b));
    if name.is_none() && named_bindings.is_none() {
        // All import bindings were elided.
        return None;
    }
    Some(arena.new_import_clause(phase_modifier, name, named_bindings))
}

/// Visits a named-imports clause, dropping elided specifiers; returns `None`
/// when no specifier survives.
///
/// Side effects: may push the rebuilt named-imports node.
// Go: internal/transformers/tstransforms/importelision.go (KindNamedImports arm)
fn visit_named_imports(
    arena: &mut NodeArena,
    resolver: &EmitReferenceResolver,
    node: NodeId,
) -> Option<NodeId> {
    let elements = match arena.data(node) {
        NodeData::NamedImports(d) => d.elements.clone(),
        _ => unreachable!("kind/data mismatch"),
    };
    let elements =
        arena.visit_nodes_removable(&elements, &mut |a, c| import_elision_visit(a, resolver, c));
    if elements.nodes.is_empty() {
        // All import specifiers were elided.
        return None;
    }
    // Rebuild with a fresh (undefined-range) list so the printer does not infer
    // a trailing comma from the original source span of the dropped specifiers.
    Some(arena.new_named_imports(NodeList::new(elements.nodes)))
}

/// Visits an export declaration, eliding it wholesale when its export clause's
/// specifiers were all elided. A bare re-export (`export * from "m"`, no export
/// clause) is kept.
///
/// Side effects: may push the rebuilt export declaration node.
// Go: internal/transformers/tstransforms/importelision.go (KindExportDeclaration arm)
fn visit_export_declaration(
    arena: &mut NodeArena,
    resolver: &EmitReferenceResolver,
    node: NodeId,
) -> Option<NodeId> {
    let (export_clause, module_specifier, attributes) = match arena.data(node) {
        NodeData::ExportDeclaration(d) => (d.export_clause, d.module_specifier, d.attributes),
        _ => unreachable!("kind/data mismatch"),
    };
    let export_clause = match export_clause {
        Some(clause) => {
            let Some(rebuilt) = import_elision_visit(arena, resolver, clause) else {
                // All export bindings were elided.
                return None;
            };
            Some(rebuilt)
        }
        None => None,
    };
    // Go's `UpdateExportDeclaration` drops the modifiers and `isTypeOnly` flag
    // (passes `nil` / `false`); the module specifier and attributes only carry
    // value/type structure the elision does not rewrite, so they pass through.
    Some(arena.new_export_declaration(None, false, export_clause, module_specifier, attributes))
}

/// Visits a named-exports clause, dropping elided specifiers; returns `None`
/// when no specifier survives.
///
/// Side effects: may push the rebuilt named-exports node.
// Go: internal/transformers/tstransforms/importelision.go (KindNamedExports arm)
fn visit_named_exports(
    arena: &mut NodeArena,
    resolver: &EmitReferenceResolver,
    node: NodeId,
) -> Option<NodeId> {
    let elements = match arena.data(node) {
        NodeData::NamedExports(d) => d.elements.clone(),
        _ => unreachable!("kind/data mismatch"),
    };
    let elements =
        arena.visit_nodes_removable(&elements, &mut |a, c| import_elision_visit(a, resolver, c));
    if elements.nodes.is_empty() {
        // All export specifiers were elided.
        return None;
    }
    // Rebuild with a fresh (undefined-range) list so the printer does not infer
    // a trailing comma from the original source span of the dropped specifiers.
    Some(arena.new_named_exports(NodeList::new(elements.nodes)))
}

/// Reports whether the alias declaration `node` should be emitted: Go's
/// `shouldEmitAliasDeclaration` = `IsInJSFile(node) || isReferencedAliasDeclaration(node)`.
///
/// Side effects: none (a read over the bound program).
// Go: internal/transformers/tstransforms/importelision.go:ImportElisionTransformer.shouldEmitAliasDeclaration
fn should_emit_alias_declaration(resolver: &EmitReferenceResolver, node: NodeId) -> bool {
    // DEFER(P5): the `IsInJSFile(node)` short-circuit (a `.js`/`.jsx` source
    // keeps every import) is not modelled; these tests parse `.ts` sources, so
    // the result reduces to the reference query. blocked-by: emit-context
    // `ParseNode` + JS-file flag threading.
    resolver.is_referenced(node)
}

#[cfg(test)]
#[path = "importelision_test.rs"]
mod tests;
