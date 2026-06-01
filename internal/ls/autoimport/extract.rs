//! Reachable AST-walking export extraction.
//!
//! This is the **reachable analog** of Go `internal/ls/autoimport/extract.go`.
//! Go extracts exports through the type checker: it walks `file.Symbol.Exports`,
//! resolves aliases (`tryResolveSymbol` / `GetAliasedSymbol`), follows
//! `export *` through `GetExportsOfModule`, and merges ambient-module
//! declarations. None of that is available without `tsgo_checker`, so this port
//! walks the **top-level `export` statements of the parsed AST directly** and
//! produces an [`Export`] per syntactic export. It covers the cases the task
//! slices exercise: `export const/let/var`, `export function`, `export class`,
//! `export interface/type/enum/namespace`, `export { x }` / `export { x as y }`,
//! `export default ...`, `export = ...`, and recognises `export * from "..."`.
//!
//! DEFER(phase-checker): alias resolution (the real kind/flags/target of an
//! `export { x }` re-export), cross-module `export *` member enumeration, and
//! ambient-module merging all need the checker's symbol graph.
//! blocked-by: `tsgo_checker`.

use tsgo_ast::symbol::{INTERNAL_SYMBOL_NAME_DEFAULT, INTERNAL_SYMBOL_NAME_EXPORT_EQUALS};
use tsgo_ast::{Kind, ModifierFlags, NodeArena, NodeData, NodeFlags, NodeId, SymbolFlags};
use tsgo_ls_lsutil::{module_specifier_to_valid_identifier, ScriptElementKind};
use tsgo_tspath::Path;

use crate::export::{is_unusable_name, Export, ExportId, ExportSyntax, ModuleId};

/// Extracts the indexable exports from a parsed source file by walking its
/// top-level statements. `path` is the file's canonical [`Path`] (the export's
/// `ModuleId`).
///
/// Exports whose written name is unusable (e.g. a bare `export *`, or an
/// anonymous `export default <expr>` with no derivable identifier) are dropped,
/// matching Go's `createExport` returning nil for `isUnusableName`.
///
/// Side effects: none (reads the arena only).
// Go: internal/ls/autoimport/extract.go:exportExtractor.extractFromFile
pub fn extract_top_level_exports(
    arena: &NodeArena,
    source_file: NodeId,
    path: &Path,
) -> Vec<Export> {
    let NodeData::SourceFile(sf) = arena.data(source_file) else {
        return Vec::new();
    };
    let file_name = sf.file_name.clone();
    let statements = sf.statements.nodes.clone();
    let ext = Extractor {
        arena,
        module_id: ModuleId(path.0.clone()),
        file_name,
        path: path.clone(),
    };
    let mut exports = Vec::new();
    for stmt in statements {
        ext.extract_statement(stmt, &mut exports);
    }
    exports
}

/// Per-file extraction context (the file's module id / name / path).
struct Extractor<'a> {
    arena: &'a NodeArena,
    module_id: ModuleId,
    file_name: String,
    path: Path,
}

impl Extractor<'_> {
    /// Builds an [`Export`] for one exported name in this file, dropping it if
    /// the name is unusable (mirrors Go `createExport`).
    fn push_export(
        &self,
        exports: &mut Vec<Export>,
        export_name: &str,
        local_name: &str,
        syntax: ExportSyntax,
        flags: SymbolFlags,
        kind: ScriptElementKind,
    ) {
        let export = Export {
            id: ExportId {
                module_id: self.module_id.clone(),
                export_name: export_name.to_string(),
            },
            module_file_name: self.file_name.clone(),
            syntax,
            flags,
            local_name: local_name.to_string(),
            script_element_kind: kind,
            path: self.path.clone(),
            ..Default::default()
        };
        if is_unusable_name(&export.name()) {
            return;
        }
        exports.push(export);
    }

    /// Returns the identifier text of `name`, or `None` if it is not a plain
    /// identifier (binding patterns / computed names are deferred).
    fn identifier_text(&self, name: NodeId) -> Option<String> {
        if self.arena.kind(name) == Kind::Identifier {
            Some(self.arena.text(name).to_string())
        } else {
            None
        }
    }

    /// The display name to use for a default-like export: the declaration's own
    /// identifier when present, else a file-name-derived identifier (Go's last
    /// resort via `ModuleSpecifierToValidIdentifier`).
    fn default_local_name(&self, name: Option<&str>) -> String {
        match name {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => module_specifier_to_valid_identifier(&self.file_name, false),
        }
    }

    fn extract_statement(&self, stmt: NodeId, exports: &mut Vec<Export>) {
        match self.arena.data(stmt) {
            NodeData::VariableStatement(data) => {
                if !modifiers_contain(&data.modifiers, ModifierFlags::EXPORT) {
                    return;
                }
                self.extract_variable_statement(data.declaration_list, exports);
            }
            NodeData::FunctionDeclaration(data) => {
                if !modifiers_contain(&data.modifiers, ModifierFlags::EXPORT) {
                    return;
                }
                let name = data.name.and_then(|n| self.identifier_text(n));
                self.push_named_or_default(
                    exports,
                    name,
                    modifiers_contain(&data.modifiers, ModifierFlags::DEFAULT),
                    SymbolFlags::FUNCTION,
                    ScriptElementKind::FunctionElement,
                );
            }
            NodeData::ClassDeclaration(data) => {
                if !modifiers_contain(&data.modifiers, ModifierFlags::EXPORT) {
                    return;
                }
                let name = data.name.and_then(|n| self.identifier_text(n));
                self.push_named_or_default(
                    exports,
                    name,
                    modifiers_contain(&data.modifiers, ModifierFlags::DEFAULT),
                    SymbolFlags::CLASS,
                    ScriptElementKind::ClassElement,
                );
            }
            NodeData::InterfaceDeclaration(data) => {
                if let Some(name) = self.exported_decl_name(&data.modifiers, data.name) {
                    self.push_export(
                        exports,
                        &name,
                        "",
                        ExportSyntax::Modifier,
                        SymbolFlags::INTERFACE,
                        ScriptElementKind::InterfaceElement,
                    );
                }
            }
            NodeData::TypeAliasDeclaration(data) => {
                if let Some(name) = self.exported_decl_name(&data.modifiers, Some(data.name)) {
                    self.push_export(
                        exports,
                        &name,
                        "",
                        ExportSyntax::Modifier,
                        SymbolFlags::TYPE_ALIAS,
                        ScriptElementKind::TypeElement,
                    );
                }
            }
            NodeData::EnumDeclaration(data) => {
                if let Some(name) = self.exported_decl_name(&data.modifiers, Some(data.name)) {
                    let flags = if modifiers_contain(&data.modifiers, ModifierFlags::CONST) {
                        SymbolFlags::CONST_ENUM
                    } else {
                        SymbolFlags::REGULAR_ENUM
                    };
                    self.push_export(
                        exports,
                        &name,
                        "",
                        ExportSyntax::Modifier,
                        flags,
                        ScriptElementKind::EnumElement,
                    );
                }
            }
            NodeData::ModuleDeclaration(data) => {
                if let Some(name) = self.exported_decl_name(&data.modifiers, Some(data.name)) {
                    self.push_export(
                        exports,
                        &name,
                        "",
                        ExportSyntax::Modifier,
                        SymbolFlags::VALUE_MODULE,
                        ScriptElementKind::ModuleElement,
                    );
                }
            }
            NodeData::ExportDeclaration(data) => {
                if let Some(clause) = data.export_clause {
                    self.extract_export_clause(clause, exports);
                }
                // A bare `export * from "..."` has no directly indexable name;
                // its specifier is gathered by `collect_star_reexport_specifiers`.
            }
            NodeData::ExportAssignment(data) => {
                let (export_name, syntax) = if data.is_export_equals {
                    (INTERNAL_SYMBOL_NAME_EXPORT_EQUALS, ExportSyntax::Equals)
                } else {
                    (
                        INTERNAL_SYMBOL_NAME_DEFAULT,
                        ExportSyntax::DefaultDeclaration,
                    )
                };
                // Go follows `SkipOuterExpressions` then resolves the alias via
                // the checker; reachable subset uses the bare expression
                // identifier, falling back to the file name.
                let ident = self.identifier_text(data.expression);
                let local = self.default_local_name(ident.as_deref());
                self.push_export(
                    exports,
                    export_name,
                    &local,
                    syntax,
                    SymbolFlags::ALIAS,
                    ScriptElementKind::Unknown,
                );
            }
            _ => {}
        }
    }

    /// Pushes a value-declaration export, choosing the default-export form when
    /// the declaration carries the `default` modifier.
    fn push_named_or_default(
        &self,
        exports: &mut Vec<Export>,
        name: Option<String>,
        is_default: bool,
        flags: SymbolFlags,
        kind: ScriptElementKind,
    ) {
        if is_default {
            let local = self.default_local_name(name.as_deref());
            self.push_export(
                exports,
                INTERNAL_SYMBOL_NAME_DEFAULT,
                &local,
                ExportSyntax::DefaultModifier,
                flags,
                kind,
            );
        } else if let Some(name) = name {
            self.push_export(exports, &name, "", ExportSyntax::Modifier, flags, kind);
        }
    }

    /// Returns the identifier text of an exported declaration's `name`, or
    /// `None` when the declaration is not exported or its name is not a plain
    /// identifier.
    fn exported_decl_name(
        &self,
        modifiers: &Option<tsgo_ast::ModifierList>,
        name: Option<NodeId>,
    ) -> Option<String> {
        if !modifiers_contain(modifiers, ModifierFlags::EXPORT) {
            return None;
        }
        name.and_then(|n| self.identifier_text(n))
    }

    fn extract_export_clause(&self, clause: NodeId, exports: &mut Vec<Export>) {
        let NodeData::NamedExports(elems) = self.arena.data(clause) else {
            // `export * as ns from "..."` (NamespaceExport) needs checker-driven
            // resolution; deferred.
            return;
        };
        for spec in elems.elements.nodes.clone() {
            let NodeData::ExportSpecifier(spec_data) = self.arena.data(spec) else {
                continue;
            };
            if let Some(name) = self.identifier_text(spec_data.name) {
                self.push_export(
                    exports,
                    &name,
                    "",
                    ExportSyntax::Named,
                    SymbolFlags::ALIAS,
                    ScriptElementKind::Unknown,
                );
            }
        }
    }

    fn extract_variable_statement(&self, declaration_list: NodeId, exports: &mut Vec<Export>) {
        // `const`/`let` are block-scoped; bare `var` is function-scoped.
        let list_flags = self.arena.flags(declaration_list);
        let scope_flags = if list_flags.intersects(NodeFlags::LET | NodeFlags::CONST) {
            SymbolFlags::BLOCK_SCOPED_VARIABLE
        } else {
            SymbolFlags::FUNCTION_SCOPED_VARIABLE
        };
        let NodeData::VariableDeclarationList(list) = self.arena.data(declaration_list) else {
            return;
        };
        for decl in list.declarations.nodes.clone() {
            let NodeData::VariableDeclaration(decl_data) = self.arena.data(decl) else {
                continue;
            };
            if let Some(name) = self.identifier_text(decl_data.name) {
                self.push_export(
                    exports,
                    &name,
                    "",
                    ExportSyntax::Modifier,
                    scope_flags,
                    ScriptElementKind::VariableElement,
                );
            }
        }
    }
}

/// Collects the module-specifier text of every bare `export * from "..."` in the
/// file. These re-export *all* of the target module's names; resolving them
/// needs cross-file context, so the registry handles them separately.
///
/// Side effects: none (reads the arena only).
// Go: internal/ls/autoimport/extract.go:extractFromSymbol (InternalSymbolNameExportStar arm)
pub fn collect_star_reexport_specifiers(arena: &NodeArena, source_file: NodeId) -> Vec<String> {
    let mut specifiers = Vec::new();
    let NodeData::SourceFile(sf) = arena.data(source_file) else {
        return specifiers;
    };
    let statements = sf.statements.nodes.clone();
    for stmt in statements {
        if arena.kind(stmt) != Kind::ExportDeclaration {
            continue;
        }
        let NodeData::ExportDeclaration(decl) = arena.data(stmt) else {
            continue;
        };
        if decl.export_clause.is_none() {
            if let Some(spec) = decl.module_specifier {
                specifiers.push(arena.text(spec).to_string());
            }
        }
    }
    specifiers
}

/// Whether a node's modifier list contains `flag`.
fn modifiers_contain(modifiers: &Option<tsgo_ast::ModifierList>, flag: ModifierFlags) -> bool {
    modifiers
        .as_ref()
        .is_some_and(|m| m.modifier_flags.contains(flag))
}

#[cfg(test)]
#[path = "extract_test.rs"]
mod tests;
