//! Port of Go `internal/transformers/moduletransforms/esmodule.go`: the ES module
//! transform for `--module es2015/esnext` output.
//!
//! # Scope (round 6u — reachable subset, no real ReferenceResolver)
//!
//! Under an ES module target the transform mostly **preserves** import/export
//! syntax (unlike CommonJS), so value imports/exports pass through unchanged.
//! The reachable structural pieces this round lands:
//!
//! * Source file entry/wiring (`new_es_module_transformer` + the
//!   `visitSourceFile` guard `IsDeclarationFile || !(IsExternalModule ||
//!   isolatedModules)` → return unchanged).
//! * `export = x` elision: illegal under `--module es2015/esnext`, so it is
//!   removed (Go elides unless `--module preserve`).
//! * `import x = require("m")` elision: under an ES module target (emit module
//!   kind `< Node16`) `import =` is removed.
//! * `createEmptyImports`: when the result is still an external module (the
//!   original carried an external-module indicator), the emit module kind is not
//!   `preserve`, and no indicator statement remains, an empty `export {};` is
//!   appended (so the file stays a module).
//! * `export * as ns from "m"` namespace re-export rewrite (round 6ab): legal
//!   under `--module esnext` (preserved verbatim) but not `--module es2015`,
//!   where it is rewritten to a namespace import bound to a generated name
//!   (`new_generated_name_for_node(ns)` → `ns_1`) plus a re-export of that name
//!   (`export { ns_1 as ns }`, or `export default ns_1` when the name is
//!   `default`).
//!
//! # Deferred (DEFER(P5))
//!
//! * **Type-only import elision** (`import type` / unreferenced type-only
//!   imports). blocked-by: checker `EmitResolver` (the reachable subset cannot
//!   know which imports are value-used).
//! * **Scope-correct reference rewriting**. blocked-by: a real
//!   `ReferenceResolver` (checker `resolveName`/`EmitResolver`).
//! * `--module preserve` `export =` → `module.exports = e` form,
//!   `--rewriteRelativeImportExtensions` specifier rewriting, dynamic
//!   `import()` rewriting, `import x = require()` → synchronous `require`
//!   helper for Node16+, and the external-helpers (`tslib`) import injection.

use crate::{new_transformer, TransformOptions, Transformer};
use tsgo_ast::{Kind, ModifierFlags, NodeArena, NodeData, NodeId, NodeList};
use tsgo_core::compileroptions::{CompilerOptions, ModuleKind};
use tsgo_printer::EmitContext;

/// Builds a [`Transformer`] for ES module output (`--module es2015/esnext`).
///
/// # Examples
/// ```
/// use tsgo_transformers::{moduletransforms::esmodule::new_es_module_transformer, TransformOptions};
/// let _tx = new_es_module_transformer(&TransformOptions::default());
/// ```
///
/// Side effects: allocates a transformer over the shared context.
// Go: internal/transformers/moduletransforms/esmodule.go:NewESModuleTransformer
pub fn new_es_module_transformer(opt: &TransformOptions) -> Transformer {
    let compiler_options = opt.compiler_options.clone();
    new_transformer(
        Box::new(move |ec: &mut EmitContext, node: NodeId| {
            es_module_visit(ec, &compiler_options, node)
        }),
        opt.context.clone(),
    )
}

/// Visits a node, dispatching the source file to the ES module transform.
///
/// Side effects: see [`new_es_module_transformer`].
// Go: internal/transformers/moduletransforms/esmodule.go:ESModuleTransformer.visit
fn es_module_visit(ec: &mut EmitContext, options: &CompilerOptions, node: NodeId) -> NodeId {
    if ec.arena().kind(node) == Kind::SourceFile {
        return transform_es_module(ec, options, node);
    }
    node
}

/// Transforms a source file for ES module output. Under `--module es2015/esnext`
/// import/export syntax is preserved, so value imports/exports pass through
/// unchanged; `export =` / `import =` are elided and a trailing empty
/// `export {};` keeps an otherwise-emptied module a module.
///
/// Side effects: rebuilds the source file (allocates statement/source nodes).
// Go: internal/transformers/moduletransforms/esmodule.go:ESModuleTransformer.visitSourceFile
fn transform_es_module(ec: &mut EmitContext, options: &CompilerOptions, node: NodeId) -> NodeId {
    let (
        file_name,
        script_kind,
        language_variant,
        statements,
        end_of_file_token,
        is_declaration_file,
        is_external_module,
    ) = match ec.arena().data(node) {
        NodeData::SourceFile(d) => (
            d.file_name.clone(),
            d.script_kind,
            d.language_variant,
            d.statements.clone(),
            d.end_of_file_token,
            d.is_declaration_file,
            d.external_module_indicator.is_some(),
        ),
        _ => unreachable!("kind checked by caller"),
    };

    // visitSourceFile guard: a declaration file, or a non-module file (no
    // external-module indicator) without `--isolatedModules`, is returned
    // unchanged.
    if is_declaration_file || !(is_external_module || options.get_isolated_modules()) {
        return node;
    }

    let emit_module_kind = options.get_emit_module_kind();
    let mut out: Vec<NodeId> = Vec::with_capacity(statements.nodes.len());
    for &statement in &statements.nodes {
        match ec.arena().kind(statement) {
            Kind::ExportAssignment if export_assignment_is_export_equals(ec.arena(), statement) => {
                // `export = x` is illegal with `--module es6`; elide it (the
                // `--module preserve` `module.exports = e` form is deferred).
                if emit_module_kind == ModuleKind::Preserve {
                    out.push(statement);
                }
            }
            Kind::ImportEqualsDeclaration
                if (emit_module_kind as i32) < (ModuleKind::Node16 as i32) =>
            {
                // `import x = require("m")` is not legal in an ES module target
                // below Node16; elide it (the Node16+ synchronous `require`
                // form is deferred).
            }
            Kind::ExportDeclaration
                if (options.module as i32) <= (ModuleKind::Es2015 as i32)
                    && export_is_namespace_reexport(ec.arena(), statement) =>
            {
                // `export * as ns from "m"` is legal under `--module esnext` but
                // not `--module es2015`, so only the latter rewrites it to a
                // namespace import bound to a generated name plus a named
                // re-export of that name.
                let (import_decl, export_decl) = rewrite_namespace_reexport(ec, statement);
                out.push(import_decl);
                out.push(export_decl);
            }
            _ => out.push(statement),
        }
    }

    // createEmptyImports: keep an external module a module when nothing in the
    // rebuilt statement list still marks it as one.
    if is_external_module
        && emit_module_kind != ModuleKind::Preserve
        && !out
            .iter()
            .any(|&s| statement_is_external_module_indicator(ec.arena(), s))
    {
        out.push(make_empty_imports(ec));
    }

    ec.arena_mut().new_source_file(
        &file_name,
        script_kind,
        language_variant,
        NodeList::new(out),
        end_of_file_token,
    )
}

/// Reports whether `node` is an `export = x` (rather than `export default x`).
///
/// Side effects: none (reads the arena).
fn export_assignment_is_export_equals(arena: &NodeArena, node: NodeId) -> bool {
    matches!(arena.data(node), NodeData::ExportAssignment(d) if d.is_export_equals)
}

/// Reports whether `statement` marks the file as an external module: any
/// import/`import =`/export declaration, an `export =`/`export default`, or a
/// statement carrying the `export` modifier.
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:IsExternalModuleIndicator
fn statement_is_external_module_indicator(arena: &NodeArena, statement: NodeId) -> bool {
    match arena.kind(statement) {
        Kind::ImportDeclaration
        | Kind::ImportEqualsDeclaration
        | Kind::ExportDeclaration
        | Kind::ExportAssignment => true,
        _ => statement_has_export_modifier(arena, statement),
    }
}

/// Reports whether `statement` carries the `export` modifier.
///
/// Side effects: none (reads the arena).
// Go: internal/ast/utilities.go:HasSyntacticModifier(ModifierFlagsExport)
fn statement_has_export_modifier(arena: &NodeArena, statement: NodeId) -> bool {
    let modifiers = match arena.data(statement) {
        NodeData::VariableStatement(d) => d.modifiers.clone(),
        NodeData::FunctionDeclaration(d) => d.modifiers.clone(),
        NodeData::ClassDeclaration(d) => d.modifiers.clone(),
        NodeData::InterfaceDeclaration(d) => d.modifiers.clone(),
        NodeData::TypeAliasDeclaration(d) => d.modifiers.clone(),
        NodeData::EnumDeclaration(d) => d.modifiers.clone(),
        NodeData::ModuleDeclaration(d) => d.modifiers.clone(),
        _ => return false,
    };
    modifiers
        .as_ref()
        .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::EXPORT))
}

/// Reports whether `statement` is an `export * as ns from "m"` declaration (a
/// re-export whose export clause is a namespace export).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/moduletransforms/esmodule.go:visitExportDeclaration
// (ModuleSpecifier != nil && IsNamespaceExport(ExportClause))
fn export_is_namespace_reexport(arena: &NodeArena, statement: NodeId) -> bool {
    match arena.data(statement) {
        NodeData::ExportDeclaration(d) => {
            d.module_specifier.is_some()
                && d.export_clause
                    .is_some_and(|c| arena.kind(c) == Kind::NamespaceExport)
        }
        _ => false,
    }
}

/// Rewrites `export * as ns from "m"` into a namespace import bound to a
/// generated name (`new_generated_name_for_node(ns)` → `ns_1`) followed by a
/// named re-export `export { ns_1 as ns }`.
///
/// Side effects: allocates the synthesized name plus the import/export nodes.
// Go: internal/transformers/moduletransforms/esmodule.go:visitExportDeclaration
fn rewrite_namespace_reexport(ec: &mut EmitContext, statement: NodeId) -> (NodeId, NodeId) {
    let (module_specifier, export_clause, attributes) = match ec.arena().data(statement) {
        NodeData::ExportDeclaration(d) => (
            d.module_specifier.expect("checked by caller"),
            d.export_clause.expect("checked by caller"),
            d.attributes,
        ),
        _ => unreachable!("kind checked by caller"),
    };
    let old_identifier = match ec.arena().data(export_clause) {
        NodeData::NamespaceExport(d) => d.name,
        _ => unreachable!("namespace export checked by caller"),
    };
    let is_export_namespace_as_default = ec.arena().text(old_identifier) == "default";
    let synth_name = ec.factory().new_generated_name_for_node(old_identifier);

    let namespace_import = ec.arena_mut().new_namespace_import(synth_name);
    let import_clause =
        ec.arena_mut()
            .new_import_clause(Kind::Unknown, None, Some(namespace_import));
    let import_decl = ec.arena_mut().new_import_declaration(
        None,
        Some(import_clause),
        module_specifier,
        attributes,
    );

    // `export * as default from "m"` re-exports the namespace as the default
    // export (`export default <gen>`); any other name uses a named re-export
    // (`export { <gen> as ns }`).
    let export_decl = if is_export_namespace_as_default {
        ec.arena_mut()
            .new_export_assignment(None, false, None, synth_name)
    } else {
        let export_specifier =
            ec.arena_mut()
                .new_export_specifier(false, Some(synth_name), old_identifier);
        let named_exports = ec
            .arena_mut()
            .new_named_exports(NodeList::new(vec![export_specifier]));
        ec.arena_mut()
            .new_export_declaration(None, false, Some(named_exports), None, None)
    };

    (import_decl, export_decl)
}

/// Builds an empty `export {};` declaration (Go's `createEmptyImports`).
///
/// Side effects: pushes the named-exports/export-declaration nodes.
// Go: internal/transformers/moduletransforms/utilities.go:createEmptyImports
fn make_empty_imports(ec: &mut EmitContext) -> NodeId {
    let arena = ec.arena_mut();
    let named_exports = arena.new_named_exports(NodeList::new(Vec::new()));
    arena.new_export_declaration(None, false, Some(named_exports), None, None)
}

#[cfg(test)]
#[path = "esmodule_test.rs"]
mod tests;
