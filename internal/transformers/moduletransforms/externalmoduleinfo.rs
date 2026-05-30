//! Port of Go `internal/transformers/moduletransforms/externalmoduleinfo.go`:
//! the shared, structural analysis of a module's imports and exports, consumed
//! by the CommonJS/ESM transforms.
//!
//! # Scope (round 6e)
//!
//! Collects the **structural** facts reachable from the parsed AST without the
//! checker's reference resolver: the external import/re-export declarations,
//! the exported names, whether the module has an `export *`, and an `export =`.
//! The resolver-dependent classification in Go's
//! `addExportedNamesForExportDeclaration` (function-vs-binding via
//! `GetReferencedImportDeclaration`) is deferred — see `moduletransforms/mod.rs`.

use tsgo_ast::{Kind, ModifierFlags, NodeArena, NodeData, NodeId};

/// The structural import/export facts of a module.
///
/// Side effects: none (a value collected from the AST).
// Go: internal/transformers/moduletransforms/externalmoduleinfo.go:externalModuleInfo
#[derive(Debug, Default, Clone)]
pub struct ExternalModuleInfo {
    /// `import`/`import =`/re-exporting `export ... from` declarations (imports
    /// and re-exports of other external modules), in source order.
    pub external_imports: Vec<NodeId>,
    /// All exported name nodes (local and re-exported), excluding locally
    /// exported function declarations (which Go tracks separately).
    pub exported_names: Vec<NodeId>,
    /// Whether the module contains an `export * from "mod"`.
    pub has_export_stars_to_export_values: bool,
    /// The `export = x` assignment, if one is present.
    pub export_equals: Option<NodeId>,
}

/// Collects [`ExternalModuleInfo`] by scanning the top-level statements of
/// `source_file`.
///
/// # Examples
/// ```
/// use tsgo_transformers::moduletransforms::externalmoduleinfo::collect_external_module_info;
/// // (constructed via the parser in tests)
/// let _ = collect_external_module_info;
/// ```
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/moduletransforms/externalmoduleinfo.go:collectExternalModuleInfo
pub fn collect_external_module_info(arena: &NodeArena, source_file: NodeId) -> ExternalModuleInfo {
    let mut info = ExternalModuleInfo::default();
    let statements = match arena.data(source_file) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => return info,
    };
    for &node in &statements {
        match arena.kind(node) {
            Kind::ImportDeclaration => {
                // import "mod" / import x from "mod" / import * as x / import { x }
                info.external_imports.push(node);
            }
            Kind::ExportDeclaration => {
                let (export_clause, module_specifier) = match arena.data(node) {
                    NodeData::ExportDeclaration(d) => (d.export_clause, d.module_specifier),
                    _ => continue,
                };
                if module_specifier.is_some() {
                    // export * from "mod" / export * as ns from "mod" / export { x } from "mod"
                    info.external_imports.push(node);
                    if export_clause.is_none() {
                        // export * from "mod"
                        info.has_export_stars_to_export_values = true;
                    } else {
                        add_exported_names_for_export_clause(arena, export_clause, &mut info);
                    }
                } else {
                    // export { x }
                    add_exported_names_for_export_clause(arena, export_clause, &mut info);
                }
            }
            Kind::ExportAssignment => {
                // export = x  (export default is `is_export_equals == false`)
                if let NodeData::ExportAssignment(d) = arena.data(node) {
                    if d.is_export_equals && info.export_equals.is_none() {
                        info.export_equals = Some(node);
                    }
                }
            }
            Kind::VariableStatement => {
                // export const x = 1, y = 2
                let (modifiers, declaration_list) = match arena.data(node) {
                    NodeData::VariableStatement(d) => (d.modifiers.clone(), d.declaration_list),
                    _ => continue,
                };
                if modifiers
                    .as_ref()
                    .is_some_and(|m| m.modifier_flags.contains(ModifierFlags::EXPORT))
                {
                    let declarations = match arena.data(declaration_list) {
                        NodeData::VariableDeclarationList(d) => d.declarations.nodes.clone(),
                        _ => continue,
                    };
                    for declaration in declarations {
                        collect_exported_variable_name(arena, declaration, &mut info);
                    }
                }
            }
            _ => {}
        }
    }
    info
}

/// Adds the exported names declared by a `NamedExports` clause (`export { a, b
/// as c }`) to `info`, de-duplicating by name text.
///
/// The resolver-dependent classification (re-export to a local function vs
/// binding) is deferred; this collects the structural specifier names.
///
/// Side effects: appends to `info.exported_names`.
// Go: internal/transformers/moduletransforms/externalmoduleinfo.go:addExportedNamesForExportDeclaration
fn add_exported_names_for_export_clause(
    arena: &NodeArena,
    export_clause: Option<NodeId>,
    info: &mut ExternalModuleInfo,
) {
    let Some(clause) = export_clause else {
        return;
    };
    let elements = match arena.data(clause) {
        NodeData::NamedExports(d) => d.elements.nodes.clone(),
        _ => return,
    };
    for specifier in elements {
        let name = match arena.data(specifier) {
            NodeData::ExportSpecifier(d) => d.name,
            _ => continue,
        };
        add_unique_exported_name(arena, name, info);
    }
}

/// Collects the exported name of an `export const` variable declaration with an
/// identifier binding. Binding patterns (`export const { a } = o`) are deferred.
///
/// Side effects: appends to `info.exported_names`.
// Go: internal/transformers/moduletransforms/externalmoduleinfo.go:collectExportedVariableInfo
fn collect_exported_variable_name(
    arena: &NodeArena,
    declaration: NodeId,
    info: &mut ExternalModuleInfo,
) {
    let name = match arena.data(declaration) {
        NodeData::VariableDeclaration(d) => d.name,
        _ => return,
    };
    if arena.kind(name) == Kind::Identifier {
        add_unique_exported_name(arena, name, info);
    }
}

/// Adds `name` to `info.exported_names`, de-duplicating by name text (Go's
/// `addUniqueExport`).
///
/// Side effects: appends to `info.exported_names`.
fn add_unique_exported_name(arena: &NodeArena, name: NodeId, info: &mut ExternalModuleInfo) {
    if !info
        .exported_names
        .iter()
        .any(|&n| arena.text(n) == arena.text(name))
    {
        info.exported_names.push(name);
    }
}

#[cfg(test)]
#[path = "externalmoduleinfo_test.rs"]
mod tests;
